import { describe, expect, it } from "vitest";
import type { Product_Serialize as Product } from "@/db/bindings";
import { emptyForm, fromProduct, matchesText, toInput } from "./productForm";

const product: Product = {
  id: "p1",
  kind: "standard",
  name: "Blue Bucket 10L",
  name_si: "නිල් බාල්දිය",
  sku: "100001",
  barcode: "2100000000017",
  category_id: "c1",
  default_cost: 20_000,
  price: 45_050,
  min_price: 40_000,
  track_stock: true,
  allow_negative_stock: true,
  attributes: { size: "10L" },
  is_active: true,
  barcode_conflict: false,
  created_at: "2026-11-08T09:00:00.000Z",
  updated_at: "2026-11-08T09:00:00.000Z",
  version: 1,
  on_hand: 0,
};

describe("product form", () => {
  it("starts a new product with no SKU and no barcode by default (None state)", () => {
    const f = { ...emptyForm(), name: "Soap", price: "120" };
    const r = toInput(f, true);
    expect(r).toEqual({
      ok: true,
      input: expect.objectContaining({
        name: "Soap",
        name_si: null,
        price: 12_000,
        min_price: null,
        default_cost: null,
        sku: { mode: "none" },
        barcode: { mode: "none" },
        category_id: null,
        attributes: {},
      }),
    });
  });

  it("handles code generation and manual codes for SKU and barcode", () => {
    const gen = toInput(
      { ...emptyForm(), name: "Soap", price: "120", skuMode: "generate", barcodeMode: "generate" },
      true,
    );
    expect(gen.ok && [gen.input.sku, gen.input.barcode]).toEqual([
      { mode: "generate" },
      { mode: "generate" },
    ]);

    const manual = toInput(
      { ...emptyForm(), name: "Soap", price: "120", sku: "SKU101", barcode: "4791234567890" },
      true,
    );
    expect(manual.ok && [manual.input.sku, manual.input.barcode]).toEqual([
      { mode: "manual", value: "SKU101" },
      { mode: "manual", value: "4791234567890" },
    ]);
  });

  it("converts typed rupees to integer cents and names the field that doesn't parse", () => {
    const base = { ...emptyForm(), name: "Soap" };
    expect(toInput({ ...base, price: "" }, true)).toEqual({ ok: false, field: "price" });
    expect(toInput({ ...base, price: "12.345" }, true)).toEqual({ ok: false, field: "price" });
    expect(toInput({ ...base, price: "100", minPrice: "abc" }, true)).toEqual({
      ok: false,
      field: "minPrice",
    });
    expect(toInput({ ...base, price: "100", cost: "-1" }, true)).toEqual({ ok: false, field: "cost" });
    const ok = toInput({ ...base, price: "1,250.5", minPrice: "1000", cost: "800.75" }, true);
    expect(ok.ok && [ok.input.price, ok.input.min_price, ok.input.default_cost]).toEqual([
      125_050, 100_000, 80_075,
    ]);
  });

  it("never sends a cost for a viewer without VIEW_COST, even a typed one", () => {
    const r = toInput({ ...emptyForm(), name: "Soap", price: "100", cost: "50" }, false);
    expect(r.ok && r.input.default_cost).toBeNull();
  });

  it("round-trips an existing product, keeping its codes as manual values", () => {
    const f = fromProduct(product);
    expect(f.price).toBe("450.50");
    expect(f.cost).toBe("200.00");
    expect([f.skuMode, f.barcodeMode]).toEqual(["manual", "manual"]);
    const r = toInput(f, true);
    expect(r).toEqual({
      ok: true,
      input: {
        kind: "standard",
        name: "Blue Bucket 10L",
        name_si: null, // one product name only: a stored Sinhala name is dropped on save
        sku: { mode: "manual", value: "100001" },
        barcode: { mode: "manual", value: "2100000000017" },
        category_id: "c1",
        default_cost: 20_000,
        price: 45_050,
        min_price: 40_000,
        track_stock: true,
        allow_negative_stock: true,
        attributes: { size: "10L" },
        is_active: true,
      },
    });
  });

  it("masks cost in the form when the list arrived without it, and drops blank attribute rows", () => {
    const f = fromProduct({ ...product, default_cost: null, sku: null });
    expect(f.cost).toBe("");
    expect(f.skuMode).toBe("none");
    const r = toInput({ ...f, attributes: [...f.attributes, { key: " ", value: "" }] }, false);
    expect(r.ok && r.input.attributes).toEqual({ size: "10L" });
    expect(r.ok && r.input.sku).toEqual({ mode: "none" });
  });

  it("filters the list by English or Sinhala name, SKU or barcode, case-insensitively", () => {
    expect(matchesText(product, "")).toBe(true);
    expect(matchesText(product, "bucket")).toBe(true);
    expect(matchesText(product, "බාල්")).toBe(true);
    expect(matchesText(product, "00001")).toBe(true);
    expect(matchesText(product, "21000000")).toBe(true);
    expect(matchesText(product, "basin")).toBe(false);
  });
});
