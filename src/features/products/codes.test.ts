import { describe, expect, it } from "vitest";
import type { Product } from "@/db/bindings";
import { deviceDigit, ean13CheckDigit, nextBarcode, nextSku } from "./codes";

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
    expect(nextSku(2, [p(null, "2200000000419")], "")).toBe("200042");
    expect(nextSku(1, [], "100005")).toBe("100006");
  });

  it("builds a valid in-store EAN-13", () => {
    expect(ean13CheckDigit("220000000001")).toBe(9);
    expect(nextBarcode(2, [], "")).toBe("2200000000019");
    const code = nextBarcode(1, [p("100003", null)], "");
    expect(code).toHaveLength(13);
    expect(code.startsWith("210000000004")).toBe(true);
    expect(code.charCodeAt(12) - 48).toBe(ean13CheckDigit(code.slice(0, 12)));
  });
});
