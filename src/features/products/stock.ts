import type { Product } from "@/db/bindings";

/** At or below this many units (and above 0) a tracked product counts as low stock. */
export const LOW_STOCK_MAX = 5;

export type StockLevel = "untracked" | "in_stock" | "low" | "out" | "oversold";

/** Where a product's on-hand count puts it. Untracked products have no stock to report. */
export function stockLevel(p: Pick<Product, "track_stock" | "on_hand">): StockLevel {
  if (!p.track_stock) return "untracked";
  if (p.on_hand < 0) return "oversold";
  if (p.on_hand === 0) return "out";
  if (p.on_hand <= LOW_STOCK_MAX) return "low";
  return "in_stock";
}

export type StockFilter = "all" | "low" | "out";

/** The quick filter next to the search: low = 1…5 units; out = none left, oversold included. */
export function matchesStockFilter(
  p: Pick<Product, "track_stock" | "on_hand">,
  filter: StockFilter,
): boolean {
  const level = stockLevel(p);
  if (filter === "low") return level === "low";
  if (filter === "out") return level === "out" || level === "oversold";
  return true;
}
