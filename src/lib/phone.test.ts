import { describe, expect, it } from "vitest";
import { formatNational, maskForReceipt, normalizeLk } from "./phone";

describe("normalizeLk (mirrors domain::phone)", () => {
  it("accepts the common Sri Lankan forms", () => {
    for (const input of [
      "0771234567",
      "771234567",
      "94771234567",
      "+94771234567",
      "+94 77 123 4567",
      "(077) 123-4567",
    ]) {
      expect(normalizeLk(input)).toBe("+94771234567");
    }
    expect(normalizeLk("0112345678")).toBe("+94112345678");
  });

  it("rejects everything else", () => {
    for (const bad of [
      "077-ABC-4567",
      "07712345",
      "+447700900123",
      "0071234567",
      "+0771234567",
      "1+771234567",
      "",
    ]) {
      expect(normalizeLk(bad)).toBeNull();
    }
  });
});

describe("masking and formatting", () => {
  it("masks for receipts and formats for screens", () => {
    expect(maskForReceipt("+94771234123")).toBe("07X XXX X123");
    expect(formatNational("+94771234567")).toBe("077 123 4567");
    expect(maskForReceipt("+4477")).toBeNull();
  });
});
