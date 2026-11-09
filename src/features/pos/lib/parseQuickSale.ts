// The F2 Quick Sale grammar (doc §3.2):  [qty "*"] name price   e.g. "3* Bucket 10L 450" → 3 × "Bucket 10L" @ 450.00
// The price is the trailing token matching ^\d+(\.\d{1,2})?$, so sizes like "10L" never read as a price.
import type { Money } from "@/lib/money";
import { parseMajor } from "@/lib/money";

export const QUICK_SALE_NAME_MAX = 80;
const MAX_QTY = 9_999;

export type QuickSaleParse =
  | { ok: true; qty: number; name: string; unitPrice: Money }
  | { ok: false; reason: "empty" | "price_missing" | "name_missing" | "qty_invalid" | "name_too_long" };

export function parseQuickSale(input: string): QuickSaleParse {
  let rest = input.trim().replace(/\s+/g, " ");
  if (rest === "") return { ok: false, reason: "empty" };

  let qty = 1;
  const q = /^(\d+)\s*\*\s*(.*)$/.exec(rest);
  if (q) {
    qty = Number.parseInt(q[1] ?? "", 10);
    rest = (q[2] ?? "").trim();
    if (!Number.isSafeInteger(qty) || qty < 1 || qty > MAX_QTY) return { ok: false, reason: "qty_invalid" };
  }

  const tokens = rest.split(" ").filter((t) => t !== "");
  const last = tokens.at(-1) ?? "";
  if (!/^\d+(\.\d{1,2})?$/.test(last)) return { ok: false, reason: "price_missing" };
  const unitPrice = parseMajor(last);
  if (unitPrice === null) return { ok: false, reason: "price_missing" };

  const name = tokens.slice(0, -1).join(" ");
  if (name === "") return { ok: false, reason: "name_missing" };
  if ([...name].length > QUICK_SALE_NAME_MAX) return { ok: false, reason: "name_too_long" };
  return { ok: true, qty, name, unitPrice };
}
