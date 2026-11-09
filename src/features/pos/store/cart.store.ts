// The till cart (doc §1.4): ephemeral UI state in Zustand. Every change is a functional update, so two fast
// scans of the same code can't overwrite each other (`qty + 1`). Rust re-prices the whole cart at checkout;
// these lines only carry what the cashier did (qty, price override, discounts).
import { z } from "zod";
import { createStore, useStore } from "zustand";
import type {
  CheckoutLine_Deserialize as CheckoutLine,
  ProductSummary_Serialize as ProductSummary,
} from "@/db/bindings";
import type { Money } from "@/lib/money";
import { priceLines, type PricedOrder } from "../lib/pricing";

export const MAX_LINE_QTY = 9_999;

const lineSchema = z.object({
  key: z.string(),
  productId: z.string().nullable(),
  name: z.string(),
  listPrice: z.number().int().nonnegative(),
  minPrice: z.number().int().nonnegative().nullable(),
  unitPrice: z.number().int().nonnegative(),
  qty: z.number().int().positive(),
  lineDiscount: z.number().int().nonnegative(),
  overrideReason: z.string().nullable(),
  unitCost: z.number().int().nonnegative().nullable(),
});
export type CartLine = z.infer<typeof lineSchema>;

/** What `held_carts` stores (crash recovery, hold / recall). */
export const payloadSchema = z.object({
  v: z.literal(1),
  token: z.string().min(1),
  lines: z.array(lineSchema),
  billDiscount: z.number().int().nonnegative(),
  customerPhone: z.string(),
  customerName: z.string(),
});
export type CartPayload = z.infer<typeof payloadSchema>;

export interface QuickSaleLine {
  name: string;
  qty: number;
  unitPrice: Money;
  unitCost?: Money | null;
}

export interface CartState {
  token: string;
  lines: CartLine[];
  billDiscount: Money;
  customerPhone: string;
  customerName: string;
  /** The line the cashier is working on (F6 edits it); the newest line by default. */
  selectedKey: string | null;
  /** Bumped on every add so the touched line can flash even when its key is unchanged. */
  touch: { key: string; seq: number } | null;

  addProduct: (p: ProductSummary) => void;
  addQuickSale: (q: QuickSaleLine) => void;
  setQty: (key: string, qty: number) => void;
  increment: (key: string, by: number) => void;
  removeLine: (key: string) => void;
  select: (key: string | null) => void;
  setLineDiscount: (key: string, discount: Money) => void;
  overridePrice: (key: string, unitPrice: Money, reason: string | null) => void;
  setBillDiscount: (discount: Money) => void;
  setCustomer: (phone: string, name: string) => void;
  /** A fresh cart with a new checkout token (after a sale, or Esc on an empty-confirm). */
  clear: () => void;
  restore: (payload: CartPayload) => void;
  snapshot: () => CartPayload;
}

export function newToken(): string {
  return crypto.randomUUID();
}

function clampQty(qty: number): number {
  return Math.min(MAX_LINE_QTY, Math.max(1, Math.trunc(qty)));
}

let quickSeq = 0;

