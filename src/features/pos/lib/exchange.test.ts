import { describe, expect, it } from "vitest";
import type { ExchangeLine, ExchangeSource } from "@/db/bindings";
import { amountDue, lineCredit, portion, toExchangeInput, totalCredit } from "./exchange";

const line = (over: Partial<ExchangeLine> = {}): ExchangeLine => ({
  item_id: "i1",
  name: "Bucket",
  qty: 3,
  line_total: 10_000,
  returned_qty: 0,
  returnable_qty: 3,
  eligible: true,
  ...over,
});

const source = (lines: ExchangeLine[]): ExchangeSource => ({
  order_id: "o1",
  receipt_no: "T01-20261112-0001",
  business_date: "2026-11-12",
  days_since: 3,
  window_days: 7,
  late: false,
  lines,
});

describe("portion", () => {
  it("splits a line exactly over any partition of its units", () => {
    expect([portion(10_000, 3, 0, 1), portion(10_000, 3, 1, 2), portion(10_000, 3, 2, 3)]).toEqual([
      3_333, 3_333, 3_334,
    ]);
    expect(portion(10_000, 3, 0, 2) + portion(10_000, 3, 2, 3)).toBe(10_000);
  });

  it("is zero for an empty or out-of-range slice", () => {
    expect(portion(100, 3, 2, 2)).toBe(0);
    expect(portion(100, 3, 0, 4)).toBe(0);
    expect(portion(100, 0, 0, 1)).toBe(0);
  });

  it("does not lose precision past 2^53", () => {
    expect(portion(9_007_199_254_740_991, 3, 0, 3)).toBe(9_007_199_254_740_991);
  });
});

describe("credit", () => {
  it("values later units after the ones already returned", () => {
    const l = line({ returned_qty: 1, returnable_qty: 2 });
    expect(lineCredit(l, 2)).toBe(portion(10_000, 3, 1, 3));
    expect(lineCredit(l, 5)).toBe(lineCredit(l, 2));
  });

  it("ignores ineligible lines and sums the rest", () => {
    const src = source([line(), line({ item_id: "i2", eligible: false })]);
    expect(totalCredit(src, { i1: 1, i2: 3 })).toBe(3_333);
  });

  it("builds the return list from the picks only", () => {
    const src = source([line(), line({ item_id: "i2" })]);
    expect(toExchangeInput(src, { i2: 2, i1: 0 })).toEqual({ returns: [{ item_id: "i2", qty: 2 }] });
  });
});

describe("amountDue", () => {
  it("charges only the difference and never goes negative", () => {
    expect(amountDue(12_000, 10_000)).toEqual({ due: 2_000, short: false });
    expect(amountDue(10_000, 10_000)).toEqual({ due: 0, short: false });
    expect(amountDue(8_000, 10_000)).toEqual({ due: 0, short: true });
  });
});
