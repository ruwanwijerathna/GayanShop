// Till preview of an exchange (doc §3.3, decision #26): an exact port of `domain::exchange::portion`. Returned
// goods are credited at what the customer actually paid, split over the returned units with cumulative
// flooring so that every unit of a line, over any number of exchanges, adds up to the line total exactly.
// Rust is authoritative; the seal trigger re-checks the same identity.
import type { ExchangeInput, ExchangeLine, ExchangeSource } from "@/db/bindings";
import type { Money } from "@/lib/money";

/** The part of `amount` that belongs to units `from..to` of a line of `qty` units. */
export function portion(amount: Money, qty: number, from: number, to: number): Money {
  if (qty <= 0 || from < 0 || to > qty || from >= to) return 0;
  const at = (n: number) => (BigInt(amount) * BigInt(n)) / BigInt(qty);
  return Number.parseInt((at(to) - at(from)).toString(), 10);
}

/** Units chosen per source line (`item_id` → qty); absent or 0 means not returned. */
export type ReturnPicks = Record<string, number>;

/** What a pick of `qty` units of `line` is worth: the units after those already returned in earlier exchanges. */
export function lineCredit(line: ExchangeLine, qty: number): Money {
  const q = Math.min(Math.max(0, Math.trunc(qty)), line.returnable_qty);
  return portion(line.line_total, line.qty, line.returned_qty, line.returned_qty + q);
}

export function totalCredit(source: ExchangeSource, picks: ReturnPicks): Money {
  return source.lines.reduce((a, l) => a + (l.eligible ? lineCredit(l, picks[l.item_id] ?? 0) : 0), 0);
}

export function pickedUnits(picks: ReturnPicks): number {
  return Object.values(picks).reduce((a, q) => a + q, 0);
}

/** The `checkout_order` return list, in the source's line order. */
export function toExchangeInput(source: ExchangeSource, picks: ReturnPicks): ExchangeInput {
  return {
    returns: source.lines
      .filter((l) => l.eligible && (picks[l.item_id] ?? 0) > 0)
      .map((l) => ({ item_id: l.item_id, qty: Math.min(picks[l.item_id] ?? 0, l.returnable_qty) })),
  };
}

/**
 * The customer pays the replacement total minus the credit, never a negative amount (no cash refunds, no credit
 * notes): `short` is true when the replacement is worth less than the returned goods.
 */
export function amountDue(replacementTotal: Money, credit: Money): { due: Money; short: boolean } {
  return replacementTotal < credit
    ? { due: 0, short: true }
    : { due: replacementTotal - credit, short: false };
}
