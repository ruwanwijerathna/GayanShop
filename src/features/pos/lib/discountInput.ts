// A typed discount: an amount ("150", "99.50") or a percentage of `base` ("5%", "2.5%"). Percentages round
// DOWN to the cent, so "5%" never lands one cent above the 5% cashier limit (doc §3.7).
import { parseMajor, type Money } from "@/lib/money";

export function parseDiscount(input: string, base: Money): Money | null {
  const s = input.trim();
  if (s === "") return 0;
  const pct = /^(\d{1,3})(?:\.(\d{1,2}))?\s*%$/.exec(s);
  if (pct) {
    const hundredths =
      Number.parseInt(pct[1] ?? "0", 10) * 100 + Number.parseInt((pct[2] ?? "").padEnd(2, "0") || "0", 10);
    if (hundredths > 10_000) return null;
    return Math.floor((base * hundredths) / 10_000);
  }
  return parseMajor(s);
}
