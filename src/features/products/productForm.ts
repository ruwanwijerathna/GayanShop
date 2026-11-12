// Product form state ⇄ the Rust `ProductInput`. Money is typed in rupees and sent as integer cents;
// Rust re-validates everything (names, codes, uniqueness, limits).
import type { CodeChoice, Product, ProductInput, ProductKind } from "@/db/bindings";
import { formatMoney, parseMajor } from "@/lib/money";

export type CodeMode = CodeChoice["mode"];

export interface AttributeRow {
  key: string;
  value: string;
}

export interface ProductFormState {
  kind: ProductKind;
  name: string;
  categoryId: string;
  price: string;
  minPrice: string;
  cost: string;
  quantity: string;
  note: string;
  skuMode: CodeMode;
  sku: string;
  barcodeMode: CodeMode;
  barcode: string;
  trackStock: boolean;
  allowNegative: boolean;
  isActive: boolean;
  attributes: AttributeRow[];
}

export type MoneyField = "price" | "minPrice" | "cost";

export type FormResult = { ok: true; input: ProductInput } | { ok: false; field: MoneyField };

/** A new product: no SKU or barcode by default (None), generate on demand or type/scan. */
export function emptyForm(): ProductFormState {
  return {
    kind: "standard",
    name: "",
    categoryId: "",
    price: "",
    minPrice: "",
    cost: "",
    quantity: "",
    note: "",
    skuMode: "none",
    sku: "",
    barcodeMode: "none",
    barcode: "",
    trackStock: true,
    allowNegative: true,
    isActive: true,
    attributes: [],
  };
}

/** "Rs 1,250.50" → "1250.50" for editing. */
function toMajor(cents: number | null | undefined): string {
  if (cents === null || cents === undefined) return "";
  return formatMoney(cents).replace("Rs ", "").replaceAll(",", "");
}

export function fromProduct(p: Product): ProductFormState {
  return {
    kind: p.kind,
    name: p.name,
    categoryId: p.category_id ?? "",
    price: toMajor(p.price),
    minPrice: toMajor(p.min_price),
    cost: toMajor(p.default_cost),
    quantity: p.on_hand > 0 ? String(p.on_hand) : "",
    note: "",
    skuMode: p.sku ? "manual" : "none",
    sku: p.sku ?? "",
    barcodeMode: p.barcode ? "manual" : "none",
    barcode: p.barcode ?? "",
    trackStock: true,
    allowNegative: true,
    isActive: p.is_active,
    attributes: Object.entries(p.attributes).map(([key, value]) => ({ key, value: value ?? "" })),
  };
}

function code(mode: CodeMode, value: string): CodeChoice {
  if (mode === "generate") return { mode: "generate" };
  const trimmed = value.trim();
  if (trimmed !== "") return { mode: "manual", value: trimmed };
  return { mode: "none" };
}

/** Build the command input, or name the money field that doesn't parse. `canViewCost` false ⇒ cost untouched. */
export function toInput(f: ProductFormState, canViewCost: boolean): FormResult {
  const price = parseMajor(f.price);
  if (price === null) return { ok: false, field: "price" };
  let minPrice: number | null = null;
  if (f.minPrice.trim() !== "") {
    minPrice = parseMajor(f.minPrice);
    if (minPrice === null) return { ok: false, field: "minPrice" };
  }
  let cost: number | null = null;
  if (canViewCost && f.cost.trim() !== "") {
    cost = parseMajor(f.cost);
    if (cost === null) return { ok: false, field: "cost" };
  }
  const attributes: Record<string, string> = {};
  for (const row of f.attributes) {
    if (row.key.trim() === "" && row.value.trim() === "") continue;
    attributes[row.key] = row.value;
  }
  return {
    ok: true,
    input: {
      kind: f.kind,
      name: f.name,
      // One name for screens, receipts and labels: no separate Sinhala name is kept.
      name_si: null,
      sku: code(f.skuMode, f.sku),
      barcode: code(f.barcodeMode, f.barcode),
      category_id: f.categoryId === "" ? null : f.categoryId,
      default_cost: cost,
      price,
      min_price: minPrice,
      track_stock: f.trackStock,
      allow_negative_stock: f.allowNegative,
      attributes,
      is_active: f.isActive,
    },
  };
}

/** Client-side list filter (the bounded product list is loaded once, doc §1.4). */
export function matchesText(p: Product, text: string): boolean {
  const q = text.trim().toLowerCase();
  if (q === "") return true;
  return [p.name, p.name_si, p.sku, p.barcode].some((v) => v?.toLowerCase().includes(q) ?? false);
}
