import { describe, expect, it } from "vitest";
import { keysForTables } from "./events";

describe("db://changed → query invalidation", () => {
  it("maps changed tables to query-key prefixes, once each", () => {
    expect(keysForTables(["products", "categories", "audit_logs"])).toEqual([
      ["catalog"],
      ["quickSales"],
      ["audit"],
    ]);
  });

  it("refreshes every screen that shows a table", () => {
    expect(keysForTables(["stock_movements", "batches"])).toEqual([
      ["catalog"],
      ["batches"],
      ["inventory"],
      ["reports"],
    ]);
  });

  it("refreshes customer figures after a sale or void", () => {
    expect(keysForTables(["orders"])).toEqual([
      ["orders"],
      ["quickSales"],
      ["customers"],
      ["reports"],
      ["staff"],
    ]);
  });

  it("ignores tables no screen shows yet", () => {
    expect(keysForTables(["sync_outbox", "sku_counters"])).toEqual([]);
  });
});
