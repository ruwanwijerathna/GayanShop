import { describe, expect, it } from "vitest";
import { keysForTables } from "./events";

describe("db://changed → query invalidation", () => {
  it("maps changed tables to query-key prefixes, once each", () => {
    expect(keysForTables(["products", "categories", "audit_logs"])).toEqual([["catalog"], ["audit"]]);
  });

  it("ignores tables no screen shows yet", () => {
    expect(keysForTables(["sync_outbox", "sku_counters"])).toEqual([]);
  });
});
