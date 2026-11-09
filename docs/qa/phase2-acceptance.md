# Phase 2 acceptance

> Block 2.10 gate (AGENT.md §5). Each item shows **how it was verified**:
> - **Perf**: `cargo test --manifest-path src-tauri/Cargo.toml --release --test perf_budgets -- --ignored`. It runs the full §13.4 table on the gate dataset and the headroom run at 1,000,000 order items.
> - **E2E**: `pnpm e2e`. WebdriverIO and `tauri-driver` drive the real release binary through WebView2. The Phase 2 suite (`tests/e2e/phase2.e2e.ts`) opens a copy of the §13.4 gate dataset, written by `src-tauri/tests/e2e_dataset.rs`.
> - **Automated**: a named Rust or Vitest test that runs in `pnpm verify`.
> - **Manual**: needs a person at the counter, or hardware this machine doesn't have: the label printer, the barcode scanner, the counter PC itself.
>
> Items marked **Manual — pending** are for the owner on the counter PC. None of them blocks the code. They qualify the hardware, the rolls and the people.

Environment: Windows 10 Pro 19045 dev machine; WebView2 157.0.4341.42; msedgedriver 157.0.4341.42; tauri-driver 2.1.0. The display runs at about 56.5 Hz (a 17.7 ms idle frame, measured by the suite).

