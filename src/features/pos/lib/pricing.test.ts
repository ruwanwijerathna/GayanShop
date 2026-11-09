// The till preview must equal Rust exactly: the golden carts are priced by `domain::pricing` and written by
// `cargo test --test checkout_props ui_preview` (UPDATE_FIXTURES=1).
import { describe, expect, it } from "vitest";
import fixtures from "../../../../src-tauri/tests/fixtures/pricing_cases.json";
import { apportion, mulDivRound, priceLines, withinLimit } from "./pricing";

describe("pricing preview == Rust", () => {
  it.each(fixtures.cases.map((c) => [c.name, c] as const))("%s", (_name, c) => {
    const r = priceLines(c.input.lines, c.input.bill_discount, c.input.tax_inclusive);
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.order).toEqual(c.expected);
  });

  it("has a meaningful number of golden carts", () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(10);
  });
});

describe("arithmetic", () => {
  it("rounds half away from zero like Rust", () => {
    expect(mulDivRound(5, 1, 2)).toBe(3);
    expect(mulDivRound(-5, 1, 2)).toBe(-3);
    expect(mulDivRound(4, 1, 3)).toBe(1);
  });

  it("splits a bill discount exactly by largest remainder, ties to the lower index", () => {
    expect(apportion(1000, [1, 1, 1])).toEqual([334, 333, 333]);
    expect(apportion(0, [5, 5])).toEqual([0, 0]);
    expect(apportion(10, [0, 0])).toBeNull();
    const shares = apportion(3_001, [10_001, 20_003, 30_007]) ?? [];
    expect(shares.reduce((a, b) => a + b, 0)).toBe(3_001);
  });

  it("stays exact beyond 2^53 intermediate products", () => {
    const shares = apportion(9_000_000_000, [9_000_000_000, 1]) ?? [];
    expect(shares).toEqual([8_999_999_999, 1]);
  });

  it("checks the 5% limit by cross-multiplication", () => {
    expect(withinLimit(500, 10_000, 500)).toBe(true);
    expect(withinLimit(501, 10_000, 500)).toBe(false);
    expect(withinLimit(0, 0, 0)).toBe(true);
  });

  it("refuses carts Rust would refuse", () => {
    expect(priceLines([], 0, true)).toEqual({ ok: false, error: "no_lines" });
    const line = { unit_price: 100, qty: 1, line_discount: 101, tax_rate_bp: 0 };
    expect(priceLines([line], 0, true)).toMatchObject({ ok: false, error: "line_discount" });
    expect(priceLines([{ ...line, line_discount: 0 }], 101, true)).toMatchObject({
      ok: false,
      error: "bill_discount",
    });
  });
});
