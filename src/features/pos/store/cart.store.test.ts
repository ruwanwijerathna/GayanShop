import { describe, expect, it } from "vitest";
import type { ProductSummary_Serialize as ProductSummary } from "@/db/bindings";
import { createCartStore, parsePayload, previewTotals, toCheckoutLines } from "./cart.store";

const product = (id: string, price: number, min: number | null = null): ProductSummary => ({
  id,
  kind: "standard",
  name: `P ${id}`,
  name_si: null,
  sku: null,
  barcode: null,
  price,
  min_price: min,
  track_stock: true,
});

function store() {
  let n = 0;
  return createCartStore(() => `tok-${++n}`);
}

describe("cart store", () => {
  it("adds a product once and bumps qty on repeat scans", () => {
    const s = store();
    s.getState().addProduct(product("a", 45_000));
    s.getState().addProduct(product("a", 45_000));
    s.getState().addProduct(product("b", 100));
    const { lines, selectedKey, touch } = s.getState();
    expect(lines.map((l) => [l.key, l.qty])).toEqual([
      ["a", 2],
      ["b", 1],
    ]);
    expect(selectedKey).toBe("b");
    expect(touch).toEqual({ key: "b", seq: 3 });
  });

  it("never loses a scan: repeated functional updates all land", () => {
    const s = store();
    const mug = product("mug", 25_000);
    for (let i = 0; i < 50; i++) s.getState().addProduct(mug);
    expect(s.getState().lines[0]?.qty).toBe(50);
  });

  it("starts a new line when the existing one was re-priced", () => {
    const s = store();
    s.getState().addProduct(product("a", 1_000));
    s.getState().overridePrice("a", 900, "damaged box");
    s.getState().addProduct(product("a", 1_000));
    expect(s.getState().lines.map((l) => [l.key, l.unitPrice, l.qty, l.overrideReason])).toEqual([
      ["a", 900, 1, "damaged box"],
      ["a#1", 1_000, 1, null],
    ]);
  });

  it("adds quick sales as their own lines", () => {
    const s = store();
    s.getState().addQuickSale({ name: "Bucket 10L", qty: 3, unitPrice: 45_000, unitCost: 20_000 });
    s.getState().addQuickSale({ name: "Bucket 10L", qty: 1, unitPrice: 45_000 });
    expect(s.getState().lines).toHaveLength(2);
    expect(toCheckoutLines(s.getState().lines)[0]).toEqual({
      product_id: null,
      name: "Bucket 10L",
      unit_price: 45_000,
      qty: 3,
      line_discount: 0,
      unit_cost: 20_000,
      override_reason: null,
    });
  });

  it("clamps qty and discounts to what the line can take", () => {
    const s = store();
    s.getState().addProduct(product("a", 1_000));
    s.getState().setLineDiscount("a", 5_000);
    expect(s.getState().lines[0]?.lineDiscount).toBe(1_000);
    s.getState().increment("a", 2);
    s.getState().setLineDiscount("a", 2_500);
    s.getState().setQty("a", 1);
    expect(s.getState().lines[0]).toMatchObject({ qty: 1, lineDiscount: 1_000 });
    s.getState().setQty("a", 0);
    expect(s.getState().lines[0]?.qty).toBe(1);
    s.getState().setQty("a", 1e9);
    expect(s.getState().lines[0]?.qty).toBe(9_999);
  });

  it("clears the override reason when the price goes back to list", () => {
    const s = store();
    s.getState().addProduct(product("a", 1_000));
    s.getState().overridePrice("a", 800, "bent");
    s.getState().overridePrice("a", 1_000, "ignored");
    expect(s.getState().lines[0]).toMatchObject({ unitPrice: 1_000, overrideReason: null });
  });

  it("previews totals with the bill discount apportioned", () => {
    const s = store();
    s.getState().addProduct(product("a", 10_000));
    s.getState().addProduct(product("b", 20_000));
    s.getState().setBillDiscount(1_500);
    const t = previewTotals(s.getState());
    expect(t?.grand_total).toBe(28_500);
    expect(t?.lines.map((l) => l.order_discount_share)).toEqual([500, 1_000]);
    expect(previewTotals({ lines: [], billDiscount: 0 })).toBeNull();
  });

  it("removes lines and resets the bill discount with the last one", () => {
    const s = store();
    s.getState().addProduct(product("a", 1_000));
    s.getState().setBillDiscount(100);
    s.getState().removeLine("a");
    expect(s.getState()).toMatchObject({ lines: [], billDiscount: 0, selectedKey: null });
  });

  it("clears to a fresh token and round-trips through the held-cart payload", () => {
    const s = store();
    s.getState().addProduct(product("a", 1_000));
    s.getState().setCustomer("0771234567", "Saman");
    const snap = s.getState().snapshot();
    expect(snap.token).toBe("tok-1");
    s.getState().clear();
    expect(s.getState().token).toBe("tok-2");
    expect(s.getState().lines).toEqual([]);

    const parsed = parsePayload(JSON.stringify(snap));
    expect(parsed).toEqual(snap);
    if (parsed) s.getState().restore(parsed);
    expect(s.getState()).toMatchObject({ token: "tok-1", customerName: "Saman", selectedKey: "a" });
    expect(parsePayload("{}")).toBeNull();
    expect(parsePayload("not json")).toBeNull();
  });

  it("keeps an exchange out of the held-cart payload and drops it on clear", () => {
    const s = store();
    s.getState().addProduct(product("a", 1_000));
    s.getState().setExchange({
      source: {
        order_id: "o1",
        receipt_no: "T01-20261112-0001",
        business_date: "2026-11-12",
        days_since: 2,
        window_days: 7,
        late: false,
        lines: [],
      },
      picks: { i1: 1 },
    });
    expect(s.getState().exchange?.picks).toEqual({ i1: 1 });
    expect(s.getState().snapshot()).not.toHaveProperty("exchange");
    s.getState().clear();
    expect(s.getState().exchange).toBeNull();
  });
});
