import { describe, expect, it } from "vitest";
import type { Product } from "@/db/bindings";
import { deviceDigit, ean8CheckDigit, nextBarcode, nextSku } from "./codes";

const p = (sku: string | null, barcode: string | null) => ({ sku, barcode }) as Product;

describe("code generation (mirrors Rust receipt_no)", () => {
  it("maps device codes to a digit, defaulting to 1", () => {
    expect(deviceDigit("T03")).toBe(3);
    expect(deviceDigit("T10")).toBe(1);
    expect(deviceDigit(null)).toBe(1);
  });

  it("starts at 00001 and continues above SKUs and barcodes already used", () => {
    expect(nextSku(1, [], "")).toBe("100001");
    expect(nextSku(1, [p("100007", null), p("200010", null)], "")).toBe("100008");
    expect(nextSku(2, [p(null, "22000419")], "")).toBe("200042");
    expect(nextSku(2, [p(null, "2200000000419")], "")).toBe("200042");
    expect(nextSku(1, [], "100005")).toBe("100006");
  });

  it("builds a short 8-digit in-store barcode with an EAN-8 check digit", () => {
    expect(ean8CheckDigit("9638507")).toBe(4); // 96385074, the published EAN-8 example
    // Counter 1, sequence 42: 2·3 + 1 + 0 + 0 + 0 + 4 + 2·3 = 17 ⇒ check 3 (Rust: in_store_barcode(1, 42)).
    expect(nextBarcode(1, [p("100041", null)], "")).toBe("21000423");
    expect(nextBarcode(2, [], "")).toBe("22000019");
    const code = nextBarcode(1, [p("100003", null)], "");
    expect(code).toHaveLength(8);
    expect(code.startsWith("2100004")).toBe(true);
    expect(code.charCodeAt(7) - 48).toBe(ean8CheckDigit(code.slice(0, 7)));
  });

  it("never reissues a number from an older 13-digit in-store barcode", () => {
    expect(nextBarcode(1, [p(null, "2100000000074")], "")).toBe(`2100008${ean8CheckDigit("2100008")}`);
    expect(nextBarcode(1, [], "21000423")).toBe(`2100043${ean8CheckDigit("2100043")}`);
  });
});
