import { describe, expect, it } from "vitest";
import { isBillNumber } from "./receipt";

describe("isBillNumber", () => {
  it("recognises the short and the old bill numbers", () => {
    expect(isBillNumber("T01-261121004")).toBe(true);
    expect(isBillNumber(" t02-2611211000 ")).toBe(true);
    expect(isBillNumber("T01-20261121-0004")).toBe(true);
  });
  it("leaves product barcodes and other text alone", () => {
    expect(isBillNumber("2000123456781")).toBe(false);
    expect(isBillNumber("T01-4")).toBe(false);
    expect(isBillNumber("T01-26112")).toBe(false);
    expect(isBillNumber("MUG-001")).toBe(false);
  });
});