export function createCartStore(token: () => string = newToken) {
  return createStore<CartState>()((set, get) => {
    const bump = (s: CartState, key: string) => ({ key, seq: (s.touch?.seq ?? 0) + 1 });
    const mapLine = (key: string, f: (l: CartLine) => CartLine) =>
      set((s) => ({ lines: s.lines.map((l) => (l.key === key ? f(l) : l)) }));
    return {
      token: token(),
      lines: [],
      billDiscount: 0,
      customerPhone: "",
      customerName: "",
      selectedKey: null,
      touch: null,

      addProduct: (p) =>
        set((s) => {
          // A repeat scan bumps the line still at list price; an edited line stays as the cashier left it.
          const same = s.lines.find(
            (l) => l.productId === p.id && l.unitPrice === p.price && l.lineDiscount === 0,
          );
          if (same) {
            return {
              lines: s.lines.map((l) => (l === same ? { ...l, qty: clampQty(l.qty + 1) } : l)),
              selectedKey: same.key,
              touch: bump(s, same.key),
            };
          }
          const key = s.lines.some((l) => l.key === p.id) ? `${p.id}#${s.lines.length}` : p.id;
          const line: CartLine = {
            key,
            productId: p.id,
            name: p.name,
            listPrice: p.price,
            minPrice: p.min_price,
            unitPrice: p.price,
            qty: 1,
            lineDiscount: 0,
            overrideReason: null,
            unitCost: null,
          };
          return { lines: [...s.lines, line], selectedKey: key, touch: bump(s, key) };
        }),

      addQuickSale: (q) =>
        set((s) => {
          quickSeq += 1;
          const key = `qs:${quickSeq}:${s.lines.length}`;
          const line: CartLine = {
            key,
            productId: null,
            name: q.name,
            listPrice: q.unitPrice,
            minPrice: null,
            unitPrice: q.unitPrice,
            qty: clampQty(q.qty),
            lineDiscount: 0,
            overrideReason: null,
            unitCost: q.unitCost ?? null,
          };
          return { lines: [...s.lines, line], selectedKey: key, touch: bump(s, key) };
        }),

      setQty: (key, qty) =>
        mapLine(key, (l) => ({
          ...l,
          qty: clampQty(qty),
          lineDiscount: Math.min(l.lineDiscount, l.unitPrice * clampQty(qty)),
        })),
      increment: (key, by) =>
        set((s) => ({
          lines: s.lines.map((l) => {
            if (l.key !== key) return l;
            const qty = clampQty(l.qty + by);
            return { ...l, qty, lineDiscount: Math.min(l.lineDiscount, l.unitPrice * qty) };
          }),
          touch: by > 0 ? bump(s, key) : s.touch,
        })),
      removeLine: (key) =>
        set((s) => {
          const lines = s.lines.filter((l) => l.key !== key);
          return {
            lines,
            selectedKey: s.selectedKey === key ? (lines.at(-1)?.key ?? null) : s.selectedKey,
            billDiscount: lines.length === 0 ? 0 : s.billDiscount,
          };
        }),
      select: (key) => set({ selectedKey: key }),
      setLineDiscount: (key, discount) =>
        mapLine(key, (l) => ({ ...l, lineDiscount: Math.max(0, Math.min(discount, l.unitPrice * l.qty)) })),
      overridePrice: (key, unitPrice, reason) =>
        mapLine(key, (l) => {
          const price = Math.max(0, unitPrice);
          const overridden = price < l.listPrice;
          return {
            ...l,
            unitPrice: price,
            overrideReason: overridden ? reason : null,
            lineDiscount: Math.min(l.lineDiscount, price * l.qty),
          };
        }),
      setBillDiscount: (discount) => set({ billDiscount: Math.max(0, discount) }),
      setCustomer: (phone, name) => set({ customerPhone: phone, customerName: name }),
      clear: () =>
        set({
          token: token(),
          lines: [],
          billDiscount: 0,
          customerPhone: "",
          customerName: "",
          selectedKey: null,
          touch: null,
        }),
      restore: (p) =>
        set({
          token: p.token,
          lines: p.lines,
          billDiscount: p.billDiscount,
          customerPhone: p.customerPhone,
          customerName: p.customerName,
          selectedKey: p.lines.at(-1)?.key ?? null,
          touch: null,
        }),
      snapshot: () => {
        const s = get();
        return {
          v: 1,
          token: s.token,
          lines: s.lines,
          billDiscount: s.billDiscount,
          customerPhone: s.customerPhone,
          customerName: s.customerName,
        };
      },
    };
  });
}

export type CartStore = ReturnType<typeof createCartStore>;

/** Parse a stored payload; `null` when it is not a cart this version understands. */
export function parsePayload(raw: string): CartPayload | null {
  try {
    const parsed = payloadSchema.safeParse(JSON.parse(raw));
    return parsed.success ? parsed.data : null;
  } catch {
    return null;
  }
}

/** The preview totals for the cart (tax-inclusive at 0%, doc §11). */
export function previewTotals(s: Pick<CartState, "lines" | "billDiscount">): PricedOrder | null {
  if (s.lines.length === 0) return null;
  const r = priceLines(
    s.lines.map((l) => ({
      unit_price: l.unitPrice,
      qty: l.qty,
      line_discount: l.lineDiscount,
      tax_rate_bp: 0,
    })),
    Math.min(
      s.billDiscount,
      s.lines.reduce((a, l) => a + l.unitPrice * l.qty - l.lineDiscount, 0),
    ),
    true,
  );
  return r.ok ? r.order : null;
}

/** The cart as `checkout_order` lines. */
export function toCheckoutLines(lines: CartLine[]): CheckoutLine[] {
  return lines.map((l) => ({
    product_id: l.productId,
    name: l.productId === null ? l.name : null,
    unit_price: l.unitPrice,
    qty: l.qty,
    line_discount: l.lineDiscount,
    unit_cost: l.productId === null ? l.unitCost : null,
    override_reason: l.overrideReason,
  }));
}

/** The till's one cart. */
export const cartStore = createCartStore();

export function useCart<T>(selector: (s: CartState) => T): T {
  return useStore(cartStore, selector);
}