## Running the suites

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --manifest-path src-tauri/Cargo.toml --release --test perf_budgets -- --ignored --nocapture --test-threads=1
pnpm e2e                    # writes the scale dataset, builds the release exe, runs phase1 + phase2
E2E_SKIP_BUILD=1 pnpm e2e   # reuse both when neither needs rebuilding
```

`pnpm e2e` writes the dataset **before** building the app. `cargo test --release` also rebuilds the package's binary into `target/release` without Tauri's `custom-protocol` feature, and that binary loads `localhost`. The first run of this block hit exactly that, and the config now builds in the safe order.

## 1. §13.4 database scale budget (p95, release build)

Gate dataset: 50,000 products, 20,000 customers, 33,307 orders and 100,000 order items over a year at three counters, with payments, sale slices, quick sales, voids, expenses, damage and audit rows. It's seeded through the sync pull path, so every derived table is maintained by the real triggers. The headroom run uses the same dataset at 1,000,000 order items (333,005 orders), and every row must stay within 2× of its budget.

| Query | Budget | Gate | Headroom (1M items) | Result |
|---|---|---|---|---|
| Barcode scan → product | < 2 ms | 0.20 ms | — | ✅ |
| SKU scan (barcode miss, then SKU) | < 2 ms | 0.30 ms | — | ✅ |
| Type-ahead `blu buc`, LIMIT 20 | < 15 ms | 1.5 ms | — | ✅ |
| Sinhala substring (trigram) | < 25 ms | 3.6 ms | — | ✅ |
| Phone → customer | < 2 ms | 0.09 ms | — | ✅ |
| Checkout (5 lines, FIFO, seal, audit, outbox, `FULL` fsync) | < 40 ms | 21.2 ms | 22.3 ms | ✅ |
| P&L: day / week / month / year | < 20 ms | 0.46 / 0.47 / 0.51 / 1.5 ms | 0.44 / 0.45 / 0.46 / 1.2 ms | ✅ |
| Order history page (500 rows, keyset; newest and deep) | < 20 ms | 5.6 ms | 4.6 ms | ✅ |
| Customer list sorted by spend | < 20 ms | 4.1 ms | 18.7 ms | ✅ |
| Quick-sale review by frequency | < 20 ms | 2.0 ms | 1.8 ms | ✅ |
| Batch report / valuation (30-item batch) | < 30 ms | 17.3 ms | 12.2 ms | ✅ |
| Audit log page (200) | < 20 ms | 3.1 ms | 2.3 ms | ✅ |
| *Store-wide valuation, 50,000 layers (on demand, not a §13.4 row)* | *guard < 250 ms* | *191 ms* | *144 ms* | ✅ |

Plan assertions run on every `pnpm verify` (`perf_budgets.rs › report_order_stock_and_audit_queries_use_their_indexes`):
- Order history walks `ix_orders_keyset`, and the audit page walks `ix_audit_keyset`.
- A day's write-offs use `ix_mov_writeoffs`, and a day's expenses use `ix_expenses_bdate`.
- Sales and payments come from the rollups only. None of these queries fully scans `orders`, `order_items`, `stock_movements` or `audit_logs`.

The first measurements found three real defects, all fixed in this block:
- **Yearly P&L at 1M items took 166 ms.** `ix_mov_date_reason` walked a year of sale slices to find the few write-offs. Migration `0002_report_indexes.sql` adds the partial index `ix_mov_writeoffs` (write-off reasons only). The P&L also now uses plain `BETWEEN` bounds, with sentinels for open ends, instead of `?1 IS NULL OR …`. The yearly P&L now takes 1.2 ms.
- **Order history sorted every order.** `store_id = ?` led SQLite to `ux_orders_receipt` and a temp B-tree sort. `+o.store_id` keeps the keyset index in charge.
- **Store-wide valuation took 500 ms, and the customer list 54 ms at 1M.** The valuation now aggregates by batch and category in SQL instead of shipping 50,000 layers to Rust. The unfiltered spend and last-visit customer lists walk `customer_stats`' indexes.

## 2. Doc §8 Phase 2 exit criteria

| # | Exit criterion | Verified by | Result |
|---|---|---|---|
| 2.1 | The owner enters a 30-SKU batch, allocates costs, receives it, and prints a full roll of 2-up labels and a row of 3-up labels in under 10 minutes | Automated: `batches.rs` (allocation methods, variance, receive, reopen, late items, close), `labels.rs` (golden TSPL/ZPL bytes for both templates, barcode fit), `IntakeWizard` and `PrintLabelsDialog` Vitest. **Manual — pending:** the timed 30-SKU run on the counter PC with the real label printer and rolls. | ☐ |
| 2.2 | Every printed barcode scans back to the right product, including the 6-digit Code128 on 1″×1″ | Automated: `labels.rs` (encoder round trips, quiet zones, module ≥ 2 dots, warnings when a code can't fit). **Manual — pending:** scan every label of the run above with the counter's scanner, including the 1″×1″ row. | ☐ |
| 2.3 | After scripted sales, voids and damages, the batch report reconciles to the cent (landed cost = Σ allocated + variance; realised profit matches a hand calculation) | Automated: `batches.rs › the P&L reconciles…`, `stock_ops.rs`, `reports.rs › the P&L identities hold…` (variance on its received date; COGS = slices + variance). | ✅ |
| 2.4 | The monthly P&L equals the sum of its daily reports; rollups equal raw sums after `rebuild_derived`; the charts match the tables | Automated: `reports.rs`. On seeded random data, month == Σ days, year == Σ months, all time == Σ years, week and custom == Σ days, rollups == item and payment sums, and a rebuild reproduces every derived table after corruption. `charts.test.tsx`: every chart carries the same numbers as a table, in EN and SI. E2E: Reports → Year shows the P&L and its charts; Settings → Rebuild reports completes on the 100k-item store. | ✅ 2026-11-09 |
| 2.5 | Scrolling 100k orders and 50k products stays at 60 fps on the counter PC | E2E on this machine: 50,000 products (median 17.6 ms, p95 18.2 ms against a 17.7 ms display frame) and the year's orders while keyset pages load (median 17.6 ms, p95 18.2 ms). Scrolling keeps the display's own rate with no dropped frames. **Manual — pending:** rerun `pnpm e2e` on the counter PC; that's the machine the criterion names. | ✅ dev / ☐ counter |
| 2.6 | A frequent quick-sale item can be promoted in one click and is scannable afterwards | Automated: `quick_sales.rs` (promotion with shelf stock, scannable barcode at once, offline-stable link id), `QuickSalesScreen.test.tsx`, `PosScreen.test.tsx` (the till's "now a product" hint). E2E: Quick sales → open the top key → Promote → "… is now a product." | ✅ 2026-11-09 |
| 2.7 | A customer's history and total spend match their receipts | Automated: `customer_backoffice.rs` (visits and spend exclude voids; history pages; the receipt drill-down masks the phone), `export.rs`. E2E: Customers → search `0710000042` → the profile and its purchase history. | ✅ 2026-11-09 |

## 3. Phase 2 features at a glance

| Area | Verified by | Result |
|---|---|---|
| Customer back office: list, profile, history, analytics, PDPA erasure | `customer_backoffice.rs` (8), `CustomersScreen.test.tsx` | ✅ |
| Expenses with drawer pay-out (Z-report reconciles; append-only cash) | `expenses.rs` (7), `ExpensesScreen.test.tsx`; E2E records an expense | ✅ |
| Reports for every §5 period, counter filter, comparison, sales by product, category and batch, voids | `reports.rs` (6), `ReportsScreen.test.tsx` | ✅ |
| Charts and the dashboard, light and dark, EN and SI | `charts.test.tsx` (token-only colours, both languages), `Dashboard.test.tsx`; E2E dashboard | ✅ |
| Keyset DataTables and CSV export (`DATA_EXPORT`, audited, Excel-safe) | `export.rs` (4), `OrdersScreen.test.tsx`, `InventoryScreen.test.tsx` | ✅ |
| **Manual — pending:** open a CSV export in Excel on the counter PC: Sinhala names readable, amounts as numbers, a name starting with `=` shown as text | — | ☐ |
| **Manual — pending:** the Phase 2 screens by touch only at 1024×768, and in Sinhala, with a native speaker reviewing the new strings (customers, expenses, reports, charts, orders) | — | ☐ |
