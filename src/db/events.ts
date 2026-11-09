// `db://changed` (doc §1.4): Rust emits the tables each committed write touched — including, from Phase 3,
// sync pulls from other counters — and the UI invalidates the matching queries.
import { useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";

export interface DbChanged {
  tables: string[];
}

/** Query-key prefixes to refresh per changed table (an array of prefixes when several screens show it). */
const TABLE_KEYS: Record<string, readonly string[] | readonly (readonly string[])[]> = {
  products: [["catalog"], ["quickSales"]],
  categories: ["catalog"],
  audit_logs: ["audit"],
  batches: ["batches"],
  batch_items: ["batches"],
  stock_movements: [["batches"], ["inventory"]],
  cash_sessions: ["cash"],
  cash_movements: ["cash"],
  print_jobs: ["printer"],
  orders: [["orders"], ["quickSales"]],
  customers: ["customers"],
  staff: ["staff"],
  quick_sale_links: ["quickSales"],
};

export function keysForTables(tables: string[]): (readonly string[])[] {
  const keys = new Map<string, readonly string[]>();
  for (const table of tables) {
    const entry = TABLE_KEYS[table];
    if (!entry) continue;
    const prefixes = entry.every((k) => Array.isArray(k))
      ? (entry as readonly (readonly string[])[])
      : [entry as readonly string[]];
    for (const key of prefixes) keys.set(key.join("/"), key);
  }
  return [...keys.values()];
}

/** Subscribe once for the logged-in shell. Outside Tauri (tests, plain browser) there is no event bus. */
export function useDbChangedInvalidation() {
  const qc = useQueryClient();
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    listen<DbChanged>("db://changed", (event) => {
      for (const queryKey of keysForTables(event.payload.tables)) void qc.invalidateQueries({ queryKey });
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // Not running inside Tauri: mutations still invalidate their own queries.
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [qc]);
}
