import { describe, expect, it } from "vitest";
import { parseDiscount } from "./discountInput";

describe("parseDiscount", () => {
  it("reads amounts", () => {
    expect(parseDiscount("150", 100_000)).toBe(15_000);
    expect(parseDiscount("99.5", 100_000)).toBe(9_950);
    expect(parseDiscount("", 100_000)).toBe(0);
  });

  it("reads percentages and rounds down to stay within a limit", () => {
    expect(parseDiscount("5%", 100_000)).toBe(5_000);
    expect(parseDiscount("5 %", 60_011)).toBe(3_000);
    expect(parseDiscount("2.5%", 10_000)).toBe(250);
    expect(parseDiscount("100%", 4_321)).toBe(4_321);
  });

  it("rejects nonsense", () => {
    expect(parseDiscount("101%", 1_000)).toBeNull();
    expect(parseDiscount("abc", 1_000)).toBeNull();
    expect(parseDiscount("-5", 1_000)).toBeNull();
  });
});
