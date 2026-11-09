import { describe, expect, it } from "vitest";
import { parseQuickSale } from "./parseQuickSale";

describe("parseQuickSale", () => {
  it.each([
    ["Blue bucket 450", 1, "Blue bucket", 45_000],
    ["3* Bucket 10L 450", 3, "Bucket 10L", 45_000],
    ["3 * Bucket 10L 450", 3, "Bucket 10L", 45_000],
    ["  Plastic   basin  99.5 ", 1, "Plastic basin", 9_950],
    ["Shoe 1250.75", 1, "Shoe", 125_075],
    ["නිල් බාල්දිය 300", 1, "නිල් බාල්දිය", 30_000],
    ["Socks 0", 1, "Socks", 0],
  ])("%s", (input, qty, name, unitPrice) => {
    expect(parseQuickSale(input)).toEqual({ ok: true, qty, name, unitPrice });
  });

  it("never reads a size as the price", () => {
    expect(parseQuickSale("Bucket 10L")).toEqual({ ok: false, reason: "price_missing" });
    expect(parseQuickSale("Bucket 450.123")).toEqual({ ok: false, reason: "price_missing" });
    expect(parseQuickSale("Bucket 1,250")).toEqual({ ok: false, reason: "price_missing" });
  });

  it("rejects what can't be sold", () => {
    expect(parseQuickSale("   ")).toEqual({ ok: false, reason: "empty" });
    expect(parseQuickSale("450")).toEqual({ ok: false, reason: "name_missing" });
    expect(parseQuickSale("3* 450")).toEqual({ ok: false, reason: "name_missing" });
    expect(parseQuickSale("0* Cup 100")).toEqual({ ok: false, reason: "qty_invalid" });
    expect(parseQuickSale("10000* Cup 100")).toEqual({ ok: false, reason: "qty_invalid" });
    expect(parseQuickSale(`${"x".repeat(81)} 100`)).toEqual({ ok: false, reason: "name_too_long" });
  });
});
