// Till preview of the order totals (doc §3.7), an exact port of `domain::pricing::price_lines` and
// `domain::discount::apportion`. Rust is authoritative; `pricing.test.ts` checks this against the golden
// carts Rust writes to src-tauri/tests/fixtures/pricing_cases.json. Products of amounts and weights can exceed
// 2^53, so the arithmetic that multiplies runs in BigInt.
import type { Money } from "@/lib/money";

export interface PreviewLine {
  unit_price: Money;
  qty: number;
  line_discount: Money;
  tax_rate_bp: number;
}

export interface PricedLine {
  line_gross: Money;
  line_discount: Money;
  order_discount_share: Money;
  line_total: Money;
  tax_amount: Money;
  line_net: Money;
}

export interface PricedOrder {
  lines: PricedLine[];
  gross_sales: Money;
  discount_total: Money;
  tax_total: Money;
  net_sales: Money;
  grand_total: Money;
}

export type PricingError =
  "no_lines" | "qty" | "price" | "tax" | "line_discount" | "bill_discount" | "overflow";

/** A BigInt result back to cents (results here always fit below 2^53). */
function cents(b: bigint): number {
  return Number.parseInt(b.toString(), 10);
}

/** a × b / d rounded half away from zero (mirrors `money::mul_div_round`). */
export function mulDivRound(a: number, b: number, d: number): number {
  const n = BigInt(a) * BigInt(b);
  const den = BigInt(d);
  const q = n / den;
  const r = n % den;
  const twice = (r < 0n ? -r : r) * 2n;
  const away = twice >= (den < 0n ? -den : den) ? 1n : 0n;
  const sign = n < 0n !== den < 0n ? -1n : 1n;
  return cents(q + away * sign);
}

/** Largest-remainder split of `total` by `weights`; ties go to the lower index (mirrors `discount::apportion`). */
export function apportion(total: Money, weights: Money[]): Money[] | null {
  if (total < 0 || weights.length === 0 || weights.some((w) => w < 0)) return null;
  const sum = weights.reduce((acc, w) => acc + BigInt(w), 0n);
  if (total === 0) return weights.map(() => 0);
  if (sum === 0n) return null;
  const shares: bigint[] = [];
  const rema: { r: bigint; i: number }[] = [];
  weights.forEach((w, i) => {
    const n = BigInt(total) * BigInt(w);
    shares.push(n / sum);
    rema.push({ r: n % sum, i });
  });
  let leftover = BigInt(total) - shares.reduce((a, s) => a + s, 0n);
  rema.sort((a, b) => (a.r === b.r ? a.i - b.i : a.r > b.r ? -1 : 1));
  for (const { i } of rema) {
    if (leftover === 0n) break;
    shares[i] = (shares[i] ?? 0n) + 1n;
    leftover -= 1n;
  }
  return shares.map(cents);
}

export function taxFor(amount: Money, rateBp: number, inclusive: boolean): Money {
  if (rateBp === 0 || amount === 0) return 0;
  return inclusive ? mulDivRound(amount, rateBp, 10_000 + rateBp) : mulDivRound(amount, rateBp, 10_000);
}

const safe = (n: number) => Number.isSafeInteger(n);

export function priceLines(
  lines: PreviewLine[],
  billDiscount: Money,
  taxInclusive: boolean,
): { ok: true; order: PricedOrder } | { ok: false; error: PricingError; line?: number } {
  if (lines.length === 0) return { ok: false, error: "no_lines" };
  const bases: Money[] = [];
  for (const [i, l] of lines.entries()) {
    if (!safe(l.qty) || l.qty <= 0) return { ok: false, error: "qty", line: i };
    if (!safe(l.unit_price) || l.unit_price < 0) return { ok: false, error: "price", line: i };
    if (!(l.tax_rate_bp >= 0 && l.tax_rate_bp <= 10_000)) return { ok: false, error: "tax", line: i };
    const gross = l.unit_price * l.qty;
    if (!safe(gross)) return { ok: false, error: "overflow", line: i };
    if (l.line_discount < 0 || l.line_discount > gross) return { ok: false, error: "line_discount", line: i };
    bases.push(gross - l.line_discount);
  }
  const baseSum = bases.reduce((a, b) => a + b, 0);
  if (!safe(baseSum)) return { ok: false, error: "overflow" };
  if (billDiscount < 0 || billDiscount > baseSum) return { ok: false, error: "bill_discount" };
  const shares = apportion(billDiscount, bases);
  if (shares === null) return { ok: false, error: "bill_discount" };

  const priced: PricedLine[] = lines.map((l, i) => {
    const base = bases[i] ?? 0;
    const share = shares[i] ?? 0;
    const lineTotal = base - share;
    const tax = taxFor(lineTotal, l.tax_rate_bp, taxInclusive);
    return {
      line_gross: base + l.line_discount,
      line_discount: l.line_discount,
      order_discount_share: share,
      line_total: lineTotal,
      tax_amount: tax,
      line_net: taxInclusive ? lineTotal - tax : lineTotal,
    };
  });
  const sum = (f: (p: PricedLine) => number) => priced.reduce((a, p) => a + f(p), 0);
  const taxTotal = sum((p) => p.tax_amount);
  const linesTotal = sum((p) => p.line_total);
  return {
    ok: true,
    order: {
      lines: priced,
      gross_sales: sum((p) => p.line_gross),
      discount_total: sum((p) => p.line_discount + p.order_discount_share),
      tax_total: taxTotal,
      net_sales: sum((p) => p.line_net),
      grand_total: taxInclusive ? linesTotal : linesTotal + taxTotal,
    },
  };
}

/** True when discount / gross ≤ limit_bp / 10000 (mirrors `discount::within_limit`). */
export function withinLimit(discount: Money, gross: Money, limitBp: number): boolean {
  if (discount <= 0) return true;
  if (gross <= 0) return false;
  return BigInt(discount) * 10_000n <= BigInt(limitBp) * BigInt(gross);
}
