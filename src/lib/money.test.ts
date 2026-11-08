import { describe, expect, it } from "vitest";
import { formatLabelPrice, formatMoney, parseMajor } from "./money";

describe("formatMoney", () => {
  it("formats LKR cents like the Rust core", () => {
    expect(formatMoney(0)).toBe("Rs 0.00");
    expect(formatMoney(123_450)).toBe("Rs 1,234.50");
    expect(formatMoney(100_000_000)).toBe("Rs 1,000,000.00");
    expect(formatMoney(-505)).toBe("-Rs 5.05");
  });

  it("rejects non-integer amounts", () => {
    expect(() => formatMoney(1.5)).toThrow(RangeError);
  });
});

describe("formatLabelPrice", () => {
  it("drops .00 for whole rupees", () => {
    expect(formatLabelPrice(125_000)).toBe("Rs. 1,250");
    expect(formatLabelPrice(125_050)).toBe("Rs. 1,250.50");
  });
});

describe("parseMajor", () => {
  it("parses typed amounts into cents", () => {
    expect(parseMajor("450")).toBe(45_000);
    expect(parseMajor("450.5")).toBe(45_050);
    expect(parseMajor("1,250.75")).toBe(125_075);
    expect(parseMajor(".5")).toBe(50);
  });

  it("rejects invalid input", () => {
    for (const bad of ["10L", "1.234", "-5", "", ".", "1e3", "Rs 5"]) expect(parseMajor(bad)).toBeNull();
  });

  it("round-trips with formatMoney", () => {
    for (const cents of [0, 1, 99, 100, 45_050, 123_456_789]) {
      expect(parseMajor(formatMoney(cents).replace("Rs ", ""))).toBe(cents);
    }
  });
});
