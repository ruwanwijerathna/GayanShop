// Chart number formatting (doc §1.4): money through formatMoney (Latin digits, Rs prefix, both languages).
import type { ChartConfig } from "@/components/ui/chart";
import { formatMoney } from "@/lib/money";

/** Axis ticks in whole rupees: 1_234_550 → "Rs 12,346". */
export function axisMoney(cents: number): string {
  return formatMoney(Math.round(cents / 100) * 100).replace(/\.00$/, "");
}

/** `7_550` bp → "75.5%". */
export function percentBp(bp: number): string {
  const sign = bp < 0 ? "-" : "";
  const abs = Math.abs(bp);
  return `${sign}${Math.trunc(abs / 100)}.${Math.trunc((abs % 100) / 10)}%`;
}

/** Series colours come only from the theme's chart tokens, so light and dark both work. */
export function chartColor(n: 1 | 2 | 3 | 4 | 5): string {
  return `var(--chart-${n})`;
}

/** A chart config whose every colour is a theme token (asserted by the chart tests). */
export function tokenConfig(series: Record<string, { label: string; n: 1 | 2 | 3 | 4 | 5 }>): ChartConfig {
  return Object.fromEntries(
    Object.entries(series).map(([k, v]) => [k, { label: v.label, color: chartColor(v.n) }]),
  );
}

/** Bucket labels: `2026-11-08` → "08 Nov", `2026-11` → "Nov 2026" (dates stay Latin in both languages). */
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
export function bucketLabel(bucket: string): string {
  const month = MONTHS[Number.parseInt(bucket.slice(5, 7), 10) - 1] ?? bucket;
  return bucket.length === 7 ? `${month} ${bucket.slice(0, 4)}` : `${bucket.slice(8, 10)} ${month}`;
}

/** Tooltip values arrive as Recharts' loose value type; money is always integer cents from Rust. */
export function tooltipMoney(value: unknown): string {
  return typeof value === "number" && Number.isSafeInteger(value) ? formatMoney(value) : String(value);
}
