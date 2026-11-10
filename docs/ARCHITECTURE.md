# GayanShop POS — Master Architecture Plan

> Status: **v4 (2026-11-10). Phases 0–4 complete and merged to main. Production NSIS installer built.** v2 added:
> - **Batches** (cost layers), replacing "lots"
> - Full financial reporting
> - Role-based access control (RBAC) with supervisor approvals
> - A hash-chained audit trail
> - Quick-sale history and promotion
> - Cash sessions and Z-reports
>
> v3 locked in the owner's policies:
> - **5% cashier discount limit**, for line and bill discounts, with bill discounts apportioned pro-rata (§3.7).
> - LKR, Asia/Colombo, a 04:00 day cutoff, Monday week start, prices tax-inclusive at 0% tax.
>
> v4 (owner simplification + operational features):
> - **All sales are final.** There are no refunds and no exchanges. The only correction is a **same-session void** with supervisor approval (§3.3). Exchanges and credit notes move to Phase 5.
> - **Dual-language UI**, English and Sinhala (සිංහල), with receipts staying English-first (§1.8).
> - **Label layouts:** 2-up 2″×1″ and 3-up 1″×1″ rolls on TSPL/ZPL printers (§7.4).
> - **Optional customer phone at the till**, with purchase history and spend analytics (§3.8).
> - **Manual "Backup Database Now"** to USB or any folder via `VACUUM INTO` (§3.9).
> - **UI stack:** TanStack Table v8 + TanStack Virtual, shadcn/ui Charts (Recharts), and responsive touch layouts from 1024×768 to 1080p with 48 px targets (§1.4).
>
> This is the founding design document. Change it via PR when a decision changes. **The SQL in §4.3 is the target for `src-tauri/migrations/0001_init.sql`.** It hasn't been run yet, and Phase 0 must prove it with a migration and trigger test.
>
> ⚠️ **Files already on disk from the paused Phase 0 start are pre-v4 drafts.** They're kept at the owner's request. `src-tauri/migrations/0001_init.sql` and `src-tauri/src/domain/{exchange,permissions}.rs` still contain exchange and credit-note code. When Phase 0 resumes, regenerate the migration from §4.3 and drop or rework those modules first.

---

## 0. Executive summary

| Concern | Decision |
|---|---|
| Desktop runtime | Tauri v2 (Rust core, WebView2 on Windows) |
| UI | React 19 + TypeScript + Vite, Tailwind CSS v4, shadcn/ui. Lists use TanStack Table v8 + TanStack Virtual, charts use shadcn/ui Charts (Recharts), and i18n uses react-i18next (English and Sinhala). Touch layouts support 1024×768 → 1920×1080. |
| Local DB | **SQLite (WAL mode), owned by Rust via `sqlx`**. The webview never touches SQL. |
| Cloud DB | **Supabase (Postgres + Auth + RLS)** |
| Sync | **A custom transactional outbox plus a Rust background sync worker.** Push is idempotent and pull uses a server sequence cursor. |
| Conflict model | Orders, stock, cash and audit records are append-only (no conflicts). Master data uses last-write-wins per row. |
| Inventory costing | **Batches made of cost layers (`batch_items`), consumed oldest-batch-first (FIFO) at checkout.** Every unit of stock has a known cost. |
| Access control | Roles with permission bitmasks, plus per-user grant and deny overrides. Rust enforces it. Supervisor PIN approval uses single-use tokens. |
| Accountability | `audit_logs` is append-only, hash-chained per device, and written in the same transaction as the action it records. |
| Money | Integer minor units (`i64`, e.g. cents). Never floats. |
| IDs | UUIDv7, generated on the device |
| Hardware | Scanner: HID keyboard wedge, read in the frontend. Receipts: ESC/POS over raw spooler, network or serial, sent from Rust. The cash drawer sits on the printer's RJ11 port (`ESC p`). Labels: TSPL/ZPL sent raw from Rust, in **2-up 2″×1″** and **3-up 1″×1″** layouts. |
| Terminals | 1 store with 2–3 counters, each with its own SQLite DB. Every terminal holds the **full store history** for store-wide reports and customer history at any counter. |
| Sales policy | **All sales are final: no refunds, no exchanges.** The only correction is a **same-session void** (`VOID_ORDER`, supervisor approval) for an immediate cashier mistake, before the cash session closes. |
| Customers | An optional phone number at checkout (numpad shortcut) links the sale to a customer profile, with purchase history and total spend in the back office. |
| Backups | **Manual "Backup Database Now"** (Owner/Manager) to a USB drive or any folder, via `VACUUM INTO`, without pausing the till. Scheduled automatic backups follow in Phase 4. |

The ideas the rest of the design depends on:

1. **The local SQLite file is the source of truth for the till.** The cloud is a replica and aggregation point. A sale must never wait on the network.
2. **Never sync a mutable quantity.** Stock is a ledger of movements. Two offline terminals each selling the last pair of shoes produce two `-1` rows, which merge correctly.
3. **Batches are cost layers.** The same product can arrive in several batches at different costs. Each `(batch, product)` pair is a layer with its own allocated unit cost. Sales draw from layers, so "what did this batch cost, sell, earn and still hold?" is a simple aggregate.
4. **History is immutable.** Order lines snapshot name, price, cost and tax. Corrections are new rows: voids (same session only) and stock adjustments. Nothing financial is ever edited in place.
5. **Permissions are checked in Rust, and every sensitive action leaves an audit row in the same transaction.** The UI hides what you can't do, but Rust is what actually stops it.

---

## 1. Architectural blueprint

### 1.1 Local database: SQLite in Rust (chosen) vs. the alternatives

| Option | Verdict | Why |
|---|---|---|
| **SQLite via `sqlx` in Rust** | ✅ **Chosen** | It's a real file on disk with ACID transactions, and WAL survives power loss mid-sale. You can back it up with `VACUUM INTO`. It's queryable for reports. Migrations are versioned. Business rules, RBAC and audit live in one place (Rust), which is the only process that can write. |
| `tauri-plugin-sql` (SQL from the webview) | ❌ | Puts raw SQL, business logic *and permission checks* in JS, which any XSS bug can reach. RBAC enforced in the webview is not RBAC. |
| RxDB / IndexedDB / PouchDB | ❌ | Storage lives inside the WebView2 profile, so it's fragile. It's weak at relational reporting, and the document model fights ledgers. |
| PowerSync / ElectricSQL | ⏸ Not now | Their client SDKs target JS-owned SQLite. Re-evaluate if we ever ship a web or mobile client. |

**Why `sqlx` rather than `rusqlite`:** it's async-native (fits Tauri's tokio runtime), ships a migration runner (`sqlx::migrate!`) and offers optional compile-time-checked queries. `rusqlite` + `refinery` is an acceptable fallback.

SQLite settings, applied on every connection:
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;      -- not NORMAL: see the note below (power-loss durability)
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -65536;     -- 64 MiB page cache
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 268435456;   -- 256 MiB memory-mapped reads
-- on pool close / daily: PRAGMA optimize;   after bootstrap or bulk import: ANALYZE;
```

**Why `synchronous = FULL` (stress-test finding, §13.2).**
- Under WAL, `NORMAL` survives an *application* crash. On a *power cut or OS crash*, though, it can lose the most recent committed transactions.
- For a till that's unacceptable. The receipt is printed after the commit, so a lost commit means a customer holding a receipt for a sale that no longer exists.
- `FULL` fsyncs the WAL on every commit. That costs roughly 1–5 ms per commit on an SSD, which is negligible at counter speed. The database is never corrupted in either mode; `FULL` is about not *losing* committed sales.
- On startup, Rust runs `PRAGMA quick_check` in the background before enabling backups. A full `integrity_check` runs on every backup (§3.9).

DB file location: `%APPDATA%\com.gayanshop.pos\pos.db` (Tauri `app_data_dir()`). Manual backups go to a folder the user picks (§3.9). Automatic backups (Phase 4) go to `...\backups\`.

### 1.2 Cloud database: Supabase (chosen)

| Option | Verdict | Why |
|---|---|---|
| **Supabase** | ✅ **Chosen** | Postgres handles cross-terminal reporting. It bundles Auth (device identities) and RLS for store isolation, and RLS can also make audit rows insert-only. RPC functions give us atomic, idempotent sync endpoints. There's a later path to an owner web dashboard. |
| CouchDB | ❌ | Its document model and revision-tree conflicts are a poor fit for ledgers. You'd also run it yourself. |
| Firebase / Firestore | ❌ | No SQL reporting, and the offline cache is JS-side. |
| Self-hosted Postgres + API | ⏸ | Our protocol is plain HTTP + JSON, so this is a backend swap later, not a rewrite. |

### 1.3 Sync design (summary; full protocol in §6)

```
┌──────────────── Desktop (per terminal, ×2–3) ───────────┐          ┌──────── Supabase ────────┐
│ React UI ──invoke()──► Rust command                      │          │                          │
│                          │ guard: RBAC / approval token  │          │                          │
│                          │ single SQLite transaction:    │  HTTPS   │  rpc.sync_push(batch)    │
│                          │  1. write business rows       │ ───────► │   advisory lock/store,   │
│                          │  2. write audit_logs row      │          │   idempotent upserts,    │
│                          │  3. append to sync_outbox     │          │   assigns server_seq     │
│                          ▼                               │          │                          │
│                     pos.db (WAL)                         │ ◄─────── │  rpc.sync_pull(cursor)   │
│                          ▲                               │          │   rows where server_seq  │
│      Sync worker (tokio task): reachability probe ─►     │          │   > cursor               │
│      push outbox ─► pull changes ─► write sync_logs      │          │  RLS: store_id = jwt.sid │
└──────────────────────────────────────────────────────────┘          └──────────────────────────┘
```

- **The worker** (`sync::worker`, Block 3.4) runs one round per wake-up: the health probe, then push batches until nothing is ready, then pull pages until caught up. It never holds the writer lock across a network call. Its states, for the pill: unregistered, bootstrap pending, syncing, synced, pending N (queued or dead ops), offline, unauthorized (re-register the counter), error. In-flight ops left by a crash go back to the queue at start.
- **Connectivity detection:** don't trust `navigator.onLine`. The worker sends a health RPC every 15 s when idle, and runs immediately after any local write. Failures back off exponentially (5 s → 5 min max, with jitter). A `sync://status` event drives the UI's status pill (🟢 synced / 🟡 pending N / 🔴 offline).
- **The business write, audit row and outbox entry go in one transaction.** It's impossible to record a sale, or an override, without it being audited and queued for sync.
- **Never ship the Supabase `service_role` key in the binary.** Each terminal has its own device identity: a Supabase Auth user with `app_metadata.store_id` / `device_id`. Its refresh token is kept in the Windows Credential Manager (`keyring` crate). All tables are protected by RLS on `store_id`.
- **Staff identities are not Supabase users.** Staff log in to a *terminal* with a PIN, verified locally (§3.5). The device identity is what authenticates to the cloud. The `staff_id` on each row records who did it.

### 1.4 Frontend architecture

**State model**
- **Server-ish state** (products, batches, orders, customers, reports): TanStack Query, where each `queryFn` is a typed Tauri `invoke`. Rust emits `db://changed { tables: [...] }` after writes, including sync pulls, and the frontend invalidates the matching query keys.
- **Session state** (logged-in staff, effective permissions, UI language): a `session://changed` event from Rust. The UI keeps a read-only mirror for hiding controls. **The mirror is never trusted for enforcement.**
- **Ephemeral UI state** (cart, dialogs, scanner buffer): Zustand. The in-progress cart is written to `held_carts` under a reserved `__current__` row with a 500 ms debounce, so it survives a crash or power cut. "Hold sale" writes a named row.
- **Forms:** react-hook-form + zod. **Routing:** TanStack Router. The POS screen is the default route and is keyboard-first.
- **Everything is bundled and offline.** No CDN fonts, scripts or icons. The till must render identically with no internet.

**Data tables: TanStack Table v8 + TanStack Virtual**

Every back-office list is one shared `components/data-table/` built on `@tanstack/react-table` **v8**, the version the shadcn data-table pattern targets. v9 exists, so pin `^8` and revisit later. Rows render through `@tanstack/react-virtual`, which keeps scrolling at 60 fps whatever the row count. That covers Products, Orders, Batches, Batch items, Customers, Audit log, Quick-sale review and Stock movements.

| List size | Strategy |
|---|---|
| Bounded (products ≤ ~50k, batches, staff) | Load once through TanStack Query, then filter, sort and virtualise client-side (`getFilteredRowModel` / `getSortedRowModel`). |
| Unbounded (orders, audit log, stock movements, customer history) | **Server-side keyset pagination**: Rust commands take `{ cursor, limit ≤ 500, filters, sort }` and return `{ rows, next_cursor }`. The table uses `useInfiniteQuery` and fetches the next page when the virtualiser nears the end. `manualSorting` / `manualFiltering` are on, and sort keys map to indexed columns only. |

- Fixed row heights: 48 px on touch layouts and 36 px in compact back-office density. That keeps virtualisation exact.
- Column visibility and widths are saved per user in `device_settings`.
- **Block 2.9 implementation notes:**
  - **Lists on the shared virtualised DataTable:** Products, Orders, Batches, Batch items, Customers, Audit log, Quick-sale review, Stock movements and Expenses.
  - **Keyset paging (`useInfiniteQuery` + `onEndReached`):**
    - Orders: `order_history` over `ix_orders_keyset`, filtered by period, counter, status and receipt-number prefix, with a receipt drill-down.
    - The audit log, stock movements and customer purchase history.
  - **CSV export** (`export.rs`, `DATA_EXPORT`, audited):
    - **What:** orders, order lines, products, customers, expenses, stock movements and sales by product, for the chosen §5 period where one applies.
    - **Permissions:** each kind also needs the permission of the screen that shows it (`REPORT_SALES`, `CUSTOMER_VIEW`, `EXPENSE_MANAGE` or `REPORT_FINANCIAL`), and cost columns appear only with `VIEW_COST`.
    - **Format:** UTF-8 with a BOM (so Excel shows Sinhala) and CRLF line ends. Quoting follows RFC 4180, and money is written as plain decimals from integer cents.
    - **Formula guard:** text cells starting with `= + - @`, tab or CR get a leading `'`.
    - **Writing:** the file is written to `.partial`, flushed and renamed, then one `DATA_EXPORT` audit row records the kind, the row count, the period and the file name, never the data.

**Analytics charts: shadcn/ui Chart (Recharts)**

Charts use the official shadcn `chart` component (`ChartContainer`, `ChartTooltip`, `ChartLegend`) over Recharts. Pin the Recharts major that the shadcn chart component targets at scaffold time.

| Chart | Type | Data source |
|---|---|---|
| Revenue vs expenses | Grouped bar, by day, week or month | `daily_sales_summary.sales_net` vs `expenses` (§5) |
| Sales trend | Area, with an optional previous-period overlay | `daily_sales_summary` |
| Payment methods | Donut | `daily_payment_summary` |
| Customer spend (top N) | Horizontal bar | `customer_stats` (derived, §3.8) |
| Batch sell-through | Bar | `batch_item_stock` |

- Colours come from the shadcn CSS chart tokens (`--chart-1…5`), so light and dark themes both work.
- Money axes use `formatMoney` (Latin digits, `Rs.` prefix) in both languages.
- Charts only receive pre-aggregated rows from Rust, never raw orders.
- **Block 2.8 implementation notes:**
  - **Library.** `components/ui/chart.tsx` is the shadcn chart component for Recharts **3.8.0**, pinned exactly. Its registry `cn` import is repointed to `@/lib/utils`.
  - **Charts.** `components/charts/` holds `SalesTrendArea`, `RevenueExpenseBar`, `PaymentDonut`, `TopCustomersBar` and `BatchSellThroughBar`.
  - **Accessibility.** Each chart renders inside a `ChartFigure`: a titled `<figure>` plus a visually hidden table of the same numbers, in the UI language. Screen readers and the tests read that table.
  - **Theming.** Colours are only the theme's `--chart-1…5` tokens, so light and dark need no extra work.
  - **Trend data.** `report_trend` returns a densified series for bounded periods (every day, week or month, empty ones as zeros) and, on request, the previous period. The previous-period overlay therefore lines up point for point.
  - **Bucketing.** The Reports screen picks day buckets for up to about two months, then weeks up to a year, then months.
  - **Sell-through.** `report_batch_sell_through` (units only, `REPORT_SALES`) feeds the sell-through bars.
  - **Dashboard.** Home shows a dashboard to `REPORT_SALES` holders: today against yesterday, the last 30 days against the 30 before, today's payment methods, and this year by month. Holders of `CUSTOMER_VIEW` also see this month's top customers.

**Responsive, touch-first layout**

| Target | Layout |
|---|---|
| **1024×768** (baseline POS touchscreen) | Till: cart on the left (~58%), with search, quick-keys and keypad on the right. The back office uses a collapsible icon sidebar and single-column forms. |
| **1366×768** | Till as above with wider product names. The back office gets an expanded sidebar and two-column forms. |
| **1920×1080** | Till gets an extra quick-keys panel. The back office gets dashboard grids up to 3 columns and side-by-side table and detail views. |

- **Minimum 48×48 px touch target** for anything tappable on the till and in dialogs. Add a `size="pos"` variant to Button (`h-12 min-w-12`) and lists with 48 px rows. No hover-only interactions. Inputs, selects and the numeric keypad are 48 px tall.
- Use Tailwind breakpoints (`lg` = 1024, `xl` = 1280, `2xl` = 1536, plus a custom `fhd` = 1920). The window minimum is 1024×768 (`tauri.conf.json` `minWidth` / `minHeight`).
- **Every touch action has a keyboard shortcut**, shown as `<Kbd>` hints on the buttons:

| Key | Action |
|---|---|
| F1 | Help / shortcut sheet |
| F2 | Quick sale bar |
| F3 | Product search |
| **F4 or Numpad `/`** | **Customer phone** (§3.8) |
| F6 | Line discount |
| F7 | Bill discount |
| F8 | Hold sale |
| **F9 or Numpad `+`** | **Pay** (owner-specified) |
| F10 | Void (opens the same-session list) |
| F11 | Recall held sale |
| Esc | Cancel or close |
| Ctrl+L | Lock |
| Ctrl+Shift+L | Toggle language |

### 1.5 Type sharing (single source of truth)

**Rust structs are the source of truth.** TS types are generated with **`tauri-specta` + `specta`**. Verify compatibility with your Tauri 2.x minor at scaffold time, and fall back to `ts-rs` plus a thin `invoke` wrapper if it lags.
- The matching versions are `tauri-specta =2.0.0-rc.26`, `specta =2.0.0-rc.26` and `specta-typescript 0.0.13` (checked 2026-11-08). Release candidates need exact `=` pins.
- `specta-typescript` 0.0.13 **refuses to export `i64`**, and it no longer has a global `BigIntExportBehavior` switch. Mark each IPC `i64` field with `#[specta(type = specta_typescript::Number)]`. That makes each precision decision explicit, and it's safe here because money is far below 2^53.
- Generate `src/db/bindings.ts` from a `cargo test`, not only at app start, so CI can regenerate types without a GUI.
- Permission bits are kept below 2^53 for the same reason (§3.5).

The TypeScript in §4 is the **target shape** of the generated `bindings.ts`. The SQLite DDL lives in `src-tauri/migrations/` and the Postgres DDL in `supabase/migrations/`.

### 1.6 Versions

Checked on 2026-11-08 (npm). **Re-verify at scaffold time:** `@tauri-apps/cli` / `@tauri-apps/api` 2.12.x · `react` 19.x · `vite` 8.x · `tailwindcss` 4.x · `shadcn` CLI 4.x · `zod` 4.x · `@tanstack/react-query` 5.x · `zustand` 5.x · `i18next` 27.x · `react-i18next` 18.x · `@tanstack/react-table` **8.x** (latest is 9.x; v8 is pinned on purpose) · `@tanstack/react-virtual` 3.x · `recharts` (the major the shadcn chart component targets; latest is 3.x) · `@tauri-apps/plugin-dialog` 2.x · `@fontsource/noto-sans-sinhala` 5.x. Pin Rust crates with `cargo add`. The toolchain is installed (rustc and cargo 1.99.1, MSVC), but `%USERPROFILE%\.cargo\bin` isn't on the Git Bash PATH by default (§9).

### 1.7 Business date

Every financial and stock row stores a **`business_date`** (`YYYY-MM-DD`). The originating device computes it at write time from these store settings:
- `store.timezone`, e.g. `Asia/Colombo`
- `store.day_cutoff`, e.g. `04:00`. A sale at 01:30 belongs to the previous business day.

It's never derived from UTC at query time. Otherwise terminals, and the cloud, would disagree about which day a late sale belongs to. All period reports filter on `business_date`.

### 1.8 Internationalisation: English and Sinhala (සිංහල)

- **Library:** `i18next` + `react-i18next`, with JSON dictionaries bundled at build time. There's no HTTP backend, so it works offline.
  - Layout: `src/i18n/locales/{en,si}/{common,pos,products,batches,reports,staff,settings,errors}.json`, one namespace per feature.
  - English (`en`) is the source and fallback language.
- **Switching:**
  - A language toggle (EN | සිං) sits in the top bar of the till and the back office, with shortcut Ctrl+Shift+L.
  - The choice is saved **per staff member** in `staff.ui_language` and applied at login. Before login, the lock screen uses the device default (`device_settings` `ui.default_language`, seeded `en`). The **sign-in form is English-only** (owner decision, 2026-11-10): it shows a Username field matched trimmed and case-insensitively against active staff `display_name`, plus a masked Password (PIN) field, whatever the device language is. A lone staff member's name is prefilled, and a name shared by two staff is refused rather than guessed. Its en/si keys still exist for parity.
  - Switching is instant, with no reload (`i18n.changeLanguage`).
- **Fonts:** bundle **Noto Sans Sinhala** locally (`@fontsource/noto-sans-sinhala`) alongside the Latin UI font.
  - Use `:lang(si)` rules for a slightly larger line-height and size, because Sinhala glyphs need more vertical room.
  - Layouts must tolerate ~30% text expansion. Buttons wrap or truncate with a tooltip, never overflow.
- **What gets translated:**

  | Translated | Not translated |
  |---|---|
  | UI chrome, labels, buttons, validation messages, empty states, report and column titles | Data the user typed: product names, customer names, notes |
  | Error messages: Rust returns stable error **codes** (`AppError.kind` plus fields) and the UI maps them to `errors.<KIND>` keys. Rust never produces display strings. | Audit `action` codes. They're stored as codes and shown translated through `audit.actions.<CODE>`. |
- **Numbers, money and dates:**
  - Money is always **Latin digits with `Rs.`** (e.g. `Rs. 1,250.00`) in both languages. This matches receipts, labels and cashier habit.
  - Dates use `Intl.DateTimeFormat` with `si-LK` or `en-LK`, keeping the numeric `YYYY-MM-DD` form in tables.
- **Receipts and labels stay English.** Generic ESC/POS and TSPL printers have no Sinhala code page, so receipt text, the footer and label text are English or Latin. This is an owner requirement.
  - Product `name` must be printable: Rust warns at save time if it contains characters outside the printer code page.
  - An optional `products.name_si` is shown on screens when the UI is in Sinhala, and is never printed.
  - **Owner decision (2026-11-10): one product name.** The product form has a single "Product Name" field and saves `name_si = NULL`, so the same English name shows in the product list, at the till and on receipts and labels. The column, its FTS index and the sync field stay (older rows, other counters). An existing `name_si` is dropped the next time that product is saved.
  - Raster (bitmap) Sinhala printing stays in the Phase 5 backlog.
- **Quality gates:**
  - A CI script fails the build when a key exists in `en` but not `si`, or vice versa.
  - A key missing at runtime falls back to English (`fallbackLng: "en"`). Rust error codes without a specific translation fall back to their kind's generic message (`errors.<KIND>._`), via `translateError` in `src/i18n`.
  - The till screens are fully translated before the pilot.

### 1.9 UI/UX design system: counter-grade, high-contrast, dual-input

**Principles, in priority order:**
1. **Legible under glare.** Fluorescent store lighting and screens at an angle wash out low-contrast UI.
2. **Fast by hand and by keyboard.** Every action has both a ≥ 48 px touch path and a keyboard path.
3. **Never block the next scan.**
4. **Calm by default, loud only for problems.**

Style: flat with clear borders, no decorative effects. Borders, not shadows, separate regions, because shadows vanish under glare. There are no gradients or glassmorphism, and icons are Lucide SVGs, never emoji.

**Colour tokens** (Tailwind v4 `@theme` + shadcn CSS variables; components use tokens only, never raw hex):

| Token | Light | Dark | Use |
|---|---|---|---|
| `--background` | white `#FFFFFF` | zinc-950 `#09090B` | App background |
| `--foreground` | zinc-950 `#09090B` (19.9:1) | zinc-50 `#FAFAFA` (19.1:1) | Body text |
| `--card` / `--popover` | white | zinc-900 `#18181B` | Panels, dialogs |
| `--muted` | zinc-100 `#F4F4F5` | zinc-800 `#27272A` | Secondary surfaces, zebra rows |
| `--muted-foreground` | zinc-600 `#52525B` (7.7:1) | zinc-400 `#A1A1AA` (7.8:1 on zinc-950) | Secondary text. Never lighter than this. |
| `--border` | zinc-300 `#D4D4D8` (1.5:1) | zinc-700 `#3F3F46` (1.9:1) | **Decorative** dividers only (table rules, panel separators), which carry no meaning on their own |
| `--input` | zinc-500 `#71717A` (4.8:1) | zinc-500 `#71717A` (4.1:1 on zinc-950, 3.7:1 on zinc-900) | **Boundaries of controls**: inputs, selects, outline buttons, checkboxes. These must meet ≥ 3:1 (WCAG 1.4.11). |
| `--primary` (**emerald**) | emerald-700 `#047857`, white text (5.5:1) | emerald-500 `#10B981`, zinc-950 text (7.8:1) | **Pay / Complete**, success states, "Sale complete ✓" |
| `--success-surface` | emerald-50 `#ECFDF5` + emerald-800 text | emerald-950 `#022C22` + emerald-300 text | Completed-transaction banner, row flash |
| `--destructive` | red-700 `#B91C1C`, white text (6.5:1) | red-500 `#EF4444`, zinc-950 text (5.3:1) | Void, errors, unknown barcode |
| `--warning` | amber-500 surface + zinc-950 text (9.3:1) | amber-400 + zinc-950 | Printer paper-out, oversold stock, backup reminder |
| `--info` | sky-700 | sky-400 | Neutral notices, sync status |
| `--ring` | emerald-600, **3 px**, offset 2 px (3.8:1 vs white) | emerald-400 (10.4:1 vs zinc-950) | Keyboard focus. Always visible, never removed. |
| `--chart-1…5` | emerald-600, sky-600, amber-500, violet-600, zinc-500 | -400 variants | Charts (§1.4). Never colour-only: labels and patterns too. |

- **Contrast rules:**
  - Till text must reach **≥ 7:1**, which is WCAG AAA, chosen for glare.
  - All other text must reach ≥ 4.5:1, and control boundaries and focus rings ≥ 3:1.
  - **White text on emerald-600 (3.8:1) fails** for text, which is why primary buttons use emerald-700 in light mode. Emerald-600 is fine for the focus ring, a non-text indicator that only needs ≥ 3:1.
  - All ratios here were computed with the WCAG 2.x relative-luminance formula.
  - CI runs an automated contrast check over the token pairs (Block 0.8).
- **Themes:** light (default for bright counters) and dark (evening shifts, and back office on request).
  - Switching uses the `.dark` class on `<html>`.
  - The choice is stored per device in `device_settings` (`ui.theme = light | dark | system`).
  - Both themes are designed from tokens, not inverted.

**Typography** (all bundled locally with `@fontsource`, offline):
- **Inter** for the Latin UI, with `font-variant-numeric: tabular-nums` on **every** money, qty and code column, so digits align and totals don't jitter as they change.
- **Noto Sans Sinhala** for `:lang(si)`, at line-height ≥ 1.6 (§1.8).
- **JetBrains Mono** only for barcodes, SKUs and receipt numbers.
- **Type scale** (rem from a 16 px base; nothing interactive below 14 px, no body text below 16 px):

| Role | Size / weight | Where |
|---|---|---|
| Grand total | 40 px / 700, tabular | Till total panel |
| Payment due and change | 32 px / 700 | Payment dialog |
| Section titles | 20 px / 600 | Panels, dialogs |
| Cart lines | 18 px / 500 (name), 18 px / 600 tabular (amount) | Till cart |
| Body and inputs | 16 px / 400 | Everywhere |
| Secondary and meta | 14 px / 400 | Timestamps, hints, `<Kbd>` |

**Spacing, sizing and layout:**
- 4 px grid: spacing steps 4, 8, 12, 16, 24 and 32. Radius is 8 px for controls and 12 px for panels.
- **Touch targets ≥ 48 × 48 px**, with **≥ 8 px between adjacent targets** on all till screens and dialogs.
- Button sizes: `pos` = 48 px (`h-12`), `pos-lg` = 64 px (`h-16`) for **Pay**, the keypad and payment methods, `default` = 40 px (mouse-only back-office forms).
- Rows are 48 px, or 36 px in an opt-in compact density for mouse-driven back-office PCs.
- **Till layout at 1024×768** (no scrolling of the primary regions):

  ```
  ┌──────────────────────────────── top bar 56px: store · counter T01 · staff · sync pill · EN|සිං · lock ──┐
  │ scan/search field (F3) 56px · customer phone (F4) · quick-sale bar (F2)                                     │
  ├──────────────────────────────────────────────┬────────────────────────────────────────────────────────────┤
  │ CART (≈58%) — virtualised, 48px lines,       │ quick keys / search results (48px tiles)                      │
  │ newest line at bottom, auto-scroll           │                                                              │
  │                                              │ numeric keypad (64px keys)                                    │
  ├──────────────────────────────────────────────┼────────────────────────────────────────────────────────────┤
  │ items · discounts · TOTAL Rs 12,450.00 (40px)│ [Hold F8] [Discount F6/F7] [Void F10]   [  PAY  F9  ] 64px   │
  └──────────────────────────────────────────────┴────────────────────────────────────────────────────────────┘
  ```

**Dual-mode input:**
- **Keyboard-first:**
  - The till has a single focus model: after any action, focus returns to the scan field.
  - The full shortcut map is in §1.4: F1 help, **F2 quick sale, F3 search, F4 phone, F9 Pay**, F8 hold, F10 void, F11 recall, Esc cancel, Ctrl+L lock, Ctrl+Shift+L language, Numpad `+` pay, Numpad `/` phone.
  - Every till button shows its key as a visible **`<Kbd>` badge** (e.g. `PAY [F9]`).
  - In dialogs, Tab order is logical, Enter triggers the default action and Esc cancels. A focus trap is mandatory, and focus returns to where it was on close.
- **Touch:**
  - **Touch screen mode** is a per-device toggle (Settings → Touch screen; `device_settings` key `ui.touch_mode`, default off, never synced). Only when it is on do the till's numeric keypad, the payment dialog's keypad and the PIN pads' digit keys show. Off, the masked PIN field and every input stay fully keyboard-driven.
  - No hover-only affordances, no long-press-only actions, and no drag-required interactions.
  - Pressed state appears within one frame (`active:` scale 0.98 plus a darker fill).
- Shortcuts are suspended while a text field is focused, except for F-keys and Esc, and while a scanner burst is being captured (§7.1).

**Micro-interactions and feedback (budgets verified in Block 1.12):**

| Event | Visual (≤ 50 ms to first frame) | Audio (bundled WAV, Web Audio, volume in settings) |
|---|---|---|
| Item added (scan, quick key, quick sale) | The new or incremented line flashes emerald (150 ms background fade), the qty "bumps" (scale 1 → 1.08 → 1, 120 ms), and the cart auto-scrolls. Transitions animate only `transform`, `opacity` and `background-color`, never layout. | Short high "tick" (≈ 880 Hz, 40 ms) |
| Unknown barcode | Scan field border and toast in `--destructive`, with no modal | Double low tone (≈ 220 Hz × 2, 80 ms each) |
| Approval required | `ApprovalDialog` slides up (150 ms), with the PIN pad focused | Single mid tone |
| Sale complete | An emerald "Sale complete ✓ · Change Rs 550.00" banner for 2 s, which doesn't block the next scan | Rising two-note chime |
| Printer or system problem | A persistent amber banner with actions, until resolved | One-time warning tone |

- **Optimistic but honest:**
  - A cart line appears from the local product cache instantly. Lookup is a local indexed IPC call, budgeted under 10 ms.
  - Totals shown on the till are previews. The receipt and the sealed order use Rust's authoritative totals (§3.7). If they ever differ, the payment dialog shows Rust's figures before taking money.
- `prefers-reduced-motion` and the device setting "Reduce motion" turn off flashes and bumps (instant colour change instead), with audio unaffected. No animation on the till exceeds 200 ms.
- **Accessibility:**
  - `aria-live="polite"` announces cart changes and totals.
  - Every icon button has an accessible label in both languages.
  - Error text sits next to its field.
  - Colour is never the only signal: icons and text accompany every status colour.

**Data tables and charts:** TanStack Table v8 + Virtual, and shadcn Charts (§1.4). Rows have fixed heights (48 or 36 px), a sticky header, `tabular-nums` money columns aligned right, and a keyboard row cursor (↑/↓, Enter opens). The 60 fps target is verified by the Block 2.10 scroll benchmark.

---

## 2. Folder structure

```
GayanShop/
├── docs/
│   ├── ARCHITECTURE.md              ← this file
│   ├── adr/                         ← one markdown file per architectural decision
│   └── hardware/                    ← tested printer models, ESC/POS quirks, label templates
├── supabase/
│   ├── config.toml
│   ├── migrations/                  ← Postgres DDL, RLS policies, sync RPCs, audit chain verifier
│   ├── functions/                   ← edge functions (device registration)
│   └── tests/                       ← pgTAP: RLS, idempotency, owner invariant
├── src/                             ← React frontend
│   ├── main.tsx
│   ├── app/
│   │   ├── App.tsx
│   │   ├── router.tsx               ← route guards read the session mirror (UX only)
│   │   ├── providers.tsx            ← QueryClient, I18nextProvider, Theme, Toaster, ScannerProvider, SessionProvider
│   │   └── layouts/
│   │       ├── PosLayout.tsx        ← full-screen till (1024×768 baseline, 48 px targets)
│   │       └── BackOfficeLayout.tsx ← sidebar: Products, Batches, Quick Sales, Customers, Reports, Staff, Audit, Settings
│   ├── components/
│   │   ├── ui/                      ← shadcn generated components (incl. chart.tsx)
│   │   ├── common/                  ← Money, Kbd, EmptyState, ConfirmDialog, PeriodPicker, LanguageToggle
│   │   ├── data-table/              ← DataTable (TanStack Table v8 + Virtual), column helpers, infinite loader
│   │   ├── charts/                  ← RevenueExpenseBar, SalesTrendArea, PaymentDonut (shadcn Chart/Recharts)
│   │   ├── auth/
│   │   │   ├── Can.tsx              ← <Can perm="VOID_ORDER">…</Can> (hides only)
│   │   │   └── ApprovalDialog.tsx   ← supervisor PIN prompt → approval token
│   │   └── sync/SyncStatusPill.tsx
│   ├── features/                    ← vertical slices
│   │   ├── pos/
│   │   │   ├── components/          ← Cart, CartLine, ProductSearch, QuickSaleBar, PaymentDialog,
│   │   │   │                          NumericKeypad, HeldSalesSheet, NoSaleButton, CustomerPhoneField
│   │   │   ├── hooks/               ← useCart, useCheckout, usePosHotkeys
│   │   │   ├── store/cart.store.ts
│   │   │   └── lib/
│   │   │       ├── pricing.ts       ← preview totals (Rust is authoritative)
│   │   │       └── parseQuickSale.ts← "3* Bucket 10L 450" parser, unit-tested
│   │   ├── cash/                    ← open/close session, pay-in/out, Z-report
│   │   ├── products/
│   │   ├── batches/                 ← intake wizard, allocation, receive, batch P&L, close
│   │   ├── quick-sales/             ← frequency review, promote/ignore
│   │   ├── inventory/               ← adjustments, damage, counts, movement history, valuation
│   │   ├── orders/                  ← history, reprint, same-session void
│   │   ├── customers/               ← customer list, profile, purchase history, spend analytics
│   │   ├── labels/                  ← label print queue, 2-up / 3-up template preview
│   │   ├── expenses/
│   │   ├── reports/                 ← period reports, P&L, payment breakdown, stock valuation, charts
│   │   ├── staff/                   ← users, roles, permission matrix editor
│   │   ├── audit/                   ← audit log viewer, chain verification
│   │   ├── settings/                ← store, tax, timezone/cutoff, printers, label templates, device,
│   │   │                              language default, Backup Database Now
│   │   └── auth/                    ← PIN login, lock screen, first-run wizard, device registration
│   ├── db/                          ← the ONLY place the UI talks to Rust
│   │   ├── bindings.ts              ← GENERATED by tauri-specta. Never edit.
│   │   ├── client.ts                ← error mapping (incl. APPROVAL_REQUIRED → ApprovalDialog)
│   │   ├── query-keys.ts
│   │   ├── events.ts
│   │   └── hooks/
│   ├── hardware/
│   │   ├── scanner/{ScannerProvider.tsx, detectScan.ts}
│   │   └── printers.ts
│   ├── lib/{money.ts, phone.ts, permissions.ts, utils.ts}
│   ├── i18n/
│   │   ├── index.ts                 ← i18next init (bundled resources, fallback 'en')
│   │   └── locales/{en,si}/         ← common, pos, products, batches, customers, reports, staff,
│   │                                  settings, errors, audit .json (CI checks key parity)
│   ├── styles/globals.css           ← tokens, chart colours, Noto Sans Sinhala @font-face, :lang(si) rules
│   └── test/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   ├── migrations/                  ← 0001_init.sql, …
│   └── src/
│       ├── main.rs
│       ├── lib.rs                   ← builder: plugins, state, commands, specta export
│       ├── error.rs                 ← AppError incl. Forbidden{perm}, ApprovalRequired{perm}
│       ├── state.rs                 ← AppState { db, device, session, approvals, sync, printers }
│       ├── domain/                  ← pure logic, no IO, heavily unit-tested
│       │   ├── money.rs
│       │   ├── pricing.rs           ← authoritative totals
│       │   ├── business_date.rs
│       │   ├── allocation.rs        ← batch cost allocation (integer, largest-remainder)
│       │   ├── fifo.rs              ← layer selection for sales (and exact reversal on void)
│       │   ├── discount.rs          ← bill-discount apportionment + limit check
│       │   ├── payments.rs          ← tender rules: no negative payments, change only from cash
│       │   ├── phone.rs             ← Sri Lankan phone normalisation → E.164
│       │   ├── quick_sale.rs        ← custom_key normalisation
│       │   ├── permissions.rs       ← bitflags Permission, effective-permission calc
│       │   └── receipt_no.rs
│       ├── auth/
│       │   ├── pin.rs               ← argon2id hash/verify, lockout
│       │   ├── session.rs           ← current staff, idle auto-lock
│       │   ├── approvals.rs         ← in-memory single-use approval tokens
│       │   └── guard.rs             ← require(perm, approval) → Actor
│       ├── audit/
│       │   ├── writer.rs            ← audit::record(tx, actor, action, entity, meta)
│       │   └── chain.rs             ← per-device SHA-256 hash chain + verify
│       ├── db/
│       │   ├── mod.rs               ← pool, PRAGMAs, migrate!
│       │   ├── models.rs
│       │   └── repo/                ← products, batches, stock, orders, cash, expenses, staff, customers,
│       │                              quick_sales, settings, reports, rollups
│       ├── commands/                ← #[tauri::command]: guard → tx → repo → audit → outbox → commit → emit
│       │   ├── auth.rs  staff.rs  products.rs  batches.rs  inventory.rs  orders.rs  customers.rs
│       │   ├── cash.rs  expenses.rs  quick_sales.rs  reports.rs  audit.rs  labels.rs
│       │   ├── hardware.rs  sync.rs  backup.rs  system.rs
│       ├── sync/{worker.rs, outbox.rs, push.rs, pull.rs, auth.rs, protocol.rs}
│       ├── hardware/
│       │   ├── printer/{mod.rs, transport_spooler.rs, transport_network.rs, transport_serial.rs}
│       │   ├── escpos/              ← receipt builder, code pages, QR, cut, drawer kick
│       │   ├── label/
│       │   │   ├── layout.rs        ← template → positioned elements (mm), shared by preview + printers
│       │   │   ├── templates.rs     ← built-ins: 2-up 2″×1″, 3-up 1″×1″
│       │   │   ├── tspl.rs          ← TSPL emitter (multi-column rows)
│       │   │   └── zpl.rs           ← ZPL emitter (multi-column rows)
│       │   └── scanner_serial.rs
│       └── backup.rs                ← VACUUM INTO snapshot, integrity check, manifest, (Phase 4) schedule
├── tests/e2e/                       ← WebdriverIO + tauri-driver
├── .github/workflows/
├── components.json  package.json  tsconfig.json  vite.config.ts  README.md
```

**Rules that keep this layout scalable:**
- `features/*` never imports from another feature. Shared code gets promoted to `components/`, `db/` or `lib/`.
- **Every** mutating Rust command follows the same shape:
  ```rust
  let actor = guard::require(&state, Permission::VOID_ORDER, args.approval)?;   // RBAC
  let mut tx = state.db.begin().await?;
  repo::orders::void(&mut tx, &actor, &args).await?;                            // business rows + outbox
  audit::record(&mut tx, &actor, Action::VoidOrder, Entity::Order(id), meta).await?; // audit + outbox
  tx.commit().await?;
  emit_changed(&app, &["orders", "stock_movements", "audit_logs"]);
  ```
- Only `db/` and `hardware/` call `invoke`.

---

## 3. Domain model

### 3.1 Batches and cost layers

A **batch** is one wholesale purchase, such as "Shoe batch – Pettah – 8 Nov". It has a supplier, purchase date, total cost and extra charges (transport, handling). A batch contains **batch items**: one row per product, holding qty received and the allocated unit cost. Each batch item is a **cost layer**.

```
batches (header)  1 ──── n  batch_items (cost layers)  1 ──── n  stock_movements (ledger)
  landed_cost = total_cost + extra_costs     qty_received, unit_cost     qty_delta, unit_cost, revenue_net
```

**Lifecycle:** `draft → received → closed`

| State | What you can do |
|---|---|
| `draft` | Add or edit items, quantities, retail prices and allocation freely. No stock exists yet. |
| `received` | "Receive" runs **one transaction**: it computes the allocation, fixes `unit_cost` on every layer, and writes a `batch_intake` stock movement per layer. It also stores `allocation_variance` and audits `BATCH_RECEIVE`. **Costs are now locked.** |
| `received` → `draft` (reopen) | Allowed only if **no unit of the batch has moved** except intake. It reverses the intake movements. Requires `BATCH_MANAGE` and is audited. |
| Late item added to a `received` batch | Allowed (the owner forgot an item). It gets `added_after_receipt = 1` and **`unit_cost = 0`**, because the batch cost is already fully allocated to the other items. Its sales are effectively pure recovery or profit. |
| `closed` | Allowed when every layer's `on_hand ≤ 0`, or the remaining stock has been written off. The batch becomes read-only for reports. Audited as `BATCH_CLOSE`. |

**Allocation methods.** Everything is in integer cents. `landed = total_cost + extra_costs`.
- `even_per_unit`: each unit has weight 1.
- `by_retail_value`: weight = `retail_price_at_intake × qty`. Expensive items carry more cost.
- `manual`: the owner types each unit cost. It's validated, and `allocation_variance` is shown live.

`domain::allocation` works as follows:
1. Compute each layer's exact share of `landed`.
2. Round each layer's *total* share using the **largest-remainder method**, so Σ line totals = `landed` exactly.
3. Set `unit_cost = floor(line_total / qty)`.
4. The leftover sub-cent rounding goes into `batches.allocation_variance = landed − Σ(unit_cost × qty_received)`. It's always 0 ≤ v < Σqty cents.

`allocation_variance` is reported as a separate line in the batch P&L. In the store P&L it's added to COGS on the batch's receive date. That keeps both reports reconciled to the cent with what the owner actually paid.

**Every unit of tracked stock belongs to a layer.** This is what makes stock valuation exact. Three cases follow from it:
- **Go-live:** a system batch `OPENING-STOCK` (`is_system = 1`) receives the existing shelf stock at entered costs.
- **Ad-hoc purchase of one item:** a one-line batch.
- **Positive count correction** (found 3 extra): the stock goes into the product's **newest** layer at that layer's cost. If the product has no layer, it goes into the system batch `ADJUSTMENTS` at the product's `default_cost`.

**Phase 1 implementation notes (Block 1.2).**
- Batch codes are `B-{business year}-{device digit}{seq:03}`, e.g. `B-2026-1045` from T01, so counters never collide offline.
- If a manual-cost batch is received with no invoice total and no extra costs (opening stock, quick purchases), `total_cost` is set to Σ(unit_cost × qty) and the variance is 0. With an invoice total, `allocation_variance = landed − Σ(unit_cost × qty)`, which can be negative in manual mode.
- A tracked product sold before it has any layer gets one in `ADJUSTMENTS` at its `default_cost` (`added_after_receipt = 1`, `qty_received = 0`), created inside the checkout transaction. The sale is never blocked, and the layer shows as oversold until stock is received.

**Block 2.1 implementation notes (`db/repo/batches.rs`, the intake wizard).**
- The method is chosen per batch (default `even_per_unit` for a new purchase, `manual` for opening stock and quick purchases) and can change freely while it's a draft. `batch_allocation_preview` returns exactly what Receive would fix (each layer's unit and line cost, the landed cost, the variance) or why it would be refused, so the wizard shows the variance live.
- `even_per_unit` and `by_retail_value` need an invoice total (`BATCH_TOTAL_REQUIRED`); `by_retail_value` with all retail prices zero has nothing to weigh (`BATCH_NO_WEIGHT`).
- Reopen writes one `batch_reopen` movement per layer, mirroring its intake. It returns the locked costs as the draft's editable manual costs, and is refused once anything but intake has moved (`BATCH_HAS_MOVEMENTS`). The system batches can't be reopened or closed (`BATCH_SYSTEM`).
- A late item is received immediately at unit cost 0 (`BATCH_LATE_ITEM`). If the batch is later reopened and received again, it becomes an ordinary layer that shares the cost.
- Close is refused while any layer has stock (`BATCH_HAS_STOCK`). The batch P&L (`BatchDetail.pnl`) is read from `batch_item_stock` and audited with `BATCH_CLOSE`.

**Block 2.3 implementation notes (`db/repo/inventory.rs`).**
- Damage (`STOCK_DAMAGE`, reason broken / expired / stolen / faulty / other with a note) and negative adjustments take units FIFO at each layer's cost, and can't remove more than is on hand (`STOCK_INSUFFICIENT`). Positive adjustments and found stock go into the newest layer at its cost, or into `ADJUSTMENTS` at `default_cost`. An adjustment always needs a reason (`STOCK_NOTE_REQUIRED`).
- A stock count (`STOCK_ADJUST`) sets each counted product to the shelf figure with `count_correction` movements in one transaction, audited once as `STOCK_COUNT` with system and counted figures. Matching lines write nothing; a single bad line refuses the whole count. Counting an oversold product back to its real figure is how a negative layer is fixed.
- "Write off what's left" (`write_off_batch`, `BATCH_MANAGE` + `STOCK_ADJUST`) turns a received batch's remaining stock into `adjustment` movements on its own layers, so the batch can then be closed. The loss shows in its P&L.
- Valuation "now" reads `batch_item_stock`; "as of" a business date sums the ledger up to that date. Both use today's prices for retail and list oversold layers separately. History, valuation and oversold layers need `REPORT_FINANCIAL`, because they show cost.
- **Products & Stock (UI, 2026-11-10).** The catalog and the stock tools share one page at `/products`. Its tabs are All Products, Stock Adjustments (count + damage/adjust), Stock Valuation (valuation + oversold layers), and Movement History, and the tab is kept in the URL as `?tab=adjustments|valuation|history`. `/inventory` redirects to `?tab=adjustments`. The product list carries `on_hand` from `stock_levels` (0 when there's no row), and the Stock column reads it: a count, low at ≤ 5 units, out at 0, oversold below 0, and a dash when untracked. The quick filters are All / Low Stock / Out of Stock, and Out of Stock includes oversold. Each tab keeps its section permissions.

**Layer selection at checkout (FIFO, offline-safe).** For each tracked line of qty *n*:
1. Take layers of that product ordered by `batches.received_at, batch_items.id`.
2. Consume from the **oldest layer with positive local `on_hand`**, splitting across layers if needed.
3. If no layer has stock left (oversold), draw the rest from the **newest** layer, which goes negative.
4. Each slice becomes one `stock_movements` row, carrying the layer's `unit_cost` and its share of the line's **`revenue_net`** (ex-tax, after discount, apportioned by qty, remainder on the last slice).

Two terminals can both drain the same layer offline. After sync, that layer is negative. **This is allowed and reported** ("Oversold layers") and is never auto-rebalanced. Rebalancing would rewrite history.

**Batch metrics.** These come from the derived `batch_item_stock` table, maintained by trigger:

| Question | Metric |
|---|---|
| How much did this batch cost? | `landed_cost = total_cost + extra_costs` |
| How much has sold so far? | `units_sold`, `revenue_net`, plus quick-sale lines tagged with the batch |
| What did that stock cost? | `cogs = Σ sold layer cost` |
| What's left? | `units_remaining`, `remaining_value_cost = Σ on_hand × unit_cost`, `remaining_value_retail = Σ on_hand × current price` |
| Losses? | `written_off_qty`, `written_off_cost` (damage, negative counts) |
| Net profit so far (realised) | `revenue_net − cogs − written_off_cost − allocation_variance` |
| Payback (cash view) | `revenue_net − landed_cost`. It's negative until the batch has paid for itself. |
| Projected profit if the rest sells at price | `payback + remaining_value_retail` |
| Sell-through % | `units_sold / units_received` |

### 3.2 Quick sale and promotion

**Speed target: F2 → type → Enter.**
- F2 focuses a single-line **Quick Sale bar** on the till.
- The cashier types `Blue bucket 450`. A live preview shows `Blue bucket · Rs 450.00 × 1`.
- Enter adds the line. That's two keys plus typing.

`parseQuickSale` grammar:
```
[qty "*"] name price          e.g.  "3* Bucket 10L 450"  →  qty 3, "Bucket 10L", 450.00
price  = trailing token matching ^\d+(\.\d{1,2})?$   ("10L" is not a price, so names with sizes are safe)
```
- If there's no valid trailing price, Enter is disabled and the preview says "price missing".
- **Tab** expands to a detailed form with qty, **optional cost** and a note. **Quick sales are never auto-tagged to a batch** (owner decision). They stay untagged until a manager links them during review.
- As the cashier types, suggestions appear: catalog products first, then recent quick-sale names with their last price. **Plain Enter always commits the typed quick sale.** Picking a suggestion needs ↓ then Enter, so speed never gets hijacked.
- If the typed name matches a **promoted** key, the bar shows a hint: "Now a product: Blue Bucket 10L (scan or ↓ Enter)".

**Stored on the line, at write time:**
- `is_custom = 1` and `product_id = NULL`.
- `name` as typed.
- `custom_key`: normalised, meaning lowercase, trimmed, collapsed whitespace, price stripped, NFC. It's the grouping key, and it's stored because lines are immutable.
- `batch_id`: always NULL at the till. Quick-sale lines are untagged, and promotion is forward-only, so it stays NULL.
- `cost_total` and `cost_known`. Cost is unknown (0, `cost_known = 0`) unless the cashier entered one. **Unknown-cost revenue is reported separately**, so profit is never silently overstated.

**Quick-sale review** (`QUICK_SALE_PROMOTE` permission) is a back-office table grouped by `custom_key`. It shows:
- Times sold, units and revenue
- Last and min/max price
- First and last seen
- The list itself reads the derived `quick_sale_stats` table. Opening one key shows its median price and which counters sold it, computed on demand from `order_items` through `ix_items_custom`.
- Status: `new` / `promoted` / `ignored`

It's sorted by frequency. Two one-click actions:
- **Promote to product** runs one transaction. It creates a `products` row (name = latest spelling, editable; price = last price, cost entered by the manager). If a batch is chosen, it adds a layer to it, with a shelf count entered so stock starts correct. It writes a `quick_sale_links` row (`custom_key → product_id, batch_id`) and audits `QUICK_SALE_PROMOTE`.
- **Ignore** writes a link with `action = 'ignored'`, which hides one-off noise.

**Block 2.4 implementation notes (`db/repo/quick_sales.rs`).**
- The review reads `quick_sale_stats` joined to the key's decision: New, Promoted or Ignored, filterable. Opening a key computes the median price, the counters that sold it (by receipt prefix) and its recent spellings from completed orders only.
- Promote is one transaction:
  - It creates the product: SKU always; an in-store barcode optionally; tracked; the manager's cost. Only one promotion per key (`QUICK_SALE_ALREADY_PROMOTED`).
  - With a shelf count and a chosen received batch, the stock arrives as a layer added after receipt (`added_after_receipt = 1`) at the manager's cost.
  - Without a batch, it arrives as a `count_correction` into `ADJUSTMENTS` at that cost.
  - An ignored key can still be promoted later.
- The till's Quick Sale bar asks `quick_sale_hint` for the typed name. A promoted key shows "Now a product: … (↓ then Enter)"; plain Enter still commits the typed quick sale.

**Promotion is forward-only.** Past quick-sale lines keep their original attribution, and no historical batch P&L changes. The review screen can still show the key's pre-promotion history through the link. `quick_sale_links.id = UUIDv5(store_id, custom_key)`, so two managers promoting the same key on different terminals produce the *same* row, which merges by LWW.

### 3.3 Sales are final: same-session void is the only correction

**Store policy (owner, final for v1):**
- **All sales are final.** There are no refunds and no exchanges, in cash or any other form.
- The only correction is a **same-session void** (`VOID_ORDER`) for an immediate cashier mistake, such as a wrong item, price or quantity, a double scan, or a customer cancelling before leaving the counter. It's only allowed while the till's cash session is still open.
- The receipt footer (`receipt.footer`, seeded at first run) prints exactly: *"All sales are final. No refunds or exchanges. Thank you, come again!"*
- Exchanges and credit notes are **deferred to Phase 5** (§8). The schema keeps extension points for them (below), so adding them later needs no rewrite.

**Void rules (enforced in Rust and checked again in SQL):**

| Rule | Detail |
|---|---|
| Scope | Same device **and** same cash session, which must still be `open`. That implies the same business day. Once the session is closed (Z-report printed), the order can't be voided by anyone, including the Owner. |
| Who | `VOID_ORDER` is held by Supervisor, Manager and Owner. A cashier's void attempt returns `ApprovalRequired { perm: VOID_ORDER }`. A supervisor enters their PIN, and the single-use approval token (§3.5) is bound to that order id. |
| Reason | Required, from a fixed list: `WRONG_ITEM`, `WRONG_PRICE`, `WRONG_QTY`, `DUPLICATE_SCAN`, `CUSTOMER_CANCELLED`, `OTHER` (free text required). It's stored in `orders.void_reason` and the audit metadata. |
| Whole order only | A void cancels the whole order. To fix one line, void and re-ring. That keeps the ledger simple and the Z-report honest. |
| Mechanism | One transaction does all of the following. Order `status: completed → voided` (the only update the immutability trigger allows) sets `voided_at`, `voided_by` and `void_approved_by`. A `void` stock movement is appended for every original sale slice, exactly reversing it (same layer, same unit cost, negative revenue). The rollup trigger removes the order from that day's `daily_sales_summary` and `daily_payment_summary`. `VOID_ORDER` is audited (receipt no., amount, reason, approver), and everything goes to the outbox. |
| Cash | A voided cash sale means the cashier hands back the cash just taken. `expected_cash` excludes voided orders, so the drawer still reconciles. The void kicks the drawer if the order had a cash payment, and prints a short **VOID slip** (receipt no., amount, reason, approver) for the cash-up. |
| Time limit | **The end of the open cash session (owner-confirmed).** `void.max_minutes` stays `null`. |

**Phase 5 extension points kept in v1:**
- `orders.type` exists, constrained to `'sale'` for now.
- Permission bits 6 and 7 are **reserved** for `EXCHANGE_PROCESS` / `EXCHANGE_LATE`.
- Every line stores its pro-rata `order_discount_share` (§3.7), so a future exchange can value a returned item at what the customer actually paid.
- The payment ledger shape (`order_payments` rows per tender) can later carry credit-note issue and redeem rows.

### 3.4 Cash sessions (shifts) and Z-report

Each device has at most **one open `cash_session`**. Sales, cash movements and drawer kicks require one.
- **Open:** cashier, opening float.
- **During:** cash sales, `pay_in` / `pay_out` (with a reason, e.g. "petty cash – tea"), and `no_sale` drawer kicks. Each is a `cash_movements` row, audited.
- **Close:** the system computes `expected_cash = opening_float + Σ cash payments of completed (non-voided) orders, net of change + pay_ins − pay_outs`. Payments are never negative, because there are no refunds. The cashier counts the drawer (optionally by denomination), and the system stores `counted_cash` and `variance`, then prints the **Z-report**. **Closing the session ends the void window** for every order in it.
  - Totals by payment method
  - Sales count and value
  - Voids (count, value, reasons, approvers), discounts and overrides
  - Quick-sale totals
  - Cash reconciliation

A **daily store report** aggregates all devices for a business date. The Z-report is per device per session.

**Expenses and the drawer (Block 2.6).** Expenses need `EXPENSE_MANAGE`. An expense marked **paid from the drawer** must be cash, dated today, and needs this counter's open session. In the same transaction it writes a `pay_out` cash movement (reason `Expense: <category> – <payee>`, audited `CASH_PAY_OUT`, with a drawer kick) and stores `cash_session_id`, so expected cash and the Z-report's pay-outs include it. Moving drawer cash also needs `CASH_PAY_IN_OUT`, which a supervisor PIN can lend (target `cash_drawer`). Because cash movements are append-only, a drawer-paid expense keeps its amount, date and method (`EXPENSE_DRAWER_LOCKED`); only the category, payee and note change. Deleting one is allowed only while its session is still open on this counter, and appends a matching `pay_in` ("Expense deleted: …"), so the cash goes back. After the session closes it is final. Other expenses can be back-dated (never into the future), edited and soft-deleted freely, and never move cash. Categories can be added, renamed and retired (`SETTINGS_CHANGE`), never removed; names are unique case-insensitively, and a retired category takes no new expenses.

### 3.5 Role-based access control

**Model.**
```
effective(user) = (role.permissions | user.grant_mask) & ~user.deny_mask
```
- The `Owner` system role always has ALL bits, and they can't be edited.
- Roles also carry limits: `max_discount_bp` and `max_override_bp`. Going above a limit triggers an approval request (`ApprovalRequired`) rather than a hard deny (`Forbidden`). Seeded limits:

  | Role | `max_discount_bp` | `max_override_bp` |
  |---|---|---|
  | Cashier | **500 (5%, owner rule)** | 0 |
  | Supervisor | 2000 | 2000 |
  | Manager | 5000 | 5000 |
  | Owner | 10000 | 10000 |

  The discount check itself is defined in §3.7.
- Enforcement is in **Rust** (`auth::guard::require`). The UI's `<Can>` only hides controls.

**Permission bits.** They're stable: never renumber, only append. All are below 2^53.

| Bit | Key | Cashier | Supervisor | Manager | Owner |
|---|---|:-:|:-:|:-:|:-:|
| 0 | `SALE_CREATE` | ✅ | ✅ | ✅ | ✅ |
| 1 | `QUICK_SALE` | ✅ | ✅ | ✅ | ✅ |
| 2 | `PRICE_OVERRIDE` | – | ✅ | ✅ | ✅ |
| 3 | `LINE_DISCOUNT` (item-level) | ✅ (≤ 5%) | ✅ | ✅ | ✅ |
| 4 | `ORDER_DISCOUNT` (full bill) | ✅ (≤ 5%) | ✅ | ✅ | ✅ |
| 5 | `VOID_ORDER` (same session only; cashiers need supervisor approval) | – | ✅ | ✅ | ✅ |
| 6 | *reserved* for `EXCHANGE_PROCESS` (Phase 5). Not seeded. | – | – | – | – |
| 7 | *reserved* for `EXCHANGE_LATE` (Phase 5). Not seeded. | – | – | – | – |
| 8 | `DRAWER_NO_SALE` | – | ✅ | ✅ | ✅ |
| 9 | `CASH_PAY_IN_OUT` | – | ✅ | ✅ | ✅ |
| 10 | `CASH_SESSION_MANAGE` (open/close own) | ✅ | ✅ | ✅ | ✅ |
| 11 | `CASH_SESSION_FORCE_CLOSE` (others') | – | ✅ | ✅ | ✅ |
| 12 | `REPRINT_RECEIPT` | ✅ | ✅ | ✅ | ✅ |
| 13 | `STOCK_ADJUST` | – | – | ✅ | ✅ |
| 14 | `STOCK_DAMAGE` | – | ✅ | ✅ | ✅ |
| 15 | `BATCH_MANAGE` | – | – | ✅ | ✅ |
| 16 | `PRODUCT_MANAGE` | – | – | ✅ | ✅ |
| 17 | `QUICK_SALE_PROMOTE` | – | – | ✅ | ✅ |
| 18 | `LABEL_PRINT` | – | ✅ | ✅ | ✅ |
| 19 | `VIEW_COST` (cost and margin fields anywhere) | – | – | ✅ | ✅ |
| 20 | `REPORT_SALES` (revenue, payments, Z) | – | ✅ | ✅ | ✅ |
| 21 | `REPORT_FINANCIAL` (COGS, profit, valuation, batch P&L) | – | – | ✅ | ✅ |
| 22 | `EXPENSE_MANAGE` | – | – | ✅ | ✅ |
| 23 | `SETTINGS_MANAGE` | – | – | – | ✅ |
| 24 | `USER_MANAGE` (staff, roles, permissions) | – | – | – | ✅ |
| 25 | `AUDIT_VIEW` | – | – | ✅ | ✅ |
| 26 | `SYNC_MANAGE` (dead ops, re-register device) | – | – | – | ✅ |
| 27 | `BACKUP_RESTORE` | – | – | – | ✅ |
| 28 | `DATA_EXPORT` | – | – | ✅ | ✅ |
| 29 | `CUSTOMER_VIEW` (profiles, purchase history, spend analytics) | – | – | ✅ | ✅ |
| 30 | `BACKUP_CREATE` ("Backup Database Now") | – | – | ✅ | ✅ |

- At the till, a cashier only sees the customer's name and visit count after entering a phone number. Full history needs `CUSTOMER_VIEW`.
- `BACKUP_RESTORE` stays Owner-only.
- `CUSTOMER_VIEW`, `BACKUP_CREATE` and `BACKUP_RESTORE` are **not approvable**: a supervisor PIN can't lend them.

The defaults above are seeded in Phase 0 and can be edited per store (except Owner). **The bit assignments are frozen once Phase 0 ships.** From then on, append only. The reserved bits 6 and 7 must not be reused for anything else.

**Supervisor approval (step-up auth).**
1. A command fails with `ApprovalRequired { perm, reason }`. For example, a cashier sets a price below list.
2. The UI opens `ApprovalDialog`, and a supervisor enters their PIN.
3. `request_approval(perm, target_ref, supervisor_id, pin)` verifies the PIN and checks that the supervisor holds `perm`. It returns an `approval_id`.
4. The token is held **in memory only**. It's single-use, expires in 60 s, and is bound to `(perm, target_ref, requesting_staff, device)`.
5. The original command is retried with `approval_id`. Rust consumes the token and records `approved_by` on the business row *and* in the audit log.

**Identity and PINs.**
- Staff PINs are 4–8 digits, hashed with **argon2id** (moderate parameters, ~100 ms).
- 5 failures lock the account for 5 minutes on that device (`auth_attempts`, local), and the lockout is audited.
- Login is required to use the till. Idle auto-lock is configurable (default 5 min), with fast user switching.
- **Known trade-off:** PIN hashes sync to every terminal so offline login works. A 4–6-digit PIN can be brute-forced by anyone holding a DB copy, so PINs protect against *casual* misuse at the counter, not a stolen database.
  - Mitigations: use 6+ digits for Manager and Owner, keep the DB file in the user profile, and plan optional Windows-account-bound DB encryption (SQLCipher) for Phase 4.
- **Staff management (Block 1.11, `USER_MANAGE`):** only an Owner can make someone an Owner, change an Owner, reset an Owner's PIN, or grant / deny `USER_MANAGE`; a permission can't be granted and denied at once; a PIN reset also lifts that person's lockout on the counter. Audited as `USER_CREATE`, `USER_UPDATE` (before/after), `USER_DISABLE` and `USER_PIN_RESET`, never with PIN material.
- **Owner invariant:** there must always be at least one active staff member with the Owner role. Rust refuses any disable or demote that breaks this, and `sync_push` rejects merged changes that would break it (dead op, surfaced to the owner).

### 3.6 Audit trail

- `audit_logs` is **append-only**. Local triggers block `UPDATE` and `DELETE`, and cloud RLS allows `INSERT` and `SELECT` only.
- Rows are written by `audit::record` **inside the action's transaction**, so an action without its audit row can't be committed.
- Each row records:
  - Timestamp, business date, `device_id`, `staff_id`, and `approved_by` when step-up auth was used
  - `action`, `entity_type` and `entity_id`
  - `metadata` JSON with before/after values, reason, amounts and the receipt number

**Tamper evidence: a per-device hash chain.**
```
hash = SHA256( prev_hash ‖ canonical_json(id, store_id, device_id, device_seq, staff_id, approved_by,
                                          action, entity_type, entity_id, metadata, business_date, created_at) )
```
- `device_seq` is a gapless per-device counter. The genesis `prev_hash` is 64 zeros.
- The chain is **per device**, because terminals interleave offline and a global chain would fork.
- The cloud verifies each device's chain on push and flags breaks. The Audit screen has a "Verify chain" button.
- This detects edits to the local file once its rows have synced. It can't stop someone with file access from rewriting *unsynced* rows, which is one more reason to sync often.

**Actions** (`AuditAction`):

| Area | Actions |
|---|---|
| Authentication | `AUTH_LOGIN`, `AUTH_LOGOUT`, `AUTH_FAILED`, `AUTH_LOCKOUT`, `APPROVAL_GRANTED`, `APPROVAL_DENIED` |
| Cash | `CASH_SESSION_OPEN`, `CASH_SESSION_CLOSE`, `CASH_SESSION_FORCE_CLOSE`, `DRAWER_KICK` (no-sale), `CASH_PAY_IN`, `CASH_PAY_OUT` |
| Pricing and orders | `PRICE_OVERRIDE`, `LINE_DISCOUNT`, `ORDER_DISCOUNT` (above the cashier's limit, or any with approval), `VOID_ORDER`, `REPRINT_RECEIPT`, `PRINT_JOB_ABANDON` (unprinted receipts dismissed: "Skip" / "Reprint later") |
| Customers | `CUSTOMER_CREATE`, `CUSTOMER_UPDATE`, `CUSTOMER_ERASE` (personal data removed on request) |
| Labels | `LABEL_PRINT` (template, product count, label count) |
| Stock and batches | `STOCK_ADJUSTMENT`, `STOCK_DAMAGE`, `STOCK_COUNT`, `BATCH_CREATE`, `BATCH_RECEIVE`, `BATCH_REOPEN`, `BATCH_CLOSE`, `BATCH_LATE_ITEM` |
| Catalog | `PRODUCT_CREATE`, `PRODUCT_UPDATE` (price/cost changes carry old → new), `PRODUCT_DELETE`, `BARCODE_CONFLICT_RESOLVE`, `QUICK_SALE_PROMOTE`, `QUICK_SALE_IGNORE` |
| Expenses | `EXPENSE_CREATE`, `EXPENSE_UPDATE`, `EXPENSE_DELETE` |
| Administration | `SETTINGS_CHANGE`, `ROLE_CREATE`, `ROLE_UPDATE`, `USER_CREATE`, `USER_UPDATE`, `USER_DISABLE`, `USER_PIN_RESET` |
| System | `STORE_INITIALIZED`, `BACKUP_CREATE` (path, size, SHA-256), `BACKUP_FAILED`, `BACKUP_RESTORE`, `DATA_EXPORT`, `SYNC_DEAD_OP_RESOLVE`, `DEVICE_REGISTER` |

Ordinary sales aren't duplicated into the audit log, because `orders` is already immutable and attributed. Overrides, discounts above the limit and voids *are* logged.

### 3.7 Discounts (owner rule: cashier ≤ 5%, item and bill level)

Prices are **tax-inclusive** (`tax.inclusive = true`). The tax rate is **0%** today (not VAT/SSCL registered). Pricing still *extracts* tax from inclusive totals, so registering later is purely a settings and rate change.

For each line:
```
line_gross            = unit_price × qty                              (shelf price, tax-inclusive)
line_discount         = item-level discount entered on that line      (needs LINE_DISCOUNT)
order_discount_share  = that line's pro-rata share of the bill discount (needs ORDER_DISCOUNT)
line_total            = line_gross − line_discount − order_discount_share   (what the customer pays, incl. tax)
tax_amount            = inclusive ? round_half_up(line_total × rate_bp / (10000 + rate_bp))
                                  : round_half_up(line_total × rate_bp / 10000)
line_net              = inclusive ? line_total − tax_amount : line_total   (ex-tax revenue)
```

- **Bill-discount apportionment:** the bill discount `D` is split across the order's new-item lines in proportion to `line_gross − line_discount`.
  - It uses the **largest-remainder method**, so `Σ order_discount_share = D` exactly.
  - Ties go to the lower `line_no`.
  - The share is stored on each line (`order_items.order_discount_share`). Batch margins use it, and the Phase 5 exchange design would use it to value returns at what the customer actually paid.
- **The limit check** is run by Rust on every line at checkout: `(line_discount + order_discount_share) × 10000 ≤ max_discount_bp × line_gross`. It uses integer cross-multiplication with no rounding. So the **combined** item and bill discount on any line must not exceed the actor's limit (5% for a cashier).
  - If any line fails, the result is `ApprovalRequired { perm: LINE_DISCOUNT | ORDER_DISCOUNT }`. A supervisor whose limit covers it approves once for the whole order.
  - A discount entered without holding the permission at all also returns `ApprovalRequired`.
- **Price overrides** (`unit_price < list_price`) are separate from discounts. They need `PRICE_OVERRIDE`, are limited by `max_override_bp`, and are never counted in the 5% check. Going below `min_price` always needs approval.
- Header totals: `discount_total = Σ line_discount + bill discount`. All discount approvals and above-limit attempts are audited.

### 3.8 Customer profiles (optional phone number at the till)

**Goal:** capture a phone number in about two seconds when the customer offers one, never slow down a sale that doesn't, and give the back office purchase history and spend analytics.

**At the till**
- **F4 or Numpad `/`** focuses the `CustomerPhoneField` in the cart header. It's numeric-only and auto-formats as `07X XXX XXXX`.
  - Enter confirms the number and returns focus to the scan field. Esc clears it.
  - Leaving it empty is the default. **A customer is never required.**
- After 9–10 digits, Rust looks the number up (`customer_lookup(phone)` is a local, indexed query, so it works offline):
  - **Known:** the cart header shows the name, if one was stored, plus "Visit #N".
  - **New:** the header shows "New customer", with an optional one-tap name field.
- At checkout, `checkout_order` attaches the customer in the same transaction. It first links the existing row or **creates one**, *before* inserting the order, then inserts the order with `customer_id` already set. `customer_id` is a foreign key, and it's frozen after insert by the order state machine. **The order stores no copy of the phone number**: it lives only on `customers`, so erasure never touches an immutable order.
- The receipt prints the phone **masked** (`Customer: 07X XXX X123`), never in full.

**Phone normalisation** (`domain::phone`, mirrored in `lib/phone.ts` for the preview):
- Sri Lankan numbers are stored as **E.164**:
  - `0771234567` → `+94771234567`
  - `771234567`, `94771234567` and `+94 77 123 4567` → the same.
  - Mobile (`07X`) and landline (`0XX`) numbers are both accepted, as 9 national digits after the leading 0.
- Anything else is rejected with a validation error, so it never gets stored half-normalised.
- Foreign numbers aren't supported in v1. Add them in Phase 5 if needed.

**Offline-safe identity.** `customers.id = UUIDv5(store_id, phone_e164)`, the same trick as `quick_sale_links`. Two counters registering the same new number while offline create the *same* row id, which merges by LWW. They never collide on the unique phone index.

**Back office** (`CUSTOMER_VIEW`, Manager and Owner):
- A **customer list**: a virtualised DataTable with search by phone or name, and sorting by total spend, visits or last visit.
- A **profile** showing name, phone, notes, first and last visit, visit count, total spend, average basket, and top products and categories.
- **Purchase history**: an infinite-scroll list of orders with drill-down to the receipt.
- **Analytics**:
  - Top customers by spend for any period (§5 periods)
  - New vs returning customers per week or month
  - The share of sales with a customer attached
- Figures come from the derived table `customer_stats` (completed orders only; voids excluded).
  - It's trigger-maintained when an order is sealed or voided, and rebuildable with `rebuild_derived`.
  - A view aggregating `orders` per customer wouldn't hold the <50 ms list budget once there are tens of thousands of orders (§13.4).
  - Per-customer history uses the index `orders(customer_id, business_date)`.

**Privacy.** Sri Lanka's Personal Data Protection Act, No. 9 of 2022, applies to customer phone numbers.
- **Consent is verbal at the counter** (owner decision). The cashier asks, "Would you like to add your phone number?", and only types it if the customer agrees.
  - The UI shows **no popup, notice dialog or confirmation step**. The field is simply optional, and Enter or Esc on an empty field moves on.
  - Store only the phone number and an optional name and note.
- **Erase on request** (`CUSTOMER_ERASE`, Owner and Manager). This clears the phone, name and notes on the `customers` row and sets `erased_at`. The row and its id remain, so past orders keep an anonymous `customer_id` and the store's totals stay correct. Orders are not modified. The same transaction drops that row's queued, unsent `sync_outbox` snapshots (an upsert carries the whole row, so the new erased snapshot supersedes them), so no unsent copy of the phone stays on disk.
  - The UUIDv5 id is derived from the phone. If the same person returns later and gives their number, the lookup finds the erased row by id. In that case, Rust **re-activates the same id** with a fresh profile. Visit history before the erasure stays anonymous, because it's still linked by id. Tell the customer this is how erasure works.
- Exporting customer data needs `DATA_EXPORT` and is audited.

**Block 2.5 implementation notes (back office):**
- **Permissions.** The list, profile, history, receipt drill-down, analytics, name/note edits and erasure all need `CUSTOMER_VIEW` (not approvable). There are no separate `CUSTOMER_UPDATE` / `CUSTOMER_ERASE` permission bits, and the bit table is frozen; `CUSTOMER_VIEW` is held by exactly the Manager and Owner roles that erasure names. Edits are audited as `CUSTOMER_UPDATE` with the changed *field names* only, never the values.
- **List.** Erased profiles are left out. Searching digits matches the phone: a number typed from its start (`07…`, `+94…`, `94…`) matches the start of the 9 national digits, while bare digits (`4567`) match anywhere. Anything else matches the name, case-insensitively. Sorting is by spend, visits, last visit or name, and at most 500 rows come back (the UI says to narrow the search when there are more).
- **Profile.** Top products and categories (5 each, by net revenue) come from completed orders. All quick sales count as one "Quick sales" row, and products without a category as "Uncategorised".
- **History.** Completed *and* voided orders appear (voids flagged, never counted in spend), newest first, keyset-paged on `(business_date, id)`. A row drills down to the receipt, with the phone masked as on paper.
- **Analytics.** Periods are the §5.2 ones (`domain::period`, shared with the reports engine). Top customers for a bounded period sum that period's completed orders; all time reads `customer_stats`. A customer counts as **new** in the week or month holding their first completed visit (`customer_stats.first_visit_date`) and as **returning** in every later one. The attached share is `Σ customer_orders / Σ sales_count` from `daily_sales_summary`.

### 3.9 Manual database backup ("Backup Database Now")

**Who and where:**
- **Settings → Backup** shows a **"Backup Database Now"** button (`BACKUP_CREATE`: Owner and Manager).
- The user picks a destination folder with the native folder picker (`tauri-plugin-dialog`), for example a plugged-in USB drive, an external disk or a secondary directory.
- The last-used folder is remembered per device (`backup.last_dir`). The picker is opened from Rust (`backup_pick_folder`, `tauri-plugin-dialog`'s Rust API), so the webview needs no dialog permission.

**How (`commands::backup::backup_now(dest_dir)`):**
1. **Pre-checks:**
   - The folder exists and is writable.
   - Free space is at least 1.2 × the current DB size, using `page_count × page_size`.
   - The target file name doesn't exist yet: `GayanShop-{device_code}-{YYYYMMDD-HHmmss}.db`.
2. **Snapshot:**
   - Run `VACUUM INTO '<dest>/<name>.partial'` on a **pooled read connection**, not the writer lock.
   - Under WAL, this reads a consistent snapshot while the till keeps selling. Writers aren't blocked, and the snapshot simply excludes commits made after it started.
   - It produces a compact, defragmented copy containing every table, including the derived ones.
   - It runs on a background task, and the UI shows a progress spinner with the elapsed time.
3. **Verify:**
   - Open the copy **read-only** and run `PRAGMA integrity_check` (it must return `ok`).
   - Check that `_sqlx_migrations` holds the expected schema version.
   - Compute the file's **SHA-256**.
4. **Finalise:**
   - Rename `.partial` to `.db`.
   - Write a sidecar `…db.json` manifest containing app version, schema version, store and device ids, device code, created_at, row counts for key tables, size and SHA-256.
5. **Record:**
   - Audit `BACKUP_CREATE` with path, size and hash.
   - Store `backup.last_manual_at` and `backup.last_manual_path` in `device_settings`. The Settings screen shows "Last backup: 2 hours ago (E:\)".
6. **Failure** (USB pulled, disk full, integrity failure): delete the `.partial` file, audit `BACKUP_FAILED` with a reason code, and show a translated error. **Sales are never affected.**

**Rules:**
- **Never copy the live `pos.db` / `-wal` / `-shm` files directly.** A raw file copy during WAL activity can produce a torn, unusable backup. Always use `VACUUM INTO`.
- **The backup contains PIN hashes and customer phone numbers.** The dialog warns: "Keep this drive safe". Optional encryption of backups (SQLCipher / age) is a Phase 4 item.
- **Restore** is a separate Owner-only flow (`BACKUP_RESTORE`), including the recovery steps in Phase 4. Before a restore, the app always takes a fresh backup of the current DB.
- **Reminder:** if there's been no manual backup in 7 days and no cloud sync configured, the back-office header shows a gentle "Back up today" banner. Until Phase 3 cloud sync, a USB backup is the shop's only off-machine copy.

---

## 4. Schemas

### 4.1 Conventions and table classes

| Class | Tables | Required columns | Merge rule (§6.3) | Pull "own write" filter |
|---|---|---|---|---|
| **Mutable (LWW)** | `store_settings`, `roles`, `staff`, `categories`, `products`, `batches`, `batch_items`, `customers`, `expense_categories`, `expenses`, `quick_sale_links`, `cash_sessions` | `id`, `store_id`, `created_at`, `updated_at`, `updated_by_device`, `deleted_at`, `version` | Last-write-wins on the row, plus server-side invariants | `updated_by_device = me` |
| **Insert-only** | `order_items`, `order_payments`, `stock_movements`, `cash_movements`, `audit_logs` | `id`, `store_id`, `device_id`, `created_at` | Same `id` ⇒ no-op | `device_id = me` |
| **Insert-only + void** | `orders` | Insert-only columns **plus** `updated_at`, `updated_by_device`, `version` (the void transition is the only update) | Insert; status `completed → voided` only | `updated_by_device = me` |
| **Local-only** | `device_settings`, `sync_ctx`, `receipt_counters`, `sku_counters`, `held_carts`, `print_jobs`, `auth_attempts`, `sync_outbox`, `sync_state`, `sync_logs` | — | Not synced | — |
| **Derived** (rebuildable, not synced) | `products_fts`, `products_fts_si`, `stock_levels`, `batch_item_stock`, `daily_sales_summary`, `daily_payment_summary`, `customer_stats`, `quick_sale_stats` | — | Maintained by triggers on local seals *and* pulled inserts | — |

**Trigger classes (§13.5).**
- **Structural** triggers are always on: append-only tables, the order state machine, cost locks, and no reopening of a cash session.
- **Policy** triggers check local writes: seal totals, open session, permission bits and the audit chain. They're skipped while `sync_ctx.applying = 1`, which the pull sets inside its own page transaction (§6.2). Pulled rows were validated on their origin device and on the server, and parent closure can deliver them in an order those rules don't expect: for example, a closed session before another counter's void.
- **Derived** triggers are always on.

- **IDs:** UUIDv7 (TEXT locally, `uuid` in Postgres), except `quick_sale_links.id = UUIDv5(store_id, custom_key)`, `customers.id = UUIDv5(store_id, phone_e164)` and `store_settings.key`.
- **Reserved system device ID:** `00000000-0000-0000-0000-000000000000`. Any row the *server* modifies (barcode-conflict resolution, invariant repair) is stamped with it, so the originating device pulls the change back instead of filtering it out as its own.
- **Money:** `INTEGER` minor units (LKR cents; exponent 2 in `store_settings`). Every money column in v1 is ≥ 0, because there are no refunds or credit notes. Voids are a status, not negative rows. Reports simply `SUM` completed orders.
- **Timestamps:** ISO-8601 UTC TEXT locally, `timestamptz` in the cloud. `business_date` is TEXT `YYYY-MM-DD` (§1.7).
- **Push order** follows outbox `seq`. Parents are always enqueued before children in the same transaction: batch → batch_items → stock_movements, and order → items → payments → movements.

### 4.2 TypeScript (target shape of generated bindings)

```ts
// ───────── shared ─────────
export type UUID = string;            // UUIDv7 (UUIDv5 for quick_sale_links)
export type ISODateTime = string;     // UTC
export type BusinessDate = string;    // 'YYYY-MM-DD' in store timezone after day cutoff
export type Money = number;           // integer minor units — NEVER a float amount
export type Qty = number;             // integer units
export type Bp = number;              // basis points: 1500 = 15.00%

export interface SyncMeta {           // mutable (LWW) tables
  id: UUID;
  store_id: UUID;
  created_at: ISODateTime;
  updated_at: ISODateTime;
  updated_by_device: UUID;
  deleted_at: ISODateTime | null;
  version: number;
}
export interface InsertOnlyMeta {     // append-only tables
  id: UUID;
  store_id: UUID;
  device_id: UUID;                    // originating device
  created_at: ISODateTime;
}

// ───────── settings ─────────
export interface StoreSetting {       // synced, LWW per key
  key: string;                        // 'store.name' | 'store.currency' | 'store.timezone' | 'store.day_cutoff'
                                      // | 'store.week_start' | 'tax.inclusive' | 'receipt.header' | ...
  store_id: UUID;
  value: unknown;                     // JSON
  created_at: ISODateTime;
  updated_at: ISODateTime;
  updated_by_device: UUID;
  deleted_at: ISODateTime | null;
  version: number;
}
// device_settings (local-only): device_id, device_code, store_id, printers, idle_lock_minutes, ...

// ───────── RBAC ─────────
export const Perm = {
  SALE_CREATE: 2 ** 0, QUICK_SALE: 2 ** 1, PRICE_OVERRIDE: 2 ** 2, LINE_DISCOUNT: 2 ** 3,
  ORDER_DISCOUNT: 2 ** 4, VOID_ORDER: 2 ** 5, /* 2 ** 6, 2 ** 7 reserved: Phase 5 exchanges */
  DRAWER_NO_SALE: 2 ** 8, CASH_PAY_IN_OUT: 2 ** 9, CASH_SESSION_MANAGE: 2 ** 10,
  CASH_SESSION_FORCE_CLOSE: 2 ** 11, REPRINT_RECEIPT: 2 ** 12, STOCK_ADJUST: 2 ** 13,
  STOCK_DAMAGE: 2 ** 14, BATCH_MANAGE: 2 ** 15, PRODUCT_MANAGE: 2 ** 16, QUICK_SALE_PROMOTE: 2 ** 17,
  LABEL_PRINT: 2 ** 18, VIEW_COST: 2 ** 19, REPORT_SALES: 2 ** 20, REPORT_FINANCIAL: 2 ** 21,
  EXPENSE_MANAGE: 2 ** 22, SETTINGS_MANAGE: 2 ** 23, USER_MANAGE: 2 ** 24, AUDIT_VIEW: 2 ** 25,
  SYNC_MANAGE: 2 ** 26, BACKUP_RESTORE: 2 ** 27, DATA_EXPORT: 2 ** 28,
  CUSTOMER_VIEW: 2 ** 29, BACKUP_CREATE: 2 ** 30,
} as const;                           // generated from Rust bitflags; use 2 ** n, never 1 << n (int32 overflow at 31)
export type PermKey = keyof typeof Perm;
export type PermMask = number;

export interface Role extends SyncMeta {
  name: string;                       // 'Cashier' | 'Supervisor' | 'Manager' | 'Owner' | custom
  permissions: PermMask;
  max_discount_bp: Bp;                // above this ⇒ approval required
  max_override_bp: Bp;                // max price reduction below list without approval
  is_system: boolean;                 // Owner role: permissions locked to ALL
}

export interface Staff extends SyncMeta {
  display_name: string;
  role_id: UUID;
  pin_hash: string;                   // argon2id PHC string (never sent to the UI — Rust-only field)
  grant_mask: PermMask;
  deny_mask: PermMask;
  max_discount_bp: Bp | null;         // per-user override of role limit
  ui_language: UiLanguage;            // applied at login (§1.8)
  is_active: boolean;
}
export type UiLanguage = 'en' | 'si';
export interface SessionInfo {        // what the UI receives (no hash)
  staff_id: UUID; display_name: string; role_name: string; ui_language: UiLanguage;
  permissions: PermKey[];             // list, not a mask: the UI never does bit arithmetic
  max_discount_bp: Bp; max_override_bp: Bp;
  cash_session_id: UUID | null;
}

// ───────── catalog ─────────
export interface Category extends SyncMeta {
  name: string;
  parent_id: UUID | null;
}

export type ProductKind = 'standard' | 'generic';   // generic = "Plasticware – any item Rs 250"

export interface Product extends SyncMeta {
  kind: ProductKind;
  name: string;                       // printable (receipt/label code page) — English/Latin
  name_si: string | null;             // optional Sinhala display name; screens only, never printed
  sku: string | null;
  barcode: string | null;             // EAN-13/UPC or in-store prefix "2"
  category_id: UUID | null;
  default_cost: Money;                // fallback cost for ADJUSTMENTS layers; real cost lives on batch_items
  price: Money;
  min_price: Money | null;            // floor; below it always requires approval
  track_stock: boolean;
  allow_negative_stock: boolean;      // default true — offline terminals must never block a sale
  tax_rate_bp: Bp;
  attributes: Record<string, string>; // { size: "42", colour: "black" } — `size` prints first on labels
  is_active: boolean;
  barcode_conflict: boolean;          // set by sync when another device registered the same barcode
  stock_on_hand?: Qty;                // derived (stock_levels), not stored on the row
}

// ───────── batches (cost layers) ─────────
export type BatchStatus = 'draft' | 'received' | 'closed';
export type CostAllocation = 'manual' | 'even_per_unit' | 'by_retail_value';

export interface Batch extends SyncMeta {
  code: string;                       // "B-2026-045" (on labels & reports); system: OPENING-STOCK, ADJUSTMENTS
  name: string;                       // "Shoe batch – Pettah – 8 Nov"
  supplier_name: string | null;
  supplier_ref: string | null;        // supplier invoice no.
  purchased_at: ISODateTime;
  received_at: ISODateTime | null;    // set on receive; drives FIFO order
  received_business_date: BusinessDate | null;
  total_cost: Money;
  extra_costs: Money;                 // transport, handling — part of landed cost
  allocation_method: CostAllocation;
  allocation_variance: Money;         // landed − Σ(unit_cost × qty_received); fixed at receive
  status: BatchStatus;
  is_system: boolean;
  received_by: UUID | null;           // staff_id
  notes: string | null;
}

export interface BatchItem extends SyncMeta {
  batch_id: UUID;
  product_id: UUID;
  qty_received: Qty;
  unit_cost: Money;                   // allocated; locked once batch is received
  retail_price_at_intake: Money;      // used by by_retail_value allocation + report
  manual_unit_cost: Money | null;     // input for 'manual' allocation (draft only)
  added_after_receipt: boolean;       // late item ⇒ unit_cost = 0
  note: string | null;
}

export interface BatchItemStock {     // derived, local, trigger-maintained
  batch_item_id: UUID;
  batch_id: UUID;
  product_id: UUID;
  received_qty: Qty;
  on_hand: Qty;
  sold_qty: Qty;                      // net of voids
  sold_revenue: Money;                // Σ revenue_net (ex tax)
  sold_cost: Money;                   // COGS
  written_off_qty: Qty;               // damage + negative counts − positive counts
  written_off_cost: Money;
}

// ───────── stock ledger (append-only) ─────────
export type StockReason =
  | 'batch_intake' | 'batch_reopen'   // batch_reopen reverses intake
  | 'sale' | 'void'                   // 'void' exactly reverses a sale slice (same session only)
  | 'damage' | 'count_correction' | 'adjustment';

export interface StockMovement extends InsertOnlyMeta {
  product_id: UUID;
  batch_item_id: UUID;                // every movement belongs to a cost layer
  qty_delta: Qty;                     // + in, − out
  reason: StockReason;
  ref_type: 'order' | 'batch' | 'adjustment' | null;
  ref_id: UUID | null;
  order_item_id: UUID | null;         // sale/void slices → line (void reverses the same layers)
  unit_cost: Money;                   // layer cost at the time
  revenue_net: Money | null;          // sale/void slices only: ex-tax, after discounts; sign opposite to qty_delta
  business_date: BusinessDate;
  staff_id: UUID | null;
  approved_by: UUID | null;
  note: string | null;
}

// ───────── cash ─────────
export type CashSessionStatus = 'open' | 'closed';

export interface CashSession extends SyncMeta {
  device_id: UUID;
  business_date: BusinessDate;        // of opening
  status: CashSessionStatus;
  opened_by: UUID;
  opened_at: ISODateTime;
  opening_float: Money;
  closed_by: UUID | null;
  closed_at: ISODateTime | null;
  expected_cash: Money | null;
  counted_cash: Money | null;
  variance: Money | null;             // counted − expected
  denominations: Record<string, number> | null; // { "5000": 3, "1000": 12, ... }
  close_note: string | null;
}

export type CashMovementType = 'pay_in' | 'pay_out' | 'no_sale';

export interface CashMovement extends InsertOnlyMeta {
  cash_session_id: UUID;
  type: CashMovementType;
  amount: Money;                      // 0 for no_sale; positive for pay_in/pay_out
  reason: string;
  staff_id: UUID;
  approved_by: UUID | null;
  business_date: BusinessDate;
}

// ───────── customers (optional, phone-keyed) ─────────
export interface Customer extends SyncMeta {   // id = UUIDv5(store_id, phone_e164)
  phone_e164: string | null;          // "+94771234567"; null after erasure
  display_name: string | null;        // optional, typed at the till or in back office
  note: string | null;
  first_seen_at: ISODateTime;
  erased_at: ISODateTime | null;      // personal data cleared on request (PDPA)
}

export interface CustomerLookup {     // what the till sees (no history without CUSTOMER_VIEW)
  customer_id: UUID | null;           // null ⇒ new number, created at checkout
  phone_e164: string;
  display_name: string | null;
  visit_count: number;
}

export interface CustomerStats {      // derived table customer_stats (trigger-maintained; completed orders only)
  customer_id: UUID;
  phone_e164: string | null;
  display_name: string | null;
  visit_count: number;
  total_spend: Money;                 // Σ grand_total
  total_net: Money;                   // Σ net_sales (ex tax)
  avg_basket: Money;
  first_visit_date: BusinessDate | null;
  last_visit_date: BusinessDate | null;
}

// ───────── orders (all sales final; void is the only correction) ─────────
export type OrderType = 'sale';                     // Phase 5 may add 'exchange'
export type OrderStatus = 'completed' | 'voided';   // + 'pending' inside the checkout transaction only (never committed/visible)
export type PaymentMethod = 'cash' | 'card' | 'bank_transfer' | 'qr' | 'other';
export type VoidReason =
  | 'WRONG_ITEM' | 'WRONG_PRICE' | 'WRONG_QTY' | 'DUPLICATE_SCAN' | 'CUSTOMER_CANCELLED' | 'OTHER';

export interface Order extends InsertOnlyMeta {
  updated_at: ISODateTime;            // void transition only
  updated_by_device: UUID;
  version: number;
  cash_session_id: UUID;
  receipt_no: string;                 // "T01-20261108-0042"
  checkout_token: UUID;               // client-generated per cart; UNIQUE ⇒ double-tap / retry can't duplicate
  type: OrderType;
  line_count: number;                 // number of order_items (verified at seal)
  status: OrderStatus;
  staff_id: UUID;
  customer_id: UUID | null;           // optional; phone lives only on customers
  gross_sales: Money;                 // Σ line_gross (shelf price, tax-incl. when tax.inclusive)
  discount_total: Money;              // Σ line_discount + bill discount
  net_sales: Money;                   // Σ line_net (ex tax)
  tax_total: Money;
  rounding: Money;
  grand_total: Money;                 // Σ line_total + rounding (+ tax when exclusive)
  approved_by: UUID | null;           // bill-discount / override approver
  paid_total: Money;
  change_due: Money;
  cost_total: Money;                  // Σ line cost_total (COGS)
  quick_sale_net: Money;              // portion of net_sales from custom lines
  unknown_cost_net: Money;            // portion of net_sales with cost_known = false
  item_count: number;                 // Σ qty
  business_date: BusinessDate;
  completed_at: ISODateTime;
  voided_at: ISODateTime | null;
  voided_by: UUID | null;
  void_approved_by: UUID | null;      // supervisor who approved a cashier's void (must differ from voided_by)
  void_reason: VoidReason | null;
  void_note: string | null;           // required when void_reason = 'OTHER'
  note: string | null;
}

export interface OrderItem extends InsertOnlyMeta {
  order_id: UUID;
  line_no: number;
  product_id: UUID | null;            // NULL ⇒ quick sale
  is_custom: boolean;
  custom_key: string | null;          // normalised quick-sale name (grouping key)
  batch_id: UUID | null;              // reserved; quick sales are never tagged at the till (owner rule)
  name: string;                       // snapshot / typed name (printable on receipts)
  barcode: string | null;
  list_price: Money;                  // catalog price at time (= unit_price for custom)
  unit_price: Money;                  // price charged (tax-inclusive when tax.inclusive)
  qty: Qty;                           // > 0
  line_gross: Money;                  // unit_price × qty
  line_discount: Money;               // item-level discount
  order_discount_share: Money;        // pro-rata share of bill discount (largest remainder)
  tax_rate_bp: Bp;
  tax_amount: Money;
  line_total: Money;                  // line_gross − line_discount − order_discount_share (customer pays)
  line_net: Money;                    // ex tax
  cost_total: Money;                  // Σ slice costs (catalog) | entered cost × qty | 0
  cost_known: boolean;
  price_overridden: boolean;
  override_reason: string | null;
  approved_by: UUID | null;           // supervisor for override/discount
}

export interface OrderPayment extends InsertOnlyMeta {
  order_id: UUID;
  method: PaymentMethod;
  amount: Money;                      // APPLIED amount, always ≥ 0 (no refunds)
  tendered: Money | null;             // cash handed over (change = tendered − amount)
  reference: string | null;           // card slip / transfer ref
}

// ───────── expenses ─────────
export interface ExpenseCategory extends SyncMeta {
  name: string;                       // Rent, Electricity, Wages, Transport (non-batch), Tea/Sundries…
  is_active: boolean;
}

export interface Expense extends SyncMeta {
  category_id: UUID;
  amount: Money;
  business_date: BusinessDate;        // expense date
  payment_method: PaymentMethod;
  paid_from_drawer: boolean;          // ⇒ also a cash_movements pay_out in the open session
  cash_session_id: UUID | null;
  payee: string | null;
  note: string | null;
  recorded_by: UUID;
}

// ───────── quick sale promotion ─────────
export interface QuickSaleLink extends SyncMeta {   // id = UUIDv5(store_id, custom_key)
  custom_key: string;
  display_name: string;
  action: 'promoted' | 'ignored';
  product_id: UUID | null;
  batch_id: UUID | null;
  decided_by: UUID;
  decided_at: ISODateTime;
}

export interface QuickSaleSummary {   // derived table quick_sale_stats ⋈ quick_sale_links
  custom_key: string;
  display_name: string;               // latest spelling
  times_sold: number;
  units_sold: Qty;
  revenue_net: Money;
  last_price: Money;
  min_price: Money;
  max_price: Money;
  first_sold_at: ISODateTime;
  last_sold_at: ISODateTime;
  status: 'new' | 'promoted' | 'ignored';
  product_id: UUID | null;
}

// ───────── audit ─────────
export type AuditAction =
  | 'STORE_INITIALIZED'
  | 'AUTH_LOGIN' | 'AUTH_LOGOUT' | 'AUTH_FAILED' | 'AUTH_LOCKOUT' | 'APPROVAL_GRANTED' | 'APPROVAL_DENIED'
  | 'CASH_SESSION_OPEN' | 'CASH_SESSION_CLOSE' | 'CASH_SESSION_FORCE_CLOSE'
  | 'DRAWER_KICK' | 'CASH_PAY_IN' | 'CASH_PAY_OUT'
  | 'PRICE_OVERRIDE' | 'LINE_DISCOUNT' | 'ORDER_DISCOUNT'
  | 'VOID_ORDER' | 'REPRINT_RECEIPT' | 'PRINT_JOB_ABANDON'
  | 'CUSTOMER_CREATE' | 'CUSTOMER_UPDATE' | 'CUSTOMER_ERASE'
  | 'LABEL_PRINT'
  | 'STOCK_ADJUSTMENT' | 'STOCK_DAMAGE' | 'STOCK_COUNT'
  | 'BATCH_CREATE' | 'BATCH_RECEIVE' | 'BATCH_REOPEN' | 'BATCH_CLOSE' | 'BATCH_LATE_ITEM'
  | 'PRODUCT_CREATE' | 'PRODUCT_UPDATE' | 'PRODUCT_DELETE' | 'BARCODE_CONFLICT_RESOLVE'
  | 'QUICK_SALE_PROMOTE' | 'QUICK_SALE_IGNORE'
  | 'EXPENSE_CREATE' | 'EXPENSE_UPDATE' | 'EXPENSE_DELETE'
  | 'SETTINGS_CHANGE' | 'ROLE_CREATE' | 'ROLE_UPDATE'
  | 'USER_CREATE' | 'USER_UPDATE' | 'USER_DISABLE' | 'USER_PIN_RESET'
  | 'BACKUP_CREATE' | 'BACKUP_FAILED' | 'BACKUP_RESTORE' | 'DATA_EXPORT' | 'SYNC_DEAD_OP_RESOLVE' | 'DEVICE_REGISTER';

export interface AuditLog extends InsertOnlyMeta {
  device_seq: number;                 // gapless per device
  staff_id: UUID | null;              // null for system events
  approved_by: UUID | null;
  action: AuditAction;
  entity_type: string | null;         // 'order' | 'product' | 'batch' | 'staff' | 'setting' | ...
  entity_id: string | null;
  metadata: Record<string, unknown>;  // { before, after, reason, amount, receipt_no, ... }
  business_date: BusinessDate;
  prev_hash: string;                  // hex SHA-256
  hash: string;
}

// ───────── reporting read models ─────────
export type ReportPeriod =
  | { kind: 'day'; date: BusinessDate }
  | { kind: 'week'; anyDate: BusinessDate }        // week_start from store settings
  | { kind: 'month'; year: number; month: number }
  | { kind: 'year'; year: number }
  | { kind: 'all_time' }
  | { kind: 'custom'; from: BusinessDate; to: BusinessDate };   // inclusive

export interface FinancialSummary {
  period: { from: BusinessDate | null; to: BusinessDate | null };
  device_id: UUID | null;             // null = whole store
  sales_count: number; void_count: number; void_value: Money;
  gross_sales: Money; discounts: Money;
  net_sales: Money;                                   // = Total Revenue (ex tax), completed orders only
  tax_collected: Money;
  customer_attached_bp: Bp;                           // share of sales with a customer phone
  cogs: Money;                                        // incl. batch allocation variance
  gross_profit: Money; gross_margin_bp: Bp;
  stock_write_offs: Money;
  operating_expenses: Money;
  net_profit: Money;
  quick_sale_net: Money; unknown_cost_net: Money;
  payments: { method: PaymentMethod; amount: Money; count: number }[];
}

export interface StockValuation {
  as_of: BusinessDate | 'now';
  total_units: Qty;
  value_at_cost: Money;
  value_at_retail: Money;
  by_batch: { batch_id: UUID; code: string; units: Qty; value_at_cost: Money; value_at_retail: Money }[];
  by_category: { category_id: UUID | null; units: Qty; value_at_cost: Money; value_at_retail: Money }[];
  negative_layers: number;            // oversold layers needing attention
}

export interface BatchReport {
  batch_id: UUID; code: string; name: string; status: BatchStatus;
  purchased_at: ISODateTime; received_at: ISODateTime | null;
  landed_cost: Money; allocation_variance: Money;
  units_received: Qty; units_sold: Qty; units_written_off: Qty; units_remaining: Qty;
  revenue_net: Money;                 // stock sales + tagged quick sales
  cogs: Money; written_off_cost: Money;
  realised_profit: Money;             // revenue − cogs − write-offs − variance
  payback: Money;                     // revenue − landed_cost
  remaining_value_cost: Money; remaining_value_retail: Money;
  projected_profit: Money;            // payback + remaining_value_retail
  sell_through_bp: Bp;
  items: (BatchItemStock & { name: string; unit_cost: Money; price: Money })[];
}

// ───────── labels (device-local templates; §7.4) ─────────
export type LabelLanguage = 'tspl' | 'zpl';
export type BarcodeSymbology = 'code128' | 'ean13';
export type LabelElementKind = 'store_name' | 'product_name' | 'attributes' | 'price' | 'barcode';

export interface LabelElement {       // positions in mm, relative to ONE label's top-left
  kind: LabelElementKind;
  x_mm: number; y_mm: number; w_mm: number; h_mm: number;
  font_size: 1 | 2 | 3 | 4;           // printer built-in font scale step
  bold?: boolean;
  max_lines?: 1 | 2;                  // product_name wrapping
  align?: 'left' | 'center' | 'right';
}

export interface LabelTemplate {
  id: string;                         // 'two_up_2x1' | 'three_up_1x1' | custom
  name: string;
  columns: 2 | 3;                     // labels per row on the roll
  label_w_mm: number; label_h_mm: number;     // 50.8×25.4 (2″×1″) or 25.4×25.4 (1″×1″)
  col_gap_mm: number;                 // horizontal gap between labels in a row
  row_gap_mm: number;                 // vertical gap (die-cut gap) between rows
  left_margin_mm: number;             // liner edge → first label
  dpi: 203 | 300;
  language: LabelLanguage;
  symbology: BarcodeSymbology | 'auto';       // auto: EAN-13 if a valid 13-digit code, else Code128
  elements: LabelElement[];
}

export interface LabelJobItem { product_id: UUID; copies: number; }

export interface LabelPreview {       // Rust-computed layout the UI renders as SVG (WYSIWYG)
  rows: number;
  roll_width_mm: number;
  labels: { row: number; col: number; product_id: UUID; elements: (LabelElement & { text: string })[] }[];
  warnings: string[];                 // e.g. "barcode module < 2 dots — use a shorter code or a 300 dpi printer"
}

// ───────── backup ─────────
export interface BackupResult {
  path: string;                       // E:\GayanShop-T01-20261108-153012.db
  size_bytes: number;
  sha256: string;
  integrity_ok: boolean;
  schema_version: number;
  duration_ms: number;
}

// ───────── sync ─────────
export type SyncOp = 'upsert' | 'delete';
export type OutboxStatus = 'pending' | 'in_flight' | 'failed' | 'dead';

export interface SyncOutboxEntry {    // local-only
  seq: number;
  op_id: UUID;                        // idempotency key
  table_name: string;
  row_id: string;                     // UUID, or key for store_settings
  op: SyncOp;
  payload: unknown;
  status: OutboxStatus;
  attempts: number;
  last_error: string | null;
  next_attempt_at: ISODateTime;
  created_at: ISODateTime;
}

export type SyncDirection = 'push' | 'pull';
export type SyncResult = 'success' | 'partial' | 'error';

export interface SyncLog {            // local-only
  id: UUID;
  device_id: UUID;
  direction: SyncDirection;
  started_at: ISODateTime;
  finished_at: ISODateTime | null;
  result: SyncResult;
  rows_sent: number;
  rows_received: number;
  rows_rejected: number;
  cursor_before: number | null;
  cursor_after: number | null;
  error_code: string | null;          // 'NETWORK' | 'AUTH' | 'CONFLICT' | 'SERVER' | 'VALIDATION' | 'INVARIANT'
  error_message: string | null;
  details: unknown | null;
}
```

### 4.3 SQLite DDL (`src-tauri/migrations/0001_init.sql`)

```sql
-- GayanShop POS — initial schema, v4.1 (all sales final; void-only corrections; customers; stress-test hardened).
-- Conventions: money = INTEGER minor units (LKR cents); ids = UUID TEXT; timestamps = ISO-8601 UTC TEXT;
-- business_date = 'YYYY-MM-DD' in store timezone after day cutoff, computed by the writing device.
--
-- Trigger classes (§13.5):
--   STRUCTURAL  – always on: append-only tables, order state machine, cost locks. Hold for every writer.
--   POLICY      – business rules for LOCAL writes (seal totals, open session, permissions, audit chain).
--                 Skipped while sync_ctx.applying = 1, because pulled rows were validated on their origin
--                 device and on the server, and may legitimately arrive in an order these rules don't expect.
--   DERIVED     – keep caches/rollups in step. Always on (local and pulled rows alike).
-- SQL cannot verify a PIN: permission checks here are structural (the approver exists, is active and holds
-- the bit). The PIN and approval-token checks happen in Rust (auth::guard).

-- ═════════ sync apply flag (local-only; a regular table because non-TEMP triggers can't read TEMP tables) ═════════
CREATE TABLE sync_ctx (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  applying INTEGER NOT NULL DEFAULT 0 CHECK (applying IN (0, 1))
);
INSERT INTO sync_ctx (id, applying) VALUES (1, 0);
-- The pull sets applying = 1 at the start of its page transaction and back to 0 before COMMIT.
-- Writes are serialised (single writer), so no local command can run while it is 1.

-- ═════════ settings ═════════
CREATE TABLE store_settings (                        -- synced, LWW per key
  key TEXT PRIMARY KEY,
  store_id TEXT NOT NULL,
  value TEXT NOT NULL,                               -- JSON
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE device_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- local-only (JSON values)

-- ═════════ RBAC ═════════
CREATE TABLE roles (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL,
  name TEXT NOT NULL,
  permissions INTEGER NOT NULL DEFAULT 0,
  max_discount_bp INTEGER NOT NULL DEFAULT 0 CHECK (max_discount_bp BETWEEN 0 AND 10000),
  max_override_bp INTEGER NOT NULL DEFAULT 0 CHECK (max_override_bp BETWEEN 0 AND 10000),
  is_system INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX ux_roles_name ON roles(store_id, name) WHERE deleted_at IS NULL;

CREATE TABLE staff (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  role_id TEXT NOT NULL REFERENCES roles(id),
  pin_hash TEXT NOT NULL,
  grant_mask INTEGER NOT NULL DEFAULT 0,
  deny_mask INTEGER NOT NULL DEFAULT 0,
  max_discount_bp INTEGER CHECK (max_discount_bp IS NULL OR max_discount_bp BETWEEN 0 AND 10000),
  ui_language TEXT NOT NULL DEFAULT 'en' CHECK (ui_language IN ('en','si')),
  is_active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX ix_staff_role ON staff(role_id);

-- Effective permissions = (role | grant) & ~deny; the Owner system role holds every bit (2^53 − 1).
-- Bits used by policy triggers: PRICE_OVERRIDE = 4 (bit 2), VOID_ORDER = 32 (bit 5).
CREATE VIEW v_staff_effective AS
SELECT s.id AS staff_id, s.is_active,
       CASE WHEN r.is_system = 1 AND r.name = 'Owner' THEN 9007199254740991
            ELSE ((r.permissions | s.grant_mask) & ~s.deny_mask) END AS perms,
       COALESCE(s.max_discount_bp, r.max_discount_bp) AS max_discount_bp,
       r.max_override_bp
FROM staff s JOIN roles r ON r.id = s.role_id
WHERE s.deleted_at IS NULL;

CREATE TABLE auth_attempts (                         -- local-only lockout state
  staff_id TEXT PRIMARY KEY,
  failed_count INTEGER NOT NULL DEFAULT 0,
  locked_until TEXT,
  last_failed_at TEXT
);

-- ═════════ catalog ═════════
CREATE TABLE categories (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL,
  name TEXT NOT NULL, parent_id TEXT REFERENCES categories(id),
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE products (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'standard' CHECK (kind IN ('standard','generic')),
  name TEXT NOT NULL,                                -- printable on receipts/labels (Latin)
  name_si TEXT,                                      -- optional Sinhala display name (screens only)
  sku TEXT, barcode TEXT,                            -- sku: 6-digit in-store code {device_digit}{5-digit seq}
  category_id TEXT REFERENCES categories(id),
  default_cost INTEGER NOT NULL DEFAULT 0 CHECK (default_cost >= 0),
  price INTEGER NOT NULL CHECK (price >= 0),
  min_price INTEGER,
  track_stock INTEGER NOT NULL DEFAULT 1,
  allow_negative_stock INTEGER NOT NULL DEFAULT 1,
  tax_rate_bp INTEGER NOT NULL DEFAULT 0,
  attributes TEXT NOT NULL DEFAULT '{}',
  is_active INTEGER NOT NULL DEFAULT 1,
  barcode_conflict INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);
-- Uniqueness among live products only; sync conflict handling (§6.3) keeps both sides valid.
CREATE UNIQUE INDEX ux_products_barcode ON products(store_id, barcode)
  WHERE barcode IS NOT NULL AND deleted_at IS NULL AND is_active = 1;
CREATE UNIQUE INDEX ux_products_sku ON products(store_id, sku)
  WHERE sku IS NOT NULL AND deleted_at IS NULL;

-- Type-ahead search. Standalone FTS tables keyed by id (NOT external-content on rowid: TEXT PK ⇒
-- VACUUM/restore may renumber rowids). Latin fields: unicode61 + prefix indexes for "blu* buc*".
-- Sinhala names: trigram tokenizer (substring match, script-agnostic) because unicode61's handling of
-- Sinhala combining vowel signs is unverified — Block 1.1 tests real Sinhala names against both.
CREATE VIRTUAL TABLE products_fts USING fts5(id UNINDEXED, name, sku, barcode, tokenize = 'unicode61', prefix = '2 3');
CREATE VIRTUAL TABLE products_fts_si USING fts5(id UNINDEXED, name_si, tokenize = 'trigram');
CREATE TRIGGER trg_products_fts_ins AFTER INSERT ON products BEGIN
  INSERT INTO products_fts(id, name, sku, barcode) VALUES (NEW.id, NEW.name, NEW.sku, NEW.barcode);
  INSERT INTO products_fts_si(id, name_si) SELECT NEW.id, NEW.name_si WHERE NEW.name_si IS NOT NULL;
END;
CREATE TRIGGER trg_products_fts_upd AFTER UPDATE OF name, name_si, sku, barcode ON products BEGIN
  DELETE FROM products_fts WHERE id = OLD.id;
  DELETE FROM products_fts_si WHERE id = OLD.id;
  INSERT INTO products_fts(id, name, sku, barcode) VALUES (NEW.id, NEW.name, NEW.sku, NEW.barcode);
  INSERT INTO products_fts_si(id, name_si) SELECT NEW.id, NEW.name_si WHERE NEW.name_si IS NOT NULL;
END;
CREATE TRIGGER trg_products_fts_del AFTER DELETE ON products BEGIN
  DELETE FROM products_fts WHERE id = OLD.id;
  DELETE FROM products_fts_si WHERE id = OLD.id;
END;

-- ═════════ batches (cost layers) ═════════
CREATE TABLE batches (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL,
  code TEXT NOT NULL, name TEXT NOT NULL,
  supplier_name TEXT, supplier_ref TEXT,
  purchased_at TEXT NOT NULL,
  received_at TEXT, received_business_date TEXT,
  total_cost INTEGER NOT NULL DEFAULT 0 CHECK (total_cost >= 0),
  extra_costs INTEGER NOT NULL DEFAULT 0 CHECK (extra_costs >= 0),
  allocation_method TEXT NOT NULL CHECK (allocation_method IN ('manual','even_per_unit','by_retail_value')),
  allocation_variance INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','received','closed')),
  is_system INTEGER NOT NULL DEFAULT 0,
  received_by TEXT REFERENCES staff(id),
  notes TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1,
  CHECK (status = 'draft' OR (received_at IS NOT NULL AND received_business_date IS NOT NULL))
);
CREATE UNIQUE INDEX ux_batches_code ON batches(store_id, code) WHERE deleted_at IS NULL;
CREATE INDEX ix_batches_received ON batches(received_at);

CREATE TABLE batch_items (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL,
  batch_id TEXT NOT NULL REFERENCES batches(id),
  product_id TEXT NOT NULL REFERENCES products(id),
  qty_received INTEGER NOT NULL CHECK (qty_received >= 0),
  unit_cost INTEGER NOT NULL DEFAULT 0 CHECK (unit_cost >= 0),
  retail_price_at_intake INTEGER NOT NULL DEFAULT 0,
  manual_unit_cost INTEGER,
  added_after_receipt INTEGER NOT NULL DEFAULT 0,
  note TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1,
  UNIQUE (batch_id, product_id)
);
CREATE INDEX ix_batch_items_product ON batch_items(product_id);
-- STRUCTURAL: unit_cost / qty_received may not change once the parent batch is received or closed.
CREATE TRIGGER trg_batch_item_cost_lock BEFORE UPDATE OF unit_cost, qty_received ON batch_items
WHEN (SELECT status FROM batches WHERE id = OLD.batch_id) <> 'draft'
  AND (NEW.unit_cost <> OLD.unit_cost OR NEW.qty_received <> OLD.qty_received)
BEGIN SELECT RAISE(ABORT, 'batch item cost/qty locked after receive'); END;

-- ═════════ cash ═════════
CREATE TABLE cash_sessions (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  business_date TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','closed')),
  opened_by TEXT NOT NULL REFERENCES staff(id), opened_at TEXT NOT NULL,
  opening_float INTEGER NOT NULL DEFAULT 0 CHECK (opening_float >= 0),
  closed_by TEXT REFERENCES staff(id), closed_at TEXT,
  expected_cash INTEGER, counted_cash INTEGER, variance INTEGER,
  denominations TEXT, close_note TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX ux_one_open_session ON cash_sessions(device_id) WHERE status = 'open';
-- STRUCTURAL: a closed session never reopens (it would re-open the void window on a printed Z-report).
CREATE TRIGGER trg_cash_session_no_reopen BEFORE UPDATE OF status ON cash_sessions
WHEN OLD.status = 'closed' AND NEW.status <> 'closed'
BEGIN SELECT RAISE(ABORT, 'a closed cash session cannot be reopened'); END;

CREATE TABLE cash_movements (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  cash_session_id TEXT NOT NULL REFERENCES cash_sessions(id),
  type TEXT NOT NULL CHECK (type IN ('pay_in','pay_out','no_sale')),
  amount INTEGER NOT NULL CHECK (amount >= 0),
  reason TEXT NOT NULL,
  staff_id TEXT NOT NULL, approved_by TEXT,
  business_date TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX ix_cashmov_session ON cash_movements(cash_session_id);
CREATE TRIGGER trg_cashmov_no_update BEFORE UPDATE ON cash_movements
BEGIN SELECT RAISE(ABORT, 'cash_movements are append-only'); END;
CREATE TRIGGER trg_cashmov_no_delete BEFORE DELETE ON cash_movements
BEGIN SELECT RAISE(ABORT, 'cash_movements are append-only'); END;

-- ═════════ customers (optional; id = UUIDv5(store_id, phone_e164)) ═════════
CREATE TABLE customers (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL,
  phone_e164 TEXT CHECK (phone_e164 IS NULL OR phone_e164 GLOB '+94[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]'),
  display_name TEXT, note TEXT,
  first_seen_at TEXT NOT NULL,
  erased_at TEXT,                                    -- PDPA erasure: phone/name/note cleared
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1,
  CHECK (erased_at IS NULL OR (phone_e164 IS NULL AND display_name IS NULL AND note IS NULL))
);
CREATE UNIQUE INDEX ux_customers_phone ON customers(store_id, phone_e164)
  WHERE phone_e164 IS NOT NULL AND deleted_at IS NULL;

-- ═════════ orders: pending → completed (seal) → voided (same session only) ═════════
-- Local checkout inserts the order as 'pending', inserts lines/payments/stock slices, then SEALS it
-- (UPDATE status = 'completed'). The seal trigger re-verifies every total in SQL. 'pending' exists only
-- inside the checkout transaction: it is never committed, never synced, never shown.
CREATE TABLE orders (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  cash_session_id TEXT NOT NULL REFERENCES cash_sessions(id),
  receipt_no TEXT NOT NULL,
  checkout_token TEXT NOT NULL,                      -- client-generated per cart; dedupes double-taps/retries
  type TEXT NOT NULL DEFAULT 'sale' CHECK (type IN ('sale')),   -- Phase 5 may add 'exchange'
  status TEXT NOT NULL CHECK (status IN ('pending','completed','voided')),
  staff_id TEXT NOT NULL REFERENCES staff(id),
  approved_by TEXT REFERENCES staff(id),             -- bill-discount / override approver
  customer_id TEXT REFERENCES customers(id),         -- optional; the phone lives only on customers
  line_count INTEGER NOT NULL CHECK (line_count > 0),
  item_count INTEGER NOT NULL CHECK (item_count > 0),-- Σ qty
  gross_sales INTEGER NOT NULL CHECK (gross_sales >= 0),
  discount_total INTEGER NOT NULL DEFAULT 0 CHECK (discount_total >= 0),
  net_sales INTEGER NOT NULL CHECK (net_sales >= 0), -- Σ line_net (ex tax)
  tax_total INTEGER NOT NULL DEFAULT 0 CHECK (tax_total >= 0),
  rounding INTEGER NOT NULL DEFAULT 0,
  grand_total INTEGER NOT NULL CHECK (grand_total >= 0),
  paid_total INTEGER NOT NULL CHECK (paid_total >= 0),
  change_due INTEGER NOT NULL DEFAULT 0 CHECK (change_due >= 0),
  cost_total INTEGER NOT NULL DEFAULT 0,
  quick_sale_net INTEGER NOT NULL DEFAULT 0,
  unknown_cost_net INTEGER NOT NULL DEFAULT 0,
  business_date TEXT NOT NULL,
  completed_at TEXT NOT NULL,
  voided_at TEXT, voided_by TEXT REFERENCES staff(id), void_approved_by TEXT REFERENCES staff(id),
  void_reason TEXT CHECK (void_reason IN ('WRONG_ITEM','WRONG_PRICE','WRONG_QTY','DUPLICATE_SCAN','CUSTOMER_CANCELLED','OTHER')),
  void_note TEXT,
  note TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  CHECK ((status = 'voided') = (voided_at IS NOT NULL AND voided_by IS NOT NULL AND void_reason IS NOT NULL)),
  CHECK (void_reason IS NOT 'OTHER' OR length(trim(coalesce(void_note, ''))) > 0),
  CHECK (paid_total = grand_total)
);
CREATE UNIQUE INDEX ux_orders_receipt ON orders(store_id, receipt_no);
CREATE UNIQUE INDEX ux_orders_checkout_token ON orders(checkout_token);
CREATE INDEX ix_orders_bdate ON orders(business_date, device_id);
CREATE INDEX ix_orders_session ON orders(cash_session_id);
CREATE INDEX ix_orders_keyset ON orders(completed_at DESC, id DESC);           -- order history paging
CREATE INDEX ix_orders_customer ON orders(customer_id, business_date) WHERE customer_id IS NOT NULL;

-- STRUCTURAL: the only legal transitions are pending→completed and completed→voided, and neither may
-- change what was sold, to whom, by whom, for how much, or when.
CREATE TRIGGER trg_orders_state_machine BEFORE UPDATE ON orders
WHEN NOT (
      ((OLD.status = 'pending' AND NEW.status = 'completed') OR (OLD.status = 'completed' AND NEW.status = 'voided'))
  AND NEW.id = OLD.id AND NEW.device_id = OLD.device_id AND NEW.cash_session_id = OLD.cash_session_id
  AND NEW.receipt_no = OLD.receipt_no AND NEW.checkout_token = OLD.checkout_token
  AND NEW.staff_id = OLD.staff_id AND NEW.approved_by IS OLD.approved_by AND NEW.customer_id IS OLD.customer_id
  AND NEW.line_count = OLD.line_count AND NEW.item_count = OLD.item_count
  AND NEW.gross_sales = OLD.gross_sales AND NEW.discount_total = OLD.discount_total
  AND NEW.net_sales = OLD.net_sales AND NEW.tax_total = OLD.tax_total AND NEW.rounding = OLD.rounding
  AND NEW.grand_total = OLD.grand_total AND NEW.paid_total = OLD.paid_total AND NEW.change_due = OLD.change_due
  AND NEW.cost_total = OLD.cost_total AND NEW.quick_sale_net = OLD.quick_sale_net
  AND NEW.unknown_cost_net = OLD.unknown_cost_net
  AND NEW.business_date = OLD.business_date AND NEW.completed_at = OLD.completed_at
)
BEGIN SELECT RAISE(ABORT, 'orders: illegal state transition or mutation of a sold order'); END;
CREATE TRIGGER trg_orders_no_delete BEFORE DELETE ON orders
BEGIN SELECT RAISE(ABORT, 'orders cannot be deleted'); END;

-- POLICY: local orders are born 'pending' (completed/voided rows only arrive via sync).
CREATE TRIGGER trg_orders_insert_pending BEFORE INSERT ON orders
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0 AND NEW.status <> 'pending'
BEGIN SELECT RAISE(ABORT, 'orders: local inserts must start as pending and be sealed'); END;

CREATE TABLE order_items (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  order_id TEXT NOT NULL REFERENCES orders(id),
  line_no INTEGER NOT NULL,
  product_id TEXT REFERENCES products(id),
  is_custom INTEGER NOT NULL DEFAULT 0,
  custom_key TEXT,
  batch_id TEXT REFERENCES batches(id),
  name TEXT NOT NULL, barcode TEXT,
  list_price INTEGER NOT NULL CHECK (list_price >= 0), unit_price INTEGER NOT NULL CHECK (unit_price >= 0),
  qty INTEGER NOT NULL CHECK (qty > 0),
  line_gross INTEGER NOT NULL,                       -- unit_price × qty
  line_discount INTEGER NOT NULL DEFAULT 0,
  order_discount_share INTEGER NOT NULL DEFAULT 0,
  tax_rate_bp INTEGER NOT NULL DEFAULT 0,
  tax_amount INTEGER NOT NULL DEFAULT 0,
  line_total INTEGER NOT NULL,                       -- what the customer pays (incl. tax when inclusive)
  line_net INTEGER NOT NULL,                         -- ex tax
  cost_total INTEGER NOT NULL DEFAULT 0 CHECK (cost_total >= 0),
  cost_known INTEGER NOT NULL DEFAULT 1,
  price_overridden INTEGER NOT NULL DEFAULT 0, override_reason TEXT, approved_by TEXT REFERENCES staff(id),
  created_at TEXT NOT NULL,
  CHECK ((is_custom = 1 AND product_id IS NULL AND custom_key IS NOT NULL)
      OR (is_custom = 0 AND product_id IS NOT NULL)),
  CHECK (line_gross = unit_price * qty),
  CHECK (line_total = line_gross - line_discount - order_discount_share),
  CHECK (line_discount >= 0 AND order_discount_share >= 0 AND line_total >= 0 AND line_net >= 0),
  CHECK (price_overridden = (unit_price < list_price)),
  UNIQUE (order_id, line_no)
);
CREATE INDEX ix_items_product ON order_items(product_id);
CREATE INDEX ix_items_custom ON order_items(custom_key) WHERE is_custom = 1;
CREATE INDEX ix_items_batch ON order_items(batch_id) WHERE batch_id IS NOT NULL;
CREATE TRIGGER trg_items_no_update BEFORE UPDATE ON order_items
BEGIN SELECT RAISE(ABORT, 'order_items are immutable'); END;
CREATE TRIGGER trg_items_no_delete BEFORE DELETE ON order_items
BEGIN SELECT RAISE(ABORT, 'order_items are immutable'); END;
-- POLICY: lines can only be added to an order that is still pending (i.e. inside its own checkout).
CREATE TRIGGER trg_items_only_into_pending BEFORE INSERT ON order_items
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0
 AND (SELECT status FROM orders WHERE id = NEW.order_id) IS NOT 'pending'
BEGIN SELECT RAISE(ABORT, 'order_items: cannot add lines to a sealed order'); END;

CREATE TABLE order_payments (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  order_id TEXT NOT NULL REFERENCES orders(id),
  method TEXT NOT NULL CHECK (method IN ('cash','card','bank_transfer','qr','other')),
  amount INTEGER NOT NULL CHECK (amount >= 0),       -- all sales final: no negative (refund) payments
  tendered INTEGER,
  reference TEXT,
  created_at TEXT NOT NULL,
  CHECK (tendered IS NULL OR (method = 'cash' AND tendered >= amount))
);
CREATE INDEX ix_payments_order ON order_payments(order_id);
CREATE TRIGGER trg_payments_no_update BEFORE UPDATE ON order_payments
BEGIN SELECT RAISE(ABORT, 'order_payments are append-only'); END;
CREATE TRIGGER trg_payments_no_delete BEFORE DELETE ON order_payments
BEGIN SELECT RAISE(ABORT, 'order_payments are append-only'); END;
-- POLICY: payments only into a pending order.
CREATE TRIGGER trg_payments_only_into_pending BEFORE INSERT ON order_payments
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0
 AND (SELECT status FROM orders WHERE id = NEW.order_id) IS NOT 'pending'
BEGIN SELECT RAISE(ABORT, 'order_payments: cannot add payments to a sealed order'); END;

-- ═════════ stock ledger (append-only) ═════════
CREATE TABLE stock_movements (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  product_id TEXT NOT NULL REFERENCES products(id),
  batch_item_id TEXT NOT NULL REFERENCES batch_items(id),
  qty_delta INTEGER NOT NULL CHECK (qty_delta <> 0),
  reason TEXT NOT NULL CHECK (reason IN ('batch_intake','batch_reopen','sale','void',
                                         'damage','count_correction','adjustment')),
  ref_type TEXT, ref_id TEXT,
  order_item_id TEXT REFERENCES order_items(id),
  unit_cost INTEGER NOT NULL CHECK (unit_cost >= 0),
  revenue_net INTEGER,
  business_date TEXT NOT NULL,
  staff_id TEXT, approved_by TEXT, note TEXT,
  created_at TEXT NOT NULL,
  CHECK ((reason IN ('sale','void')) = (order_item_id IS NOT NULL)),
  CHECK (reason <> 'sale' OR (qty_delta < 0 AND revenue_net >= 0)),
  CHECK (reason <> 'void' OR (qty_delta > 0 AND revenue_net <= 0))
);
CREATE INDEX ix_mov_product ON stock_movements(product_id);
CREATE INDEX ix_mov_layer ON stock_movements(batch_item_id);
CREATE INDEX ix_mov_date_reason ON stock_movements(business_date, reason);
CREATE INDEX ix_mov_order_item ON stock_movements(order_item_id) WHERE order_item_id IS NOT NULL;
CREATE TRIGGER trg_mov_no_update BEFORE UPDATE ON stock_movements
BEGIN SELECT RAISE(ABORT, 'stock_movements are append-only'); END;
CREATE TRIGGER trg_mov_no_delete BEFORE DELETE ON stock_movements
BEGIN SELECT RAISE(ABORT, 'stock_movements are append-only'); END;
-- POLICY: sale slices only inside their pending checkout; void slices only for a voided order, and each
-- must mirror an existing sale slice (same line, layer, cost) without reversing more than was sold.
CREATE TRIGGER trg_mov_sale_void_guard BEFORE INSERT ON stock_movements
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0 AND NEW.reason IN ('sale','void') BEGIN
  SELECT RAISE(ABORT, 'stock: sale slices only inside a pending checkout')
  WHERE NEW.reason = 'sale'
    AND (SELECT o.status FROM order_items oi JOIN orders o ON o.id = oi.order_id WHERE oi.id = NEW.order_item_id) IS NOT 'pending';
  SELECT RAISE(ABORT, 'stock: void slices only for a voided order')
  WHERE NEW.reason = 'void'
    AND (SELECT o.status FROM order_items oi JOIN orders o ON o.id = oi.order_id WHERE oi.id = NEW.order_item_id) IS NOT 'voided';
  SELECT RAISE(ABORT, 'stock: void slice must mirror a sale slice and not exceed it')
  WHERE NEW.reason = 'void'
    AND (NOT EXISTS (SELECT 1 FROM stock_movements m WHERE m.order_item_id = NEW.order_item_id
                       AND m.batch_item_id = NEW.batch_item_id AND m.unit_cost = NEW.unit_cost AND m.reason = 'sale')
         OR COALESCE((SELECT SUM(qty_delta) FROM stock_movements m WHERE m.order_item_id = NEW.order_item_id
                       AND m.batch_item_id = NEW.batch_item_id AND m.reason IN ('sale','void')), 0) + NEW.qty_delta > 0);
END;

-- ═════════ order seal / void policy (after stock_movements exists, which they read) ═════════
-- POLICY: seal (pending → completed) re-verifies the whole checkout in SQL.
CREATE TRIGGER trg_orders_seal_verify BEFORE UPDATE OF status ON orders
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0 AND OLD.status = 'pending' AND NEW.status = 'completed' BEGIN
  SELECT RAISE(ABORT, 'seal: cash session must be open on this device')
  WHERE (SELECT status FROM cash_sessions WHERE id = NEW.cash_session_id AND device_id = NEW.device_id) IS NOT 'open';
  SELECT RAISE(ABORT, 'seal: cashier must be an active staff member')
  WHERE (SELECT is_active FROM v_staff_effective WHERE staff_id = NEW.staff_id) IS NOT 1;
  SELECT RAISE(ABORT, 'seal: line_count / item_count mismatch')
  WHERE NEW.line_count <> (SELECT COUNT(*) FROM order_items WHERE order_id = NEW.id)
     OR NEW.item_count <> (SELECT COALESCE(SUM(qty), 0) FROM order_items WHERE order_id = NEW.id);
  SELECT RAISE(ABORT, 'seal: header totals do not equal the sum of lines')
  WHERE NEW.gross_sales      <> (SELECT COALESCE(SUM(line_gross), 0) FROM order_items WHERE order_id = NEW.id)
     OR NEW.discount_total   <> (SELECT COALESCE(SUM(line_discount + order_discount_share), 0) FROM order_items WHERE order_id = NEW.id)
     OR NEW.net_sales        <> (SELECT COALESCE(SUM(line_net), 0) FROM order_items WHERE order_id = NEW.id)
     OR NEW.tax_total        <> (SELECT COALESCE(SUM(tax_amount), 0) FROM order_items WHERE order_id = NEW.id)
     OR NEW.cost_total       <> (SELECT COALESCE(SUM(cost_total), 0) FROM order_items WHERE order_id = NEW.id)
     OR NEW.quick_sale_net   <> (SELECT COALESCE(SUM(line_net), 0) FROM order_items WHERE order_id = NEW.id AND is_custom = 1)
     OR NEW.unknown_cost_net <> (SELECT COALESCE(SUM(line_net), 0) FROM order_items WHERE order_id = NEW.id AND cost_known = 0);
  -- inclusive pricing: Σ line_net = Σ line_total − tax ⇒ grand = Σ line_total + rounding;
  -- exclusive pricing: Σ line_net = Σ line_total ⇒ grand = Σ line_total + tax + rounding.
  SELECT RAISE(ABORT, 'seal: grand_total does not match lines + rounding (+ tax when exclusive)')
  WHERE NEW.grand_total <> (SELECT COALESCE(SUM(line_total), 0) FROM order_items WHERE order_id = NEW.id) + NEW.rounding
        + CASE WHEN (SELECT COALESCE(SUM(line_net), 0) FROM order_items WHERE order_id = NEW.id)
                  = (SELECT COALESCE(SUM(line_total), 0) FROM order_items WHERE order_id = NEW.id)
               THEN NEW.tax_total ELSE 0 END;
  SELECT RAISE(ABORT, 'seal: payments must equal grand_total exactly; change only from cash tendered')
  WHERE NEW.grand_total <> (SELECT COALESCE(SUM(amount), 0) FROM order_payments WHERE order_id = NEW.id)
     OR NEW.change_due  <> (SELECT COALESCE(SUM(tendered - amount), 0) FROM order_payments WHERE order_id = NEW.id AND tendered IS NOT NULL);
  SELECT RAISE(ABORT, 'seal: stock slices must cover each tracked line exactly (qty, revenue, cost)')
  WHERE EXISTS (
    SELECT 1 FROM order_items oi JOIN products p ON p.id = oi.product_id
    WHERE oi.order_id = NEW.id AND p.track_stock = 1
      AND (oi.qty       <> -(SELECT COALESCE(SUM(qty_delta), 0)             FROM stock_movements m WHERE m.order_item_id = oi.id AND m.reason = 'sale')
        OR oi.line_net  <>  (SELECT COALESCE(SUM(revenue_net), 0)           FROM stock_movements m WHERE m.order_item_id = oi.id AND m.reason = 'sale')
        OR oi.cost_total <> (SELECT COALESCE(SUM(-qty_delta * unit_cost), 0) FROM stock_movements m WHERE m.order_item_id = oi.id AND m.reason = 'sale')));
  SELECT RAISE(ABORT, 'seal: price below list needs PRICE_OVERRIDE or an approver who holds it')
  WHERE EXISTS (
    SELECT 1 FROM order_items oi WHERE oi.order_id = NEW.id AND oi.price_overridden = 1
      AND ((SELECT perms FROM v_staff_effective WHERE staff_id = NEW.staff_id) & 4) = 0
      AND COALESCE((SELECT perms & 4 FROM v_staff_effective WHERE staff_id = oi.approved_by AND is_active = 1), 0) = 0);
  SELECT RAISE(ABORT, 'seal: discount above the cashier limit needs an approver whose limit covers it')
  WHERE EXISTS (
    SELECT 1 FROM order_items oi WHERE oi.order_id = NEW.id
      AND (oi.line_discount + oi.order_discount_share) * 10000
          > (SELECT max_discount_bp FROM v_staff_effective WHERE staff_id = NEW.staff_id) * oi.line_gross
      AND (oi.line_discount + oi.order_discount_share) * 10000
          > COALESCE((SELECT max_discount_bp FROM v_staff_effective
                      WHERE staff_id = COALESCE(oi.approved_by, NEW.approved_by) AND is_active = 1), -1) * oi.line_gross);
END;

-- POLICY: void (completed → voided) only on the originating device, while its session is still open,
-- by someone holding VOID_ORDER, or approved by a different active staff member who holds it.
CREATE TRIGGER trg_orders_void_verify BEFORE UPDATE OF status ON orders
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0 AND OLD.status = 'completed' AND NEW.status = 'voided' BEGIN
  SELECT RAISE(ABORT, 'void: only on the device that made the sale')
  WHERE NEW.device_id IS NOT json_extract((SELECT value FROM device_settings WHERE key = 'device.id'), '$');
  SELECT RAISE(ABORT, 'void: the order''s cash session is closed — sales are final')
  WHERE (SELECT status FROM cash_sessions WHERE id = NEW.cash_session_id) IS NOT 'open';
  SELECT RAISE(ABORT, 'void: requires VOID_ORDER or a supervisor approval')
  WHERE COALESCE((SELECT perms & 32 FROM v_staff_effective WHERE staff_id = NEW.voided_by AND is_active = 1), 0) = 0
    AND (NEW.void_approved_by IS NULL OR NEW.void_approved_by = NEW.voided_by
         OR COALESCE((SELECT perms & 32 FROM v_staff_effective WHERE staff_id = NEW.void_approved_by AND is_active = 1), 0) = 0);
END;

-- Derived caches (rebuildable; not synced)
CREATE TABLE stock_levels (product_id TEXT PRIMARY KEY, on_hand INTEGER NOT NULL DEFAULT 0);
CREATE TABLE batch_item_stock (
  batch_item_id TEXT PRIMARY KEY, batch_id TEXT NOT NULL, product_id TEXT NOT NULL,
  received_qty INTEGER NOT NULL DEFAULT 0, on_hand INTEGER NOT NULL DEFAULT 0,
  sold_qty INTEGER NOT NULL DEFAULT 0, sold_revenue INTEGER NOT NULL DEFAULT 0, sold_cost INTEGER NOT NULL DEFAULT 0,
  written_off_qty INTEGER NOT NULL DEFAULT 0, written_off_cost INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX ix_bis_batch ON batch_item_stock(batch_id);
CREATE INDEX ix_bis_negative ON batch_item_stock(product_id) WHERE on_hand < 0;   -- "Oversold layers"

-- DERIVED. Sign convention: a sale slice has qty_delta −2 and revenue_net +900 ⇒ sold_qty +2, sold_revenue +900.
-- void slices mirror it exactly (+qty, −revenue, same layer and cost).
CREATE TRIGGER trg_mov_derive AFTER INSERT ON stock_movements BEGIN
  INSERT INTO stock_levels(product_id, on_hand) VALUES (NEW.product_id, NEW.qty_delta)
  ON CONFLICT(product_id) DO UPDATE SET on_hand = on_hand + excluded.on_hand;

  INSERT INTO batch_item_stock(batch_item_id, batch_id, product_id)
  SELECT NEW.batch_item_id, bi.batch_id, NEW.product_id FROM batch_items bi WHERE bi.id = NEW.batch_item_id
  ON CONFLICT(batch_item_id) DO NOTHING;

  UPDATE batch_item_stock SET
    on_hand          = on_hand + NEW.qty_delta,
    received_qty     = received_qty     + CASE WHEN NEW.reason IN ('batch_intake','batch_reopen') THEN NEW.qty_delta ELSE 0 END,
    sold_qty         = sold_qty         + CASE WHEN NEW.reason IN ('sale','void') THEN -NEW.qty_delta ELSE 0 END,
    sold_revenue     = sold_revenue     + CASE WHEN NEW.reason IN ('sale','void') THEN COALESCE(NEW.revenue_net, 0) ELSE 0 END,
    sold_cost        = sold_cost        + CASE WHEN NEW.reason IN ('sale','void') THEN -NEW.qty_delta * NEW.unit_cost ELSE 0 END,
    written_off_qty  = written_off_qty  + CASE WHEN NEW.reason IN ('damage','count_correction','adjustment') THEN -NEW.qty_delta ELSE 0 END,
    written_off_cost = written_off_cost + CASE WHEN NEW.reason IN ('damage','count_correction','adjustment') THEN -NEW.qty_delta * NEW.unit_cost ELSE 0 END
  WHERE batch_item_id = NEW.batch_item_id;
END;

-- ═════════ expenses ═════════
CREATE TABLE expense_categories (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, name TEXT NOT NULL,
  is_active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE expenses (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL,
  category_id TEXT NOT NULL REFERENCES expense_categories(id),
  amount INTEGER NOT NULL CHECK (amount > 0),
  business_date TEXT NOT NULL,
  payment_method TEXT NOT NULL CHECK (payment_method IN ('cash','card','bank_transfer','qr','other')),
  paid_from_drawer INTEGER NOT NULL DEFAULT 0,
  cash_session_id TEXT REFERENCES cash_sessions(id),
  payee TEXT, note TEXT,
  recorded_by TEXT NOT NULL REFERENCES staff(id),
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX ix_expenses_bdate ON expenses(business_date) WHERE deleted_at IS NULL;

-- ═════════ quick sale promotion ═════════
CREATE TABLE quick_sale_links (
  id TEXT PRIMARY KEY,                               -- UUIDv5(store_id, custom_key)
  store_id TEXT NOT NULL,
  custom_key TEXT NOT NULL, display_name TEXT NOT NULL,
  action TEXT NOT NULL CHECK (action IN ('promoted','ignored')),
  product_id TEXT REFERENCES products(id), batch_id TEXT REFERENCES batches(id),
  decided_by TEXT NOT NULL, decided_at TEXT NOT NULL,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1,
  CHECK (action = 'ignored' OR product_id IS NOT NULL)
);
CREATE UNIQUE INDEX ux_qsl_key ON quick_sale_links(store_id, custom_key) WHERE deleted_at IS NULL;

-- ═════════ audit (append-only, per-device hash chain) ═════════
CREATE TABLE audit_logs (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  device_seq INTEGER NOT NULL CHECK (device_seq >= 1),
  staff_id TEXT, approved_by TEXT,
  action TEXT NOT NULL,
  entity_type TEXT, entity_id TEXT,
  metadata TEXT NOT NULL DEFAULT '{}',
  business_date TEXT NOT NULL,
  prev_hash TEXT NOT NULL, hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (device_id, device_seq)
);
CREATE INDEX ix_audit_keyset ON audit_logs(created_at DESC, id DESC);
CREATE INDEX ix_audit_entity ON audit_logs(entity_type, entity_id);
CREATE INDEX ix_audit_action ON audit_logs(action, business_date);
CREATE TRIGGER trg_audit_no_update BEFORE UPDATE ON audit_logs
BEGIN SELECT RAISE(ABORT, 'audit_logs are append-only'); END;
CREATE TRIGGER trg_audit_no_delete BEFORE DELETE ON audit_logs
BEGIN SELECT RAISE(ABORT, 'audit_logs are append-only'); END;
-- POLICY: local audit rows must extend this device's chain (gapless seq, linked prev_hash).
-- Pulled rows (other devices) are verified by audit::verify and the server instead.
CREATE TRIGGER trg_audit_chain_guard BEFORE INSERT ON audit_logs
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0 BEGIN
  SELECT RAISE(ABORT, 'audit: device_seq must be the previous seq + 1')
  WHERE NEW.device_seq <> COALESCE((SELECT MAX(device_seq) FROM audit_logs WHERE device_id = NEW.device_id), 0) + 1;
  SELECT RAISE(ABORT, 'audit: prev_hash must equal the previous row''s hash')
  WHERE NEW.prev_hash <> COALESCE((SELECT hash FROM audit_logs WHERE device_id = NEW.device_id
                                   ORDER BY device_seq DESC LIMIT 1),
                                  '0000000000000000000000000000000000000000000000000000000000000000');
END;

-- ═════════ derived read models: reporting rollups, customers, quick sales ═════════
CREATE TABLE daily_sales_summary (
  business_date TEXT NOT NULL, device_id TEXT NOT NULL,
  sales_count INTEGER NOT NULL DEFAULT 0, void_count INTEGER NOT NULL DEFAULT 0, void_value INTEGER NOT NULL DEFAULT 0,
  gross_sales INTEGER NOT NULL DEFAULT 0, discounts INTEGER NOT NULL DEFAULT 0,
  net_sales INTEGER NOT NULL DEFAULT 0,
  tax_total INTEGER NOT NULL DEFAULT 0, cogs INTEGER NOT NULL DEFAULT 0,
  quick_sale_net INTEGER NOT NULL DEFAULT 0, unknown_cost_net INTEGER NOT NULL DEFAULT 0,
  customer_orders INTEGER NOT NULL DEFAULT 0,        -- completed orders with a customer attached
  PRIMARY KEY (business_date, device_id)
);
CREATE TABLE daily_payment_summary (
  business_date TEXT NOT NULL, device_id TEXT NOT NULL, method TEXT NOT NULL,
  amount INTEGER NOT NULL DEFAULT 0, txn_count INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (business_date, device_id, method)
);
CREATE TABLE customer_stats (
  customer_id TEXT PRIMARY KEY,
  visit_count INTEGER NOT NULL DEFAULT 0,
  total_spend INTEGER NOT NULL DEFAULT 0,            -- Σ grand_total
  total_net INTEGER NOT NULL DEFAULT 0,              -- Σ net_sales
  first_visit_date TEXT, last_visit_date TEXT
);
CREATE INDEX ix_customer_stats_spend ON customer_stats(total_spend DESC);
CREATE INDEX ix_customer_stats_last ON customer_stats(last_visit_date DESC);
CREATE TABLE quick_sale_stats (
  custom_key TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,                        -- latest spelling
  times_sold INTEGER NOT NULL DEFAULT 0, units_sold INTEGER NOT NULL DEFAULT 0,
  revenue_net INTEGER NOT NULL DEFAULT 0,
  last_price INTEGER NOT NULL, min_price INTEGER NOT NULL, max_price INTEGER NOT NULL,
  first_sold_at TEXT NOT NULL, last_sold_at TEXT NOT NULL
);
CREATE INDEX ix_qss_frequency ON quick_sale_stats(times_sold DESC);

-- Shared fragments, applied in the triggers below:
--   (a) local SEAL (pending → completed): count the order, its payments (aggregated), its customer,
--       and its quick-sale lines.
--   (b) PULLED insert of an already-completed order: count the order + customer now; its payments and
--       lines arrive afterwards (parents first) and are counted by their own insert triggers.
--   (c) VOID: subtract the order and its payments; recompute the customer and quick-sale keys from base rows.

CREATE TRIGGER trg_rollup_seal AFTER UPDATE OF status ON orders
WHEN OLD.status = 'pending' AND NEW.status = 'completed' BEGIN
  INSERT INTO daily_sales_summary(business_date, device_id) VALUES (NEW.business_date, NEW.device_id)
  ON CONFLICT DO NOTHING;
  UPDATE daily_sales_summary SET
    sales_count = sales_count + 1, gross_sales = gross_sales + NEW.gross_sales,
    discounts = discounts + NEW.discount_total, net_sales = net_sales + NEW.net_sales,
    tax_total = tax_total + NEW.tax_total, cogs = cogs + NEW.cost_total,
    quick_sale_net = quick_sale_net + NEW.quick_sale_net, unknown_cost_net = unknown_cost_net + NEW.unknown_cost_net,
    customer_orders = customer_orders + (NEW.customer_id IS NOT NULL)
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
  INSERT INTO daily_payment_summary(business_date, device_id, method, amount, txn_count)
  SELECT NEW.business_date, NEW.device_id, p.method, SUM(p.amount), COUNT(*)
  FROM order_payments p WHERE p.order_id = NEW.id GROUP BY p.method
  ON CONFLICT(business_date, device_id, method) DO UPDATE SET
    amount = amount + excluded.amount, txn_count = txn_count + excluded.txn_count;
  INSERT INTO quick_sale_stats(custom_key, display_name, times_sold, units_sold, revenue_net,
                               last_price, min_price, max_price, first_sold_at, last_sold_at)
  SELECT oi.custom_key, oi.name, 1, oi.qty, oi.line_net, oi.unit_price, oi.unit_price, oi.unit_price, oi.created_at, oi.created_at
  FROM order_items oi WHERE oi.order_id = NEW.id AND oi.is_custom = 1
  ON CONFLICT(custom_key) DO UPDATE SET
    times_sold = times_sold + 1, units_sold = units_sold + excluded.units_sold, revenue_net = revenue_net + excluded.revenue_net,
    display_name = CASE WHEN excluded.last_sold_at >= last_sold_at THEN excluded.display_name ELSE display_name END,
    last_price   = CASE WHEN excluded.last_sold_at >= last_sold_at THEN excluded.last_price ELSE last_price END,
    min_price = MIN(min_price, excluded.min_price), max_price = MAX(max_price, excluded.max_price),
    first_sold_at = MIN(first_sold_at, excluded.first_sold_at), last_sold_at = MAX(last_sold_at, excluded.last_sold_at);
END;

CREATE TRIGGER trg_rollup_insert_completed AFTER INSERT ON orders WHEN NEW.status = 'completed' BEGIN
  INSERT INTO daily_sales_summary(business_date, device_id) VALUES (NEW.business_date, NEW.device_id)
  ON CONFLICT DO NOTHING;
  UPDATE daily_sales_summary SET
    sales_count = sales_count + 1, gross_sales = gross_sales + NEW.gross_sales,
    discounts = discounts + NEW.discount_total, net_sales = net_sales + NEW.net_sales,
    tax_total = tax_total + NEW.tax_total, cogs = cogs + NEW.cost_total,
    quick_sale_net = quick_sale_net + NEW.quick_sale_net, unknown_cost_net = unknown_cost_net + NEW.unknown_cost_net,
    customer_orders = customer_orders + (NEW.customer_id IS NOT NULL)
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
END;
-- A pulled order that arrives already voided is never counted, except in void_count / void_value.
CREATE TRIGGER trg_rollup_insert_voided AFTER INSERT ON orders WHEN NEW.status = 'voided' BEGIN
  INSERT INTO daily_sales_summary(business_date, device_id) VALUES (NEW.business_date, NEW.device_id)
  ON CONFLICT DO NOTHING;
  UPDATE daily_sales_summary SET void_count = void_count + 1, void_value = void_value + NEW.grand_total
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
END;

CREATE TRIGGER trg_customer_stats_add AFTER UPDATE OF status ON orders
WHEN OLD.status = 'pending' AND NEW.status = 'completed' AND NEW.customer_id IS NOT NULL BEGIN
  INSERT INTO customer_stats(customer_id, visit_count, total_spend, total_net, first_visit_date, last_visit_date)
  VALUES (NEW.customer_id, 1, NEW.grand_total, NEW.net_sales, NEW.business_date, NEW.business_date)
  ON CONFLICT(customer_id) DO UPDATE SET
    visit_count = visit_count + 1, total_spend = total_spend + excluded.total_spend, total_net = total_net + excluded.total_net,
    first_visit_date = MIN(COALESCE(first_visit_date, excluded.first_visit_date), excluded.first_visit_date),
    last_visit_date  = MAX(COALESCE(last_visit_date,  excluded.last_visit_date),  excluded.last_visit_date);
END;
CREATE TRIGGER trg_customer_stats_add_pulled AFTER INSERT ON orders
WHEN NEW.status = 'completed' AND NEW.customer_id IS NOT NULL BEGIN
  INSERT INTO customer_stats(customer_id, visit_count, total_spend, total_net, first_visit_date, last_visit_date)
  VALUES (NEW.customer_id, 1, NEW.grand_total, NEW.net_sales, NEW.business_date, NEW.business_date)
  ON CONFLICT(customer_id) DO UPDATE SET
    visit_count = visit_count + 1, total_spend = total_spend + excluded.total_spend, total_net = total_net + excluded.total_net,
    first_visit_date = MIN(COALESCE(first_visit_date, excluded.first_visit_date), excluded.first_visit_date),
    last_visit_date  = MAX(COALESCE(last_visit_date,  excluded.last_visit_date),  excluded.last_visit_date);
END;

-- Pulled children of an already-completed order (local children are counted at seal time instead).
CREATE TRIGGER trg_rollup_payment_pulled AFTER INSERT ON order_payments
WHEN (SELECT status FROM orders WHERE id = NEW.order_id) = 'completed' BEGIN
  INSERT INTO daily_payment_summary(business_date, device_id, method, amount, txn_count)
  SELECT o.business_date, o.device_id, NEW.method, NEW.amount, 1 FROM orders o WHERE o.id = NEW.order_id
  ON CONFLICT(business_date, device_id, method) DO UPDATE SET
    amount = amount + excluded.amount, txn_count = txn_count + 1;
END;
CREATE TRIGGER trg_qss_item_pulled AFTER INSERT ON order_items
WHEN NEW.is_custom = 1 AND (SELECT status FROM orders WHERE id = NEW.order_id) = 'completed' BEGIN
  INSERT INTO quick_sale_stats(custom_key, display_name, times_sold, units_sold, revenue_net,
                               last_price, min_price, max_price, first_sold_at, last_sold_at)
  VALUES (NEW.custom_key, NEW.name, 1, NEW.qty, NEW.line_net, NEW.unit_price, NEW.unit_price, NEW.unit_price, NEW.created_at, NEW.created_at)
  ON CONFLICT(custom_key) DO UPDATE SET
    times_sold = times_sold + 1, units_sold = units_sold + excluded.units_sold, revenue_net = revenue_net + excluded.revenue_net,
    display_name = CASE WHEN excluded.last_sold_at >= last_sold_at THEN excluded.display_name ELSE display_name END,
    last_price   = CASE WHEN excluded.last_sold_at >= last_sold_at THEN excluded.last_price ELSE last_price END,
    min_price = MIN(min_price, excluded.min_price), max_price = MAX(max_price, excluded.max_price),
    first_sold_at = MIN(first_sold_at, excluded.first_sold_at), last_sold_at = MAX(last_sold_at, excluded.last_sold_at);
END;

CREATE TRIGGER trg_rollup_void AFTER UPDATE OF status ON orders
WHEN OLD.status = 'completed' AND NEW.status = 'voided' BEGIN
  UPDATE daily_sales_summary SET
    sales_count = sales_count - 1, void_count = void_count + 1, void_value = void_value + NEW.grand_total,
    gross_sales = gross_sales - NEW.gross_sales, discounts = discounts - NEW.discount_total,
    net_sales = net_sales - NEW.net_sales, tax_total = tax_total - NEW.tax_total, cogs = cogs - NEW.cost_total,
    quick_sale_net = quick_sale_net - NEW.quick_sale_net, unknown_cost_net = unknown_cost_net - NEW.unknown_cost_net,
    customer_orders = customer_orders - (NEW.customer_id IS NOT NULL)
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
  UPDATE daily_payment_summary SET
    amount    = amount    - (SELECT COALESCE(SUM(p.amount), 0) FROM order_payments p
                             WHERE p.order_id = NEW.id AND p.method = daily_payment_summary.method),
    txn_count = txn_count - (SELECT COUNT(*) FROM order_payments p
                             WHERE p.order_id = NEW.id AND p.method = daily_payment_summary.method)
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
  -- exact recompute for the affected customer (the voided order is already excluded: status = 'voided')
  INSERT INTO customer_stats(customer_id, visit_count, total_spend, total_net, first_visit_date, last_visit_date)
  SELECT c.cid, COUNT(o.id), COALESCE(SUM(o.grand_total), 0), COALESCE(SUM(o.net_sales), 0),
         MIN(o.business_date), MAX(o.business_date)
  FROM (SELECT NEW.customer_id AS cid) c
  LEFT JOIN orders o ON o.customer_id = c.cid AND o.status = 'completed'
  WHERE c.cid IS NOT NULL
  GROUP BY c.cid
  ON CONFLICT(customer_id) DO UPDATE SET
    visit_count = excluded.visit_count, total_spend = excluded.total_spend, total_net = excluded.total_net,
    first_visit_date = excluded.first_visit_date, last_visit_date = excluded.last_visit_date;
  -- exact recompute for the affected quick-sale keys
  DELETE FROM quick_sale_stats
  WHERE custom_key IN (SELECT custom_key FROM order_items WHERE order_id = NEW.id AND is_custom = 1);
  INSERT INTO quick_sale_stats(custom_key, display_name, times_sold, units_sold, revenue_net,
                               last_price, min_price, max_price, first_sold_at, last_sold_at)
  SELECT oi.custom_key,
         (SELECT x.name FROM order_items x JOIN orders xo ON xo.id = x.order_id AND xo.status = 'completed'
          WHERE x.custom_key = oi.custom_key AND x.is_custom = 1 ORDER BY x.created_at DESC LIMIT 1),
         COUNT(*), SUM(oi.qty), SUM(oi.line_net),
         (SELECT y.unit_price FROM order_items y JOIN orders yo ON yo.id = y.order_id AND yo.status = 'completed'
          WHERE y.custom_key = oi.custom_key AND y.is_custom = 1 ORDER BY y.created_at DESC LIMIT 1),
         MIN(oi.unit_price), MAX(oi.unit_price), MIN(oi.created_at), MAX(oi.created_at)
  FROM order_items oi JOIN orders o ON o.id = oi.order_id AND o.status = 'completed'
  WHERE oi.is_custom = 1
    AND oi.custom_key IN (SELECT custom_key FROM order_items WHERE order_id = NEW.id AND is_custom = 1)
  GROUP BY oi.custom_key;
END;
-- Voids are same-session only (§3.3), so a void never rewrites a closed Z-report.

-- ═════════ integrity probes (empty = healthy; checked at startup and in the support bundle) ═════════
CREATE VIEW v_integrity_pending_orders AS            -- a committed 'pending' order means a Rust bug
SELECT id, receipt_no, created_at FROM orders WHERE status = 'pending';
CREATE VIEW v_integrity_void_unreversed AS           -- voided orders whose sale slices aren't fully reversed
SELECT o.id AS order_id, m.order_item_id, m.batch_item_id, SUM(m.qty_delta) AS net_qty
FROM orders o JOIN order_items oi ON oi.order_id = o.id
JOIN stock_movements m ON m.order_item_id = oi.id AND m.reason IN ('sale','void')
WHERE o.status = 'voided'
GROUP BY o.id, m.order_item_id, m.batch_item_id
HAVING SUM(m.qty_delta) <> 0;

-- ═════════ local-only plumbing ═════════
CREATE TABLE receipt_counters (day TEXT PRIMARY KEY, last_seq INTEGER NOT NULL);
CREATE TABLE sku_counters (device_digit INTEGER PRIMARY KEY, last_seq INTEGER NOT NULL);
CREATE TABLE held_carts (                            -- '__current__' = crash-recovery cart (500 ms debounce)
  id TEXT PRIMARY KEY, label TEXT, payload TEXT NOT NULL,
  checkout_token TEXT,                               -- the cart's token; deleted in the checkout transaction
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE print_jobs (                            -- written in the SAME transaction as the thing printed
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('receipt','void_slip','z_report','labels','drawer_kick','test')),
  ref_id TEXT,                                       -- order / session / label job id
  printer_key TEXT NOT NULL,                         -- device_settings printer profile key
  payload TEXT NOT NULL,                             -- JSON render input (re-rendered on retry)
  status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','printing','done','failed','abandoned')),
  attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX ix_print_jobs_pending ON print_jobs(status, created_at) WHERE status IN ('queued','printing','failed');

CREATE TABLE sync_outbox (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  op_id TEXT NOT NULL UNIQUE,
  table_name TEXT NOT NULL, row_id TEXT NOT NULL,
  op TEXT NOT NULL CHECK (op IN ('upsert','delete')),
  payload TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','in_flight','failed','dead')),
  attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT,
  next_attempt_at TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE INDEX ix_outbox_ready ON sync_outbox(status, next_attempt_at, seq);

-- One global pull cursor (row table_name = '__global__'); server_seq is a single store-wide sequence.
CREATE TABLE sync_state (table_name TEXT PRIMARY KEY, last_server_seq INTEGER NOT NULL DEFAULT 0, last_pulled_at TEXT);

CREATE TABLE sync_logs (
  id TEXT PRIMARY KEY, device_id TEXT NOT NULL,
  direction TEXT NOT NULL CHECK (direction IN ('push','pull')),
  started_at TEXT NOT NULL, finished_at TEXT,
  result TEXT NOT NULL CHECK (result IN ('success','partial','error')),
  rows_sent INTEGER NOT NULL DEFAULT 0, rows_received INTEGER NOT NULL DEFAULT 0, rows_rejected INTEGER NOT NULL DEFAULT 0,
  cursor_before INTEGER, cursor_after INTEGER,
  error_code TEXT, error_message TEXT, details TEXT
);
CREATE INDEX ix_sync_logs_started ON sync_logs(started_at);
```

**Parents are always present when a pulled child is applied.** This matters because the foreign keys and the derived triggers (`trg_rollup_payment` reads the order, and `trg_mov_derive` reads the batch item) depend on it. The pull protocol guarantees it with a parent closure (§6.2). Without that guarantee, a missing parent would abort the pull, or with relaxed foreign keys, silently drop rollup counts.

**Seeds** (Phase 0, first-run wizard, one transaction):
- The four system roles with the default masks and limits from §3.5 (Cashier `max_discount_bp = 500`).
- The Owner staff member, with a PIN of at least 6 digits.
- Expense categories: Rent, Electricity, Water, Wages, Transport (non-batch), Tea & sundries, Repairs, Other.
- System batches `OPENING-STOCK` (draft until go-live) and `ADJUSTMENTS` (received, so it can take layers).
- The owner-confirmed store settings from §11, including `store.name = GayanShop` and `receipt.footer`.
- Device settings: `ui.default_language = en`, and the two built-in label templates (`two_up_2x1`, `three_up_1x1`).
- Every seeded row goes to the outbox. One `STORE_INITIALIZED` audit row starts this device's hash chain. `STORE_INITIALIZED` is the first entry in `AuditAction`, and it's appended to the §3.6 System actions.

**Migrations after `0001_init.sql`** (append-only; doc and `rebuild_derived` updated with each):
- `0002_report_indexes.sql` (Block 2.10): `CREATE INDEX ix_mov_writeoffs ON stock_movements(business_date) WHERE reason IN ('damage','count_correction','adjustment')`. Stock write-offs for the P&L read only write-off movements (§5.3, §13.4). No derived logic changed.

### 4.4 Cloud (Postgres) deltas

- **Layout** (`supabase/migrations`, Block 3.1). The business tables live in schema `pos`, which PostgREST doesn't expose, and the API roles (`anon`, `authenticated`) have no privilege on anything in it. Devices only call the RPCs in `public` (`sync_push`, `sync_pull`, `sync_health`). Those are `SECURITY DEFINER` functions owned by the `NOLOGIN` role `pos_sync`, which has no `BYPASSRLS`, so every row they read or write still passes the RLS policies below.
- Types become `uuid`, `timestamptz`, `bigint` money and masks, `boolean` and `date` (business_date).
  - **JSON-in-text columns stay `text`** (`store_settings.value`, `products.attributes`, `cash_sessions.denominations`, `audit_logs.metadata`), with a JSON-validity check. `jsonb` would reorder keys and drop whitespace, so a pulled row would no longer be byte-identical to what its origin wrote, and `audit_logs.metadata` feeds the hash chain. A dashboard casts them to `jsonb` when it reads them.
  - `sync_pull` renders rows back in the terminal's wire format exactly: flags as 0/1, timestamps as `YYYY-MM-DDTHH:MM:SS.mmmZ` (`pos.ts()`, the same format as `clock::iso`), so text comparison for last-write-wins keeps working on every terminal.
- Every synced table gets `server_seq bigint NOT NULL` (one global sequence, set by a `BEFORE INSERT OR UPDATE` trigger) and `server_received_at timestamptz` (`batches` already has a business `received_at`). This cursor is only safe because `sync_push` serialises writers per store (§6.1).
- `orders.checkout_token` is unique per store (`ux_orders_checkout_token (store_id, checkout_token)`).
- `processed_ops(op_id uuid PRIMARY KEY, store_id, device_id, table_name, result, reason, processed_at)` is the idempotency ledger. A replayed op returns its first result, rejections included.
- `stores(id, name, owner_user_id, created_at)` and `devices(id, store_id, code, name, auth_user_id, app_version, registered_at, last_seen_at, last_audit_seq, last_audit_hash, chain_broken_at, chain_broken_seq)`. `pos.synced_tables` lists the eighteen synced tables with their merge kind (insert-only, order, last-write-wins) and apply order.
- Rows the server itself changes carry the SYSTEM device id `00000000-0000-0000-0000-000000000000`.
- RLS on every table: `store_id = pos.claim_store_id()`. The claims are read from `request.jwt.claims` (which PostgREST sets from the verified JWT), not from Supabase's `auth.jwt()`, so the same schema runs behind a bare PostgREST on a self-hosted server. Only a device identity (`app_metadata.role = 'device'`) has a store claim; any other signed-in user sees no rows.
  - **`audit_logs`, `stock_movements`, `order_items`, `order_payments` and `cash_movements` get no UPDATE or DELETE policy.**
  - On `orders`, UPDATE is allowed only for the void transition, enforced by a trigger.
- `staff.pin_hash` is only readable by device identities, never by a future web dashboard role. `customers.phone_e164` is readable by device identities, and by dashboard roles only with an explicit customer-data grant.
- The derived tables are cloud **views** (`stock_levels`, `batch_item_stock`, daily summaries), or materialised views for the dashboard. They're never mutable stored columns.

---

## 5. Reporting

### 5.1 Metric glossary (single definitions; every report uses these)

| Metric | Definition |
|---|---|
| **Gross sales** | Σ `line_gross` on completed orders, at shelf price (tax-inclusive while `tax.inclusive`) |
| **Discounts** | Σ line discounts + bill discounts on completed orders |
| **Voids** | Count and value of voided orders, by reason and approver. They're shown for control and **excluded** from every revenue figure. |
| **Net sales = Total Revenue** | Σ `line_net` on completed orders. It's **ex tax** (with tax at 0%, ex-tax equals tax-inclusive). There are no refunds or returns to subtract. |
| **Tax collected** | Σ `tax_amount` (0 while not registered) |
| **COGS** | Σ sale-slice cost − Σ return-line cost (both restock and damaged returns reverse the original cost) + Σ `allocation_variance` of batches received in the period |
| **Gross profit** | Net sales − COGS. Gross margin = GP / Net sales. |
| **Stock write-offs** | Σ cost of `damage`, `count_correction` and `adjustment` movements. Net of found stock, so it can be negative. |
| **Operating expenses** | Σ `expenses.amount` (not deleted). **Batch purchases are never expenses.** They're inventory, and become COGS when sold. Batch transport belongs in `extra_costs`, never in expenses, or it gets double-counted. |
| **Net profit** | Gross profit − Stock write-offs − Operating expenses |
| **Quick-sale revenue / unknown-cost revenue** | Shown beside the profit figures. Unknown-cost revenue has zero COGS, so it *inflates* gross profit, and the report says so. |
| **Payment breakdown** | Σ `order_payments.amount` by method (cash, card, transfer, QR, other) on completed orders. Always ≥ 0. Shown as the donut chart. |
| **Customer metrics** | The share of orders with a customer attached (`customer_orders / sales_count`), new vs returning customers, and top customers by `total_spend` (`customer_stats`). All on completed orders. |
| **Stock valuation (at cost)** | Σ over layers of `max(on_hand, 0) × unit_cost`. Negative layers are listed separately. At retail: × the current product price. |

### 5.2 Periods

| Period | Definition |
|---|---|
| Day (store daily report) | One `business_date` |
| Z-report | One cash session (§3.4) |
| Week | 7 days starting on the store's `week_start` |
| Month / Year | Calendar month / year on `business_date` |
| All time | No date filter |
| Custom | An inclusive `from..to` range |

Every period supports a `device_id` filter ("Counter 2 only") and comparison with the previous equivalent period. Trend charts group days by `substr(business_date,1,7)` (month) or by week number.

### 5.3 How reports stay fast

- Sales and payment metrics come from `daily_sales_summary` and `daily_payment_summary`. A yearly report is ~365 × 3 rows, even at millions of order lines.
- Write-offs come from `stock_movements` via the partial index `ix_mov_writeoffs` (migration `0002_report_indexes.sql`, Block 2.10; `ix_mov_date_reason` would walk every sale slice of the period). Expenses use `ix_expenses_bdate`. Batch metrics come from `batch_item_stock`. Date filters are plain `BETWEEN` bounds, with sentinels for open ends, so they always use these indexes.
- Point-in-time stock valuation ("as of 31 Dec") sums the ledger, `WHERE business_date <= :d`, grouped by layer. It's slower but exact, and runs on demand.
- **`rebuild_derived`** truncates and recomputes every derived table from the base tables. It runs after a restore, after a migration that touches derived logic, and from Settings → Maintenance.
  - A CI test asserts that rollup totals equal a raw `SUM` over orders and payments for randomised data.

The P&L query:
```sql
-- :from/:to inclusive business dates; :dev optional
WITH s AS (
  SELECT SUM(sales_count) sales_count, SUM(void_count) void_count, SUM(void_value) void_value,
         SUM(gross_sales) gross_sales, SUM(discounts) discounts,
         SUM(net_sales) net_sales, SUM(tax_total) tax, SUM(customer_orders) customer_orders,
         SUM(cogs) cogs, SUM(quick_sale_net) qs, SUM(unknown_cost_net) unk
  FROM daily_sales_summary
  WHERE business_date BETWEEN :from AND :to AND (:dev IS NULL OR device_id = :dev)),
v AS (SELECT COALESCE(SUM(allocation_variance),0) var FROM batches
      WHERE received_business_date BETWEEN :from AND :to AND status <> 'draft' AND deleted_at IS NULL),
w AS (SELECT COALESCE(SUM(-qty_delta * unit_cost),0) wo FROM stock_movements
      WHERE business_date BETWEEN :from AND :to AND reason IN ('damage','count_correction','adjustment')),
e AS (SELECT COALESCE(SUM(amount),0) opex FROM expenses
      WHERE business_date BETWEEN :from AND :to AND deleted_at IS NULL)
SELECT s.*,
       s.cogs + v.var                                AS cogs_total,
       s.net_sales - s.cogs - v.var                  AS gross_profit,
       w.wo AS write_offs, e.opex,
       s.net_sales - s.cogs - v.var - w.wo - e.opex  AS net_profit
FROM s, v, w, e;
-- Store-wide only for v/w/e (they aren't per-device). The device filter applies to sales metrics.
```

Batch report core (one batch):
```sql
SELECT b.total_cost + b.extra_costs                       AS landed_cost,
       b.allocation_variance,
       SUM(bis.received_qty) units_received, SUM(bis.sold_qty) units_sold,
       SUM(bis.written_off_qty) units_written_off, SUM(bis.on_hand) units_remaining,
       SUM(bis.sold_revenue)
         + (SELECT COALESCE(SUM(oi.line_net),0) FROM order_items oi JOIN orders o ON o.id = oi.order_id
            WHERE oi.batch_id = b.id AND oi.is_custom = 1 AND o.status = 'completed') AS revenue_net,
       SUM(bis.sold_cost) cogs, SUM(bis.written_off_cost) written_off_cost,
       SUM(MAX(bis.on_hand,0) * bi.unit_cost) remaining_value_cost,
       SUM(MAX(bis.on_hand,0) * p.price)      remaining_value_retail
FROM batches b
JOIN batch_items bi ON bi.batch_id = b.id AND bi.deleted_at IS NULL
JOIN products p ON p.id = bi.product_id
LEFT JOIN batch_item_stock bis ON bis.batch_item_id = bi.id
WHERE b.id = :batch_id
GROUP BY b.id;
-- Rust derives realised_profit, payback, projected_profit, sell_through from these columns.
```

**Block 2.7 implementation notes (reports engine, `db::repo::reports`):**
- **Periods** are `domain::period` (§5.2), each resolved to inclusive business dates, with the previous equivalent period on request (all time has none).
- **Counter filter.** It narrows the sales and payment metrics. Allocation variance, write-offs and expenses belong to the store, so they stay store-wide on a counter's report.
- **Permissions.** Every report needs `REPORT_SALES`. COGS, profit, margin, write-offs, expenses and per-row cost come back empty (`null`) unless the viewer also has `REPORT_FINANCIAL` **and** `VIEW_COST`.
- **Trend** (for the §1.4 charts) buckets the rollups and expenses by day, week (store week start) or month. Its buckets sum to the period. The allocation variance stays in the summary, because it is a batch-level figure.
- **Sales by product / category / batch.** Each set of rows sums to the period's net sales and sale cost. Quick sales are one "Quick sales" row by product, and products without a category are one "Uncategorised" row. By batch, revenue and cost come from the sale slices (plus quick sales tagged to a batch). Everything else (untracked products, untagged quick sales) is one "No batch layer" row.
- **Voids** are grouped by reason and by approver: the supervisor who approved, or the voider when their own role holds `VOID_ORDER`.
- **`rebuild_derived`** (Settings → Maintenance, `SETTINGS_MANAGE`, audited `SETTINGS_CHANGE`) recomputes the tables below in one transaction:
  - `daily_sales_summary` and `daily_payment_summary`
  - `customer_stats` and `quick_sale_stats`
  - `stock_levels` and `batch_item_stock`
  - both product FTS indexes

  `tests/reports.rs` checks on randomised data that the triggers and a rebuild agree row for row, and that monthly, weekly, yearly, custom and all-time reports equal the sum of their days. The only difference a rebuild makes is to drop all-zero payment-summary rows, left behind when a method's only sale was voided.

**Report permissions:** `REPORT_SALES` shows revenue, payments and Z-reports. `REPORT_FINANCIAL` also shows COGS, profit, valuation and batch P&L. Cost columns anywhere require `VIEW_COST`. Exporting to CSV or PDF needs `DATA_EXPORT` and is audited.

---

## 6. Sync protocol (details)

### 6.1 Push
1. The worker selects up to 200 outbox rows where `status IN ('pending','failed') AND next_attempt_at <= now` and orders them by `seq`. It marks them `in_flight`.
2. It calls `rpc/sync_push({ device_id, ops: [{op_id, table, op, row}] })`.
3. The server processes the batch in **one Postgres transaction that first takes `pg_advisory_xact_lock(hashtext(store_id::text))`**. This serialises all writers of a store, so `server_seq` assignment order equals commit order.
   - Without the lock, a puller could see seq 105 committed while 103 is still in flight, advance its cursor past 103, and never receive that row.
   - At 2–3 counters, the lock costs milliseconds.
   - Any other server-side writer, such as a future dashboard, must take the same lock. If writer volume ever grows, switch to a "safe watermark" pull.

   For each op (each in its own subtransaction, so one bad op never undoes the batch):
   - If the `op_id` is already in `processed_ops`, return its recorded result. This is a replay.
   - Otherwise validate it: run the table's merge rule (§6.3) and the invariants (owner invariant, batch cost lock, order immutability, audit chain continuity). Then apply it, record the `op_id`, and return `ok` or `rejected{reason}`. Rejections are recorded too, so a replayed rejected op is rejected again.
   - A row whose parent hasn't reached the cloud yet (a foreign-key miss) returns `retry`. It isn't recorded, so it can succeed later.
   - Only `upsert` ops exist: deletes are soft (`deleted_at`).
4. The client deletes `ok` ops. A `rejected` op becomes `dead`, gets a `sync_logs` entry, and is surfaced in Settings → Sync (`SYNC_MANAGE`). A `retry`, a network failure or a 5xx makes the op `failed` with `attempts+1` and a backoff.
5. Parents go before children, by enqueue order, which also keeps one device's changes in causal order: a layer's cost is set before its batch is marked received. Checkout writes an order's lines, payments and slices before it can seal the order, so after sealing it moves their queued ops behind the sealed order's (same snapshots, new op ids). A batch boundary can still split a parent from its child; the child is answered `retry` and goes in the next round.

### 6.2 Pull
- `rpc/sync_pull({ cursor, max_rows, include_own: false })` uses **one global cursor**. It returns `{ rows: [{ seq, table, row }], cursor, has_more, watermark }`. `server_seq` is a single store-wide sequence, so per-table cursors would add nothing but ordering hazards. It returns rows across all tables with `server_seq > cursor`, ordered by `server_seq`, **excluding this device's own writes**, which it already has:
  - Mutable tables and `orders`: `updated_by_device = me`.
  - Insert-only tables: `device_id = me`.

  Rows the server modified carry the SYSTEM device ID, so they always come back.
- **Cursor and watermark.** Before reading, the server takes the store's newest `server_seq` (the watermark) and reads only up to it. Every row at or below it is committed, because pushes are serialised per store. A full page's cursor is its last row's seq. The last page's cursor jumps to the watermark, past this device's own newest rows, which it never receives.
- **Parent closure.** Ordering by `server_seq` alone can't guarantee parents arrive before children:
  - A page limit can cut between a parent and its child.
  - A mutable parent gets a *new* `server_seq` on every edit. A `batch_item` created at seq 100 and edited at seq 200 sorts after the movement at seq 150 that references it.

  So the server adds to each page **every ancestor row with `server_seq > cursor`**, following the foreign keys: `order_items → orders/products/batches`, `stock_movements → batch_items → batches/products`, `order_payments → orders`, `orders → cash_sessions`, and so on. Those ancestors are applied again later at their own seq, which is harmless because applying is idempotent.
- **Applying a page** is one transaction, taken through the single-writer lock:
  - It starts with `UPDATE sync_ctx SET applying = 1` and ends with `UPDATE sync_ctx SET applying = 0` just before `COMMIT`. A rollback restores 0 automatically, because it's a table row. While the flag is set, **policy** triggers stand down. **Structural** and **derived** triggers keep running, and pulled orders arrive already `completed` or `voided`, never `pending` (§4.1).
  - It runs with `PRAGMA defer_foreign_keys = ON`, so order within the page doesn't matter.
  - It applies tables in topological order, so the derived triggers find their parents.
  - It advances the cursor to the page's max seq.
- **Apply rules.** A generic upsert would trip the immutability triggers.
  - Insert-only tables use `INSERT OR IGNORE`.
  - `orders` uses `INSERT OR IGNORE`, then `UPDATE … SET status='voided', …` **only if** local is `completed` and remote is `voided`.
  - Mutable tables use an upsert guarded by LWW (`WHERE excluded.updated_at > updated_at OR (= AND excluded.updated_by_device > updated_by_device)`).
- Applying pulled rows **never** enqueues to the outbox (no echo loop) and **never** writes audit rows. Pulled audit rows are inserted as they are.
- **Bootstrap** of a new or recovered terminal is the same pull with `cursor = 0` and `include_own = true` (a recovered terminal needs its own past writes back), repeated until it's caught up. It ends with `rebuild_derived` as cheap insurance (Block 3.5).
  - Every page commits with its cursor, so an interrupted bootstrap resumes where it stopped.
  - A joined counter carries `sync.bootstrap_pending` until the pull completes. Until then the app shows "Finishing setup" instead of the login screen, and the worker leaves the pull to the bootstrap.
  - Progress (`sync://bootstrap`) is the cursor's way towards the page's `watermark`.
- **Every terminal pulls everything**, all history included. Store-wide reports and customer history at any counter depend on it. Bootstrapping a new terminal pulls the full dataset in pages, with a progress UI. At ~1,000 lines a day across three counters, that's roughly 1M rows over three years, which SQLite handles comfortably.

### 6.3 Merge rules per table

| Table | Rule |
|---|---|
| `order_items`, `order_payments`, `stock_movements`, `cash_movements`, `audit_logs` | **Insert-only.** Same `id` ⇒ no-op. Conflicts are impossible by construction. |
| `orders` | Insert-only. The only update is `completed → voided`, which only the originating device can make (voids are same-session). |
| `cash_sessions` | Only the originating device writes, except `CASH_SESSION_FORCE_CLOSE` from another terminal (audited). LWW. |
| `products`, `categories`, `roles`, `staff`, `expenses`, `expense_categories`, `store_settings`, `quick_sale_links` | **Last-write-wins on the whole row** by `(updated_at, updated_by_device)`. On an exact tie from the *same* device (a row written twice within one millisecond, e.g. a batch created and received in one transaction), the later snapshot wins: one device's changes always arrive in order. |
| `batches`, `batch_items` | LWW while `draft`. Once received, the server **rejects** any change to `unit_cost` or `qty_received` (dead op). Status moves forward only, except an audited reopen when there's been no movement. |
| Any refused change to a mutable row | The server re-stamps its own current row (new `server_seq`), so every counter pulls it, and the counter whose change was refused lets the cloud's version replace it. The refused op stays `dead` in Settings → Sync. A counter never keeps a row the cloud rejected (Block 3.6). |
| `staff` / `roles` owner invariant | A merged change that would leave no active Owner is rejected (`OWNER_INVARIANT`) and surfaced to the owner. Only a change that *removes* the last active Owner counts: while a store's seeds are first arriving there is none yet. |
| `audit_logs` chain | The server checks `prev_hash` against the device's `last_audit_hash`. A mismatch is still **accepted**, but flagged (`devices.chain_broken_at`) and shown on the Audit screen. Losing audit rows would be worse than storing a broken chain. |
| Barcode collision (two devices registered the same barcode offline) | It's never a rejection, because a sale must never fail over this. **Server:** before upserting an active product, check for another live, active product with that barcode. If one exists, store the incoming row with `is_active=false`, `barcode_conflict=true`, `updated_at=now()` and `updated_by_device=SYSTEM`, and return `ok`. **Client pull:** before applying a pulled active product, deactivate any *other* local active product with that barcode in the same transaction (`barcode_conflict=1` plus an outbox upsert). The partial unique index can't fail the pull. The owner resolves it in "Barcode conflicts" (`BARCODE_CONFLICT_RESOLVE`). |
| `quick_sale_links` | The deterministic ID means concurrent promotions of the same key converge on one row. If two managers created *different* products for it, the loser's product stays, and the review screen flags "duplicate product from promotion". |
| Negative stock after merge | Allowed. Reported in "Oversold layers". The owner fixes it with a `count_correction` movement. |
| `customers` | Deterministic id `UUIDv5(store_id, phone_e164)`, so two counters registering the same number offline converge on one row (LWW for name and note). An erasure (`erased_at` set) wins over any concurrent edit: the server and the pull both refuse to re-populate an erased row, except through an explicit re-activation by Rust. |

### 6.4 Receipt numbers offline
The format is `{device_code}-{YYYYMMDD}-{seq:04}`, e.g. `T01-20261108-0042`. The date is the business date. `device_code` (T01–T03) is assigned at first run or registration. `seq` comes from `receipt_counters` and is incremented inside the checkout transaction. Receipts print a QR code of the order `id` (for reprint and void lookup during the session), plus the `receipt.footer` policy text.


### 6.5 Device registration (Block 3.3)
- **The owner's cloud account** is one email + password user created in the Supabase dashboard (Authentication → Users). Public sign-up is off (decision #19). It owns at most one store (`stores.owner_user_id`).
- **Registering a counter.** In Settings → Sync (`SYNC_MANAGE`), the owner enters the server address, the project's public (anon) key, and their cloud email and password.
  - The counter signs the owner in directly with GoTrue's password grant. The password goes nowhere else and is never stored.
  - The counter calls `POST /functions/v1/register-device` with the owner's short-lived token. The function verifies the token, refuses device identities, and calls `admin_claim_device`:
    - **Adopt** (the counter already has a store from first run): it sends its `store_id`, `device_id` and code. The first registration claims the store for the account. Another account can't claim it, and an account can't claim a second store.
    - **Join** (decision #18): a new counter sends only its fresh `device_id` and gets the next free code, T01–T09.
    - Re-registering keeps the device row and code, and replaces the identity's credentials.
  - The function creates (or re-keys) the counter's **device identity**: an auth user `{device_id}@devices.gayanshop.invalid` with `app_metadata = {role: 'device', store_id, device_id, device_code}`. It then signs that identity in and returns its tokens. The random password never leaves the function.
  - The counter checks that the server answered for **its own** ids (`SYNC_IDENTITY_MISMATCH` otherwise; ids are never reassigned). It stores the refresh token in the Windows Credential Manager (service `com.gayanshop.pos`, account `sync.refresh_token.{device_id}`), saves the server address and anon key in `device_settings` (`sync.server`), and audits `DEVICE_REGISTER`.
- **Tokens.** The worker refreshes the access token with the refresh-token grant a minute before it expires. GoTrue rotates refresh tokens, so the new one is written to the keyring before the access token is used.
- **Portability.** The function speaks plain HTTP to GoTrue (`/auth/v1`) and PostgREST (`/rest/v1`), with no client library. The service-role key exists only in the function's environment, never in the desktop binary.

---

## 7. Hardware integration

### 7.1 Barcode scanners
- **Default: USB HID keyboard wedge.** The race-free capture design (stress-tested in §13.3):
  - **Configure every scanner with a prefix and a suffix.** Use the prefix `F13` (a key no keyboard has; most scanners can send it), or `Ctrl+Shift+F12` if a model can't, and the suffix `Enter`. With a prefix, capture is **deterministic**: the prefix opens capture mode before the first digit arrives, so nothing can leak into a focused input.
  - **Fallback for unconfigured scanners:** a timing heuristic, where keys arriving ≤ 30 ms apart, ≥ 6 characters long and ended by `Enter` count as a scan.
    - From the second fast key onwards the provider calls `preventDefault()` provisionally and buffers.
    - If the burst ends without `Enter`, or the gap exceeds 30 ms, it's treated as human typing: the buffered characters are **replayed** into the focused input with `setRangeText`.
    - If it is a scan, the one leaked first character is removed from the focused input.
  - **Layout-independent decoding:** decode from `KeyboardEvent.code` (`Digit7`, `KeyA`, `Minus`…), **never** from `event.key`. A HID wedge types through the active Windows keyboard layout, so with a Sinhala layout `event.key` would give Sinhala characters. Caps Lock and Shift state are applied explicitly.
  - **The scan's `Enter` is swallowed** (`preventDefault` + `stopPropagation` in the capture phase), so it can never submit a form, press a focused **Pay** button, or confirm a dialog.
  - **While capture is open, F-key shortcuts are suspended.** The capture listener is registered in the **capture phase on `window`** ahead of every other handler.
  - **Ordered processing:** decoded codes go into a FIFO queue drained by one async worker. That preserves scan order even when barcode lookups resolve out of order. Repeated scans of the same code apply as one functional Zustand update (`qty + 1`), so two fast scans can't overwrite each other.
  - IME composition events (`isComposing`) are ignored by the scanner and passed through untouched.
- On the till:
  - A known barcode adds the item, and repeated scans increment qty.
  - A scanned receipt QR opens that order. Reprint and void are offered only if the order's session is still open.
  - An unknown barcode plays the **error tone**, flashes the scan field red, and shows a non-blocking toast: "Not found: 2000123456781". It then opens the Quick Sale bar pre-filled with the barcode as a note, with a "Register product" shortcut for `PRODUCT_MANAGE`. There's never a modal, so the next scan still works.
- Serial (COM) scanners are supported through the optional `hardware/scanner_serial.rs`, which emits `scanner://scan`.

### 7.2 Thermal receipt printers (ESC/POS, generic)
The models aren't purchased yet, so everything targets **generic ESC/POS**, with per-printer profiles in `device_settings`:
- Paper width: 58 mm (32 columns) or 80 mm (48 columns), configurable since fonts vary.
- Code page (default PC437 / WPC1252), cut mode (full/partial/none) and feed lines before cut.
- QR support (`GS ( k`), with a fallback to a Code128 receipt barcode.
- Transport:
  1. **Windows spooler, RAW datatype** (`OpenPrinterW` / `StartDocPrinterW("RAW")` / `WritePrinter` via the `windows` crate). Best for USB printers with a vendor or generic driver.
  2. **Network**: TCP 9100.
  3. **Serial**: COM port.

A **"Print test page"** in Settings prints the column ruler, code page sample, QR and cut, then kicks the drawer. It qualifies a new printer in under a minute.

Sinhala/Tamil text: most printers lack those code pages. Render such lines as a raster bitmap (`GS v 0`) in Phase 5. Receipts are English-first until then.

### 7.3 Cash drawer (confirmed: RJ11 to the receipt printer)
- Kick: `ESC p m t1 t2` → `1B 70 00 19 FA` (pin 2, 50 ms on, 500 ms off). Pin 5 (`m = 01`) is configurable for drawers wired that way.
- **Automatic kick** happens after a completed order with a cash payment. It's covered by the order record, with no separate audit row. A **void** of a cash order also kicks (covered by its `VOID_ORDER` audit row), because the cashier hands back the cash just taken.
- **No-sale kick** requires `DRAWER_NO_SALE` (or supervisor approval) and an open cash session. One transaction writes the `cash_movements` row (`no_sale`, with a reason) and audits `DRAWER_KICK`. The kick is sent after commit.
- Pay-in and pay-out also kick, and are audited.
- The drawer state sensor (`DLE EOT`) isn't relied on, because generic printers report it inconsistently.

### 7.4 Barcode label printers: TSPL / ZPL, multi-column rolls

**Printers and languages.**
- `hardware/label` emits two printer languages from one layout engine, selected by the `LabelLanguage` enum:
  - **TSPL**, the default (Xprinter, TSC and most generic thermal label printers)
  - **ZPL** (Zebra and compatibles)
- Both use the same transports as receipts (§7.2).
- The printer profile in `device_settings` holds the language, DPI (203 or 300), darkness, speed, and the per-device calibration offsets `offset_x_mm` / `offset_y_mm`.

**Two built-in roll layouts** (owner requirement):

| Template id | Layout | Label size | Default gaps | Typical roll width | Use |
|---|---|---|---|---|---|
| `two_up_2x1` | **2 labels per row** | **2″ × 1″ = 50.8 × 25.4 mm** | **row (die-cut) gap 2.0 mm**, column gap 2.0 mm, left margin 1.5 mm, **203 dpi** | ≈ 106.6 mm (≈ 4.2″) | General merchandise: shoes, plasticware, clothing |
| `three_up_1x1` | **3 labels per row** | **1″ × 1″ = 25.4 × 25.4 mm** | **row (die-cut) gap 2.0 mm**, column gap 2.0 mm, left margin 1.5 mm, **203 dpi** | ≈ 83.2 mm (≈ 3.3″) | Small items: jewellery, accessories |

**The defaults are owner-confirmed: 203 dpi and the standard 2 mm gap.**
- The row gap goes straight into TSPL `GAP 2 mm,0` and the ZPL label length.
- The column gap and left margin vary between roll suppliers. They default to 2.0 mm and 1.5 mm, and are corrected per device in Settings → Labels.
- An **alignment test** prints the outline of every cell, which makes calibration take a minute.

**Printable elements** (each template positions these, in mm relative to one label's top-left corner):

| Element | Source | Rules |
|---|---|---|
| Store name | `store_settings` `store.name` → **"GayanShop"** | Small, centred, single line |
| Product name | `products.name` (Latin; `name_si` is never printed) | Up to 2 lines on both label sizes, with word wrap then ellipsis. Characters outside the printer's built-in font are replaced with `?`, and the preview warns. |
| Attributes / size | `products.attributes`: `size` first, then the others, joined with ` · ` | One line, truncated with an ellipsis |
| Selling price | `products.price` → **`Rs. 1,250`**, or `Rs. 1,250.50` when there are cents | Bold, the largest text on the label |
| Barcode | `products.barcode`, else `products.sku` | Code128 or EAN-13 (rules below), with a human-readable line under the bars |

**Reference layouts** (mm, before calibration offsets):

```
2" x 1" (50.8 x 25.4 mm)                       1" x 1" (25.4 x 25.4 mm)
+----------------------------------------+     +---------------------+
|               GayanShop                |     |      GayanShop      |
| Ladies Sandal Leather                  |     | Silver Ear Stud     |
| Comfort Fit                            |     | Round 6mm           |
| Size 38 · Black          Rs. 1,250     |     | Size 6mm            |
| ||| || ||| | || ||| || | ||| || ||| || |     |     Rs. 450         |
|          2000123456781                 |     | ||| || ||| | || ||  |
+----------------------------------------+     |       104523        |
                                               +---------------------+

2" x 1" element boxes (x, y, w, h in mm; f = built-in font step)
  store_name    2.0  1.5  46.8  2.5   f1, centre
  product_name  2.0  4.5  46.8  6.0   f2, bold, 2 lines
  attributes    2.0 11.0  28.0  2.5   f1
  price        30.0 10.5  18.8  4.0   f3, bold, right
  barcode       3.0 15.0  44.8  8.5   bars 6.5 + text 2.0

1" x 1" element boxes
  store_name    1.0  1.0  23.4  2.0   f1, centre
  product_name  1.0  3.5  23.4  5.0   f1, 2 lines
  attributes    1.0  9.0  23.4  2.2   f1
  price         1.0 11.5  23.4  3.5   f2, bold, centre
  barcode       1.0 15.5  23.4  9.0   bars 7.0 + text 2.0
```

**Barcode symbology and size rules.** These are computed in `label/layout.rs`, never guessed:
- The narrow bar (module) must be **≥ 2 printer dots**. At 203 dpi, 1 dot = 0.125 mm, which is below what most CCD scanners read reliably.
- The **quiet zone is ≥ 10 modules** on each side.
- `symbology: 'auto'`: **EAN-13** when the code is a valid 13-digit EAN/UPC, otherwise **Code128** (auto subset, so numeric runs use subset C).

| Template | What fits at 203 dpi | Consequence |
|---|---|---|
| 2″×1″ (≈ 44.8 mm usable) | EAN-13 at a **3-dot** module (0.375 mm) is 113 modules ≈ 42.4 mm. Code128 for a 13-digit code at 2 dots ≈ 143 modules ≈ 35.8 mm. | Every code the shop uses fits. |
| 1″×1″ (≈ 23.4 mm usable) | EAN-13 doesn't fit at any scannable size. Code128 at 2 dots fits **≤ 6 numeric digits** (68 + 20 quiet modules = 88 × 0.25 mm = 22.0 mm). | **3-up labels print the product's 6-digit in-store short code (`sku`) as Code128-C.** A 300 dpi printer allows up to 8 digits. |

- **In-store codes, generated in Rust and unique offline:**
  - Barcodes use the EAN-13 prefix `2` plus a store/device-unique number plus a check digit (for 2-up labels).
  - SKUs are a 6-digit `{device_digit}{5-digit sequence}`, e.g. `104523` from T01. That gives each counter 100,000 codes, with no cross-counter collisions while offline.
- **The scanner lookup** checks `barcode` first, then `sku`, so both kinds of label scan at the till.
- If a product's code can't meet the rules on the chosen template, the preview shows a warning such as "barcode too long for 1″×1″; assign a short SKU". Printing is still allowed, but flagged.

**How a multi-column job prints:**
- `print_labels({ template_id, items: [{product_id, copies}] })` expands copies into cells. The cells fill rows left to right. A partial last row leaves the remaining cells blank, or optionally fills them with copies of the last item.
- **TSPL**, per row:
  - The setup is `SIZE {roll_width} mm,{label_h} mm` / `GAP {row_gap} mm,0` / `DIRECTION 1` / `REFERENCE 0,0`.
  - Each row is `CLS`, then for each column `TEXT` / `BARCODE` (`"128"` or `"EAN13"`) at `x = left_margin + col × (label_w + col_gap) + offset_x`, then `PRINT 1`.
  - Consecutive identical rows collapse into `PRINT n`.
- **ZPL**, per row: `^XA ^PW{roll_width_dots} ^LL{label_h_dots} ^LH0,0`, then the fields per column via `^FO` / `^A0` / `^BY{module}` / `^BC` or `^BE`, then `^PQ{n}` for identical rows, then `^XZ`.
- Layout is in **mm**, and the conversion to dots happens only at emit time: `dots = round(mm × dpi / 25.4)`. Both DPIs therefore share one template.
- **WYSIWYG preview:** Rust returns the computed `LabelPreview` (rows, cells, positioned texts, warnings). The UI renders it as SVG at scale, from the same data the emitters use.
- **Templates** are device-local JSON in `device_settings` (`label.templates`), because each counter's printer and roll can differ. The two built-ins can be edited but not deleted, and "Reset to default" restores them.
- **Bulk flows:**
  - Receive a batch → "Print labels". The copies default to `qty_received`, and the template choice is remembered per category.
  - Product list → select rows → "Print labels".
  - Each job is audited once as `LABEL_PRINT` (template, product count, label count) and requires `LABEL_PRINT`.

**Block 2.2 implementation notes (`hardware/label/`).**
- `barcode.rs` encodes Code128 (subset C for all-digit codes, with a final CODE B for an odd last digit; else B) and EAN-13, so the fit rules count real modules and the SVG preview draws the real bars. `layout.rs` is the only module allowed float arithmetic (mm geometry and `dots = round(mm × dpi / 25.4)`); it never sees money.
- Code choice per label: the first candidate (barcode, then SKU) that fits the box with ≥ 10 quiet modules each side at a 2–4-dot module (the largest that fits). If none fits, the first is printed at 2 dots with a `BARCODE_TOO_LONG` warning. Other warnings: `NO_CODE`, `TEXT_NOT_PRINTABLE` (anything outside printable ASCII prints as `?`; `"` becomes `'` and `^` / `~` become `?` because they are TSPL / ZPL syntax) and `TEXT_TRUNCATED`.
- Attributes are joined with ` - ` rather than ` · `, because the printers' built-in fonts have no middle dot.
- TSPL has no bold attribute, so a bold line is drawn twice, one dot apart. ZPL sets the darkness with `~SD` (0–30, twice the stored TSPL `DENSITY` 0–15).
- The label printer has its own `print_jobs` queue (`printer_key = 'label'`) and its own print worker, so a jammed label printer never holds up receipts. The worker never sends it ESC/POS `DLE EOT`. Its profile lives in `device_settings` `printer.label`; templates live in `label.templates` and are calibrated per counter (gaps and margin; "Reset to default" restores the built-in).
- `LABEL_PRINT` is approvable, so the print dialog asks for a supervisor's approval bound to the target `labels`. The preview itself needs only a login. The last template used is remembered per counter rather than per category (batch lines carry no category).
- Golden TSPL / ZPL byte snapshots live in `src-tauri/tests/fixtures/labels/` (`UPDATE_FIXTURES=1 cargo test --test labels`, then review the bytes).

### 7.5 Hardware abstraction
```rust
pub trait PrinterTransport: Send + Sync { fn send(&self, bytes: &[u8]) -> Result<(), AppError>; }
pub enum PrinterKind { Receipt { width: PaperWidth, codepage: CodePage }, Label { lang: LabelLanguage, dpi: u16, offset_mm: (f32, f32) } }
pub fn layout(t: &LabelTemplate, cells: &[LabelCell]) -> LabelPreview;              // one engine for preview + print
pub fn emit(t: &LabelTemplate, p: &LabelPreview, lang: LabelLanguage, dpi: u16) -> Vec<u8>; // TSPL or ZPL bytes
```
**Printing is decoupled from the sale (stress-tested in §13.3):**
1. **The job is created in the sale's own transaction.** `checkout_order` inserts a `print_jobs` row (`queued`) together with the order, so the job exists exactly when the sale exists. A crash right after the commit can't lose it, and a rolled-back sale can't print.
2. **After `COMMIT`**, the command returns to the UI immediately ("Sale complete ✓") and pings the print worker over an in-memory channel. **The sale never awaits the printer.**
3. **The print worker** is one tokio task per printer, with **at most one in-flight job per printer**.
   - Network or serial I/O uses explicit timeouts: connect 2 s, write 5 s.
   - The Windows spooler `WritePrinter` can block indefinitely on a frozen spooler, so every transport call runs on its own OS thread (`std::thread`, not `spawn_blocking`, so an abandoned call can never hold up runtime shutdown when the app exits) under a 5 s `tokio::time::timeout`.
   - On a timeout, the printer is marked **unhealthy**: its blocked thread is abandoned, since it can't be killed, and no new thread is started for that printer until a health probe succeeds. Threads never pile up.
4. **Paper-out and errors.** Where the transport supports it (network and serial), the worker queries real-time status before each job: `DLE EOT 4` for the paper sensor, `DLE EOT 2` for the error state. Paper-out or cover-open sets the job, and every other job waiting for that printer, to `failed` with a reason code (no paper is wasted and nothing is sent), and the worker idles until Retry or a new job.
   - The till shows a **persistent banner**: "Printer: paper out — 2 receipts waiting [Retry] [Reprint later]".
   - Selling continues.
5. **Retries** re-render from the stored payload, with exponential backoff (2 s → 30 s) up to 5 attempts, then `failed`. A job is `abandoned` only when a user dismisses it, and that's audited.
6. **On startup**, any `queued`, `printing` or `failed` jobs from before a crash are listed: "3 receipts from before the restart were not printed — Print now / Skip". There are no silent reprints, which avoids duplicate paper receipts.
7. **Cash drawer:** the kick is its own `drawer_kick` job, sent **before** the receipt bytes on the same printer. If the printer is down, the banner tells the cashier to use the drawer key. The sale is already recorded.

Each of the 2–3 counters has its own printer and drawer profile in `device_settings` (not synced).

**Data folder override.** `GAYANSHOP_DATA_DIR` points the app at another data folder (it holds `pos.db` and `backups\`); the default is `%APPDATA%\com.gayanshop.pos`. The Block 1.12 e2e suite (`pnpm e2e`, WebdriverIO + `tauri-driver`) uses it so a test run never touches the shop's database.

---

## 8. Phased execution plan

Each phase ends with **exit criteria**. Don't start the next phase until they pass. RBAC guards and audit writes are built **from the start**, because retrofitting them across every command later is painful.

> **Execution detail lives in [`AGENT.md`](../AGENT.md) §5: the canonical Atomic Block roadmap.** Each block lists its deliverables, its verification gate, and a status checkbox. Every block that passes its gate gets its own git commit, `feat(block-X.Y): …`. This section keeps the phase scope and exit criteria; `AGENT.md` holds the step-by-step blocks. If the two disagree, fix `AGENT.md` to match this document, then continue.

### Phase 0: Foundations (≈ 1.5 weeks) — ⏸ ON HOLD (owner request)
- Toolchain and scaffold (§9). Tailwind v4 and shadcn init. Tauri plugins: `single-instance`, `log`, `window-state`, `dialog`.
- **Rust core:**
  - `AppError` (including `Forbidden` / `ApprovalRequired`), as stable codes that the UI translates.
  - `AppState`, and the sqlx pool with PRAGMAs.
  - The **full `0001_init.sql` regenerated from §4.3 (v4)**.
  - The tauri-specta export (per-field `specta_typescript::Number` for `i64`, §1.5).
  - Writes go through one writer, a tokio `Mutex` around write transactions. That keeps the audit `device_seq` gapless and avoids SQLITE_BUSY.
- **`domain`:** money, pricing and discount, payments (no negatives), business_date, allocation, fifo, phone, receipt_no and custom_key, each with unit and property tests.
- **First-run wizard:**
  - Creates the store, owner PIN, currency, timezone, day cutoff, week start and default UI language.
  - Generates `store_id`, `device_id` and `device_code` and stores them in `device_settings`. Phase 3 registration **adopts** these IDs and must not reassign them.
  - A second or third terminal instead gets the existing `store_id` and a unique `device_code` *before* it records any sale.
  - Seeds roles, system batches, expense categories and label templates.
- **RBAC core:** argon2id PINs, lockout, login and lock screen, `guard::require`, the approval tokens, and the `<Can>` / `ApprovalDialog` UI.
- **Audit core:** `audit::record` with the per-device hash chain, the append-only triggers, and a minimal viewer.
- **UI foundations:**
  - i18n bootstrapped (`en` + `si` dictionaries, Noto Sans Sinhala bundled, a key-parity CI check, LanguageToggle).
  - The shared `DataTable` (TanStack Table v8 + Virtual).
  - The `size="pos"` 48 px variants.
  - The `minWidth/minHeight` 1024×768 window.
- Tooling: ESLint, Prettier, `clippy -D warnings`, vitest, cargo test, and CI on `windows-latest`.

**Exit:**
- The app launches at 1024×768 and 1920×1080, migrations run, and the wizard completes in both English and Sinhala.
- PIN login works offline. A cashier calling a manager-only command gets `Forbidden`, and a supervisor approval token lets it through once.
- Every test command writes its audit row in the same transaction, and the chain verifies.
- CI is green, including the i18n key-parity check.

### Phase 1: Offline MVP till (≈ 4 weeks)
- Products: CRUD (with optional `name_si`), FTS search (English and Sinhala names), barcodes and in-store SKUs.
- **Minimal batches:**
  - Create a draft, add items, enter manual costs, and receive.
  - `OPENING-STOCK` entry for go-live, and ad-hoc one-item batches.
  - FIFO consumption at checkout.
- **Cash sessions:** open with float, pay-in and pay-out, close with count and variance, and the **Z-report** print (including voids by reason and approver).
- **POS screen**, touch- and keyboard-first and laid out for 1024×768 → 1080p:
  - Scan or search → cart.
  - Qty edit, price override and line and bill discounts (limits plus approvals, §3.7).
  - **Quick Sale bar (F2 → type → Enter)** with the live parse preview.
  - **Customer phone (F4 / Numpad `/`)**, optional, with known-customer lookup (§3.8).
  - Payment: cash with change, card, transfer, QR, split.
- **`checkout_order(checkout_token, …)` is one transaction**, using the seal pattern (§13.5). In order:
  1. An idempotency check: if an order with this `checkout_token` exists, return it unchanged.
  2. Resolve or create the customer. This must happen **before** the order insert: `orders.customer_id` is a foreign key checked immediately, and the state machine freezes it after insert.
  3. Bump the receipt counter, for the same reason: `receipt_no` is frozen after insert.
  4. Insert the order as `pending`, with the customer and receipt number already set.
  5. Insert the items, payments (all ≥ 0) and FIFO stock slices with `revenue_net`.
  6. **Seal** it (`pending → completed`), which makes SQL re-verify every total.
  7. Write audit rows for any override or above-limit discount.
  8. Delete the `held_carts` `__current__` row.
  9. Insert the `print_jobs` receipt row (plus a drawer kick for cash).
  10. Write **outbox rows** for everything.
- **Block 1.4 implementation notes (`db/repo/checkout.rs`, `commands/checkout.rs`):**
  - Rust prices and validates the whole cart before writing anything, and refuses what the seal would refuse with a translatable error. The SQL seal stays the second line of defence.
  - One approval covers the order. The first attempt fails with `ApprovalRequired { perm }`, where perm is `PRICE_OVERRIDE` before `ORDER_DISCOUNT` before `LINE_DISCOUNT`. The UI gets an approval bound to (perm, `checkout_token`, cashier) and retries. The approver must be a different, active staff member whose own permissions and limits cover every line, otherwise `APPROVER_LIMIT_TOO_LOW`. The approver is stored on the order header and on each overridden or discounted line.
  - Selling below `min_price` always needs someone else's approval, even when the seller holds `PRICE_OVERRIDE`.
  - Audit: `PRICE_OVERRIDE` per overridden line and `LINE_DISCOUNT` / `ORDER_DISCOUNT` for approved discounts. A refused attempt writes nothing; the approval request itself is audited (`APPROVAL_GRANTED` / `APPROVAL_DENIED`).
  - Untracked catalog lines cost `default_cost × qty` (cost unknown if 0). Quick-sale lines cost what the cashier typed, or unknown.
  - Receipt and drawer-kick print jobs use the `receipt` printer profile. The outbox row for the order is written after the seal, so sync always ships the completed order.
  - `tests/fixtures/pricing_cases.json` holds golden carts priced by Rust; the till's TypeScript preview (Block 1.9) is tested against it.
- **Same-session void** (§3.3):
  - Whole order only, with a reason from the fixed list.
  - A cashier needs supervisor approval.
  - Exact stock reversal, a VOID slip, and a drawer kick for cash orders.
  - Refused once the session is closed.
- ESC/POS receipt with QR (spooler, network), the "All sales are final" footer, the masked customer phone, the automatic drawer kick, and an audited **no-sale kick**. Printer test page.
- Hold and recall, and debounced crash recovery of the current cart.
  - **Block 1.9 notes:** the cart is a Zustand store (`features/pos/store/cart.store.ts`) whose `held_carts` payload is versioned JSON (`v: 1`, validated with zod on recall). Commands `cart_save_current` (500 ms debounce), `cart_current` ("Recover cart?"; never offers a token that was already sold), `cart_hold` (F8, at most 50), `held_carts` and `cart_recall` (F11, holding the cart in progress first). The till previews totals with `features/pos/lib/pricing.ts`, a BigInt-exact port of `domain::pricing`, tested against `pricing_cases.json`.
- **Manual "Backup Database Now"** (§3.9) to USB or a chosen folder, with an integrity check, manifest and audit. It ships in Phase 1 because, until Phase 3 sync, it's the only off-machine copy.
- Staff management (create staff, assign role, reset PIN, set UI language) for the Owner.
- The till's Sinhala translation is complete.

**Exit:**
- With the network unplugged, a cashier completes 50 mixed sales (scanned plus F2 quick sales, some with a customer phone) and the following all work:
  - A quick sale takes F2, typing and Enter, with no mouse. A phone number takes F4, typing and Enter.
  - A cashier's price override prompts for supervisor approval and is audited.
  - A cashier's void prompts for supervisor approval, restores stock to the exact layers, and drops out of the Z-report totals. After the session closes, the same void is refused for every role.
  - Any attempt to record a negative payment is rejected by Rust (and by the SQL `CHECK`).
  - A 6% bill discount by a cashier prompts for approval. A 5% one is accepted, and its shares sum exactly to the bill discount.
  - The Z-report's expected cash equals the counted cash in the test script.
  - Every till screen works by touch only at 1024×768 (all targets ≥ 48 px) and by keyboard only, in both languages.
- "Backup Database Now" to a USB stick completes while a second cashier keeps selling. The copy passes `integrity_check`. Pulling the stick mid-backup leaves no `.partial` file and logs `BACKUP_FAILED`.
- Killing the process mid-checkout never leaves a partial order.
- Rust totals match the UI preview in property tests.

### Phase 2: Batches, labels, customers, expenses and reports (≈ 4 weeks)
- **Batch intake wizard:** header (supplier, invoice ref, total cost, extra costs) → quick-entry grid (name, qty, retail price, attributes; create products inline) → allocation (even / by retail value / manual, with live variance) → receive → generate barcodes and SKUs → **print labels**.
- **Labels (§7.4):**
  - The `two_up_2x1` and `three_up_1x1` templates.
  - TSPL and ZPL emitters, the WYSIWYG SVG preview, and the alignment test and calibration offsets.
  - Barcode-fit warnings.
  - Printing from batch receive and from product lists.
- Batch lifecycle: reopen (when there's been no movement), late items, and close with a remaining-stock write-off.
- **Batch report** (§3.1 metrics) with a per-item table: received date, unit cost, sold, remaining, revenue, profit.
- **Stock:**
  - Adjustments and damage, with approval where configured.
  - Stock counts producing `count_correction` movements.
  - Movement history per product and layer.
  - Stock valuation: now, as of a date, by batch and by category.
  - Oversold layers.
- **Quick-sale review and promotion:** frequency table, promote (product + optional batch layer + shelf count) or ignore. The promoted-key hint appears on the till.
- **Customers (§3.8):**
  - List, profile, purchase history (infinite scroll) and spend analytics.
  - Erasure on request.
  - `CUSTOMER_VIEW`-gated.
- **Expenses:** categories, entry (with drawer pay-out integration) and list.
- **Reports suite:**
  - Day, week, month, year, all-time and custom periods, with device filter and period comparison.
  - The full §5.1 metrics.
  - **Charts** (shadcn Chart / Recharts): revenue vs expense bars, sales trend area, payment method donut, top customers, batch sell-through.
  - Sales by product, category and batch, and the quick-sale share.
  - Voids by reason and approver.
  - CSV export (`DATA_EXPORT`, audited).
- **All back-office lists are on the shared virtualised DataTable**, with server-side keyset pagination for orders, audit and movements.
- `rebuild_derived` maintenance command plus the rollup reconciliation test.

**Exit:**
- The owner enters a 30-SKU batch, allocates costs, receives it, and prints a full roll of 2-up labels and a row of 3-up labels in under 10 minutes. Every printed barcode scans back to the right product, including the 6-digit Code128 on 1″×1″.
- After scripted sales, voids and damages, the batch report reconciles to the cent: landed cost = Σ allocated + variance, and realised profit matches a hand calculation.
- The monthly P&L equals the sum of its daily reports. Rollups equal raw sums after `rebuild_derived`, and the charts match the tables.
- Scrolling 100k orders and 50k products stays at 60 fps on the counter PC.
- A frequent quick-sale item can be promoted in one click and is scannable afterwards. A customer's history and total spend match their receipts.

### Phase 3: Cloud sync across 2–3 counters (≈ 3 weeks)
- Supabase:
  - Migrations, RLS (including the no-UPDATE/DELETE policies), `processed_ops` and `server_seq` triggers.
  - `sync_push` with the advisory lock and invariants, and `sync_pull`.
  - Audit chain verification.
  - pgTAP tests.
- Device registration: an owner signs in once, the edge function creates the device identity **adopting the existing IDs**, and the token goes into the keyring.
- Rust sync worker:
  - Reachability, backoff, push and pull.
  - Barcode-conflict handling, and the deterministic customer ids.
  - `sync_logs`, status pill, and Settings → Sync (pending, last success, dead ops, "Sync now").
- Full-history bootstrap for a new counter, with progress.

**Exit (chaos tests with three terminals):**
- 200 offline sales per terminal, then reconnect: cloud and every terminal show identical daily totals, batch reports, stock valuation and customer stats.
- Kill the network mid-push: no duplicates and no losses.
- Two terminals selling the last unit of a layer: both sales are kept, the layer shows −1, and it's reported.
- The same new phone number captured on T01 and T02 while offline: one customer after sync, with both orders in their history.
- A void on T02 (in its open session) appears as voided on T01 and T03 after sync, and their store-wide reports exclude it.
- A ±1-day clock skew on one terminal changes nothing, because business_date and server_seq don't depend on the clock.
- Bootstrap with `limit = 7`, where batches and batch items were edited *after* their stock movements and orders were voided after their payments: no foreign key aborts, and the derived tables equal `rebuild_derived` output.
- Concurrent owner demotions are rejected by the invariant.
- Editing an audit row in a terminal's DB file is flagged by chain verification.

### Phase 4: Production hardening (≈ 2 weeks)
- **Automatic backups:**
  - Hourly `VACUUM INTO` to `...\backups\`, keeping 48 hourly and 30 daily copies. This reuses the §3.9 engine.
  - An optional automatic copy to a remembered USB folder when it's present.
  - Optional backup encryption.
  - The restore UI (`BACKUP_RESTORE`, audited) always backs up the current DB first, then runs `rebuild_derived` and rebuilds the FTS index after a restore.
- **Restore recovery.** A backup is behind the server for this device's own writes, and the normal pull filters those out. So restore is a guarded flow, and **sales are blocked until it completes**:
  1. **Online (normal case):** run a recovery pull with `include_own: true` from the backup's cursor. That re-imports this device's post-backup orders, movements and audit rows.
  2. Reset `receipt_counters` to the device's max receipt sequence per business day, taken from local orders after the recovery pull.
  3. Reset the audit chain head (`device_seq`, `prev_hash`) from the device's last audit row, cross-checked against `devices.last_audit_seq` / `last_audit_hash`.
  4. Requeue any outbox rows in the backup that the server already acknowledged. They resolve as replays through `processed_ops`.
  5. **Offline restore:** the device can't know what it lost, so it **retires its identity**. It gets a new `device_id` and `device_code` (e.g. T01 → T04), which avoids receipt-number, SKU-range and audit-sequence collisions, and the old device's data stays in the cloud. Once online, the new identity bootstraps normally.
  6. Drill both paths in the restore test.
- **How it is built (Block 4.1):**
  - **Automatic backups** take the §3.9 snapshot every hour into `<data dir>\backups\auto` (`auto-{code}-{YYYYMMDD-HHmmss}.db` + manifest). Rotation keeps the 48 newest copies, a count, so disk use stays bounded whatever the schedule, plus the newest of each of the last 30 business days. Only files of that name in that folder are ever deleted, never the newest. A success is shown in Settings → Backup ("Last automatic backup"); a failure is shown there and audited once (`BACKUP_FAILED`, `automatic: true`) until one succeeds again (decision #22). At 300–500 MB a copy, 48 + 30 copies is roughly 25–40 GB: the 1.2 × free-space pre-check skips a run rather than fill the disk.
  - **A restore is staged across a restart**, never applied to the open database:
    1. Settings → Backup → Restore (Owner) checks the file. It refuses a backup that is corrupt, comes from a newer app, or belongs to another store or **another counter** (two counters must never share an identity).
    2. It backs up the current database to `backups\pre-restore`, copies the backup to `restore\staged.db`, writes the marker `restore.json`, and restarts the app.
    3. At startup, before the database opens, the staged copy is written to `pos.db.restoring`, flushed, the old `-wal`/`-shm` are deleted, and it is renamed over `pos.db`. Every step can be repeated, so a crash anywhere resumes at the next start. Windows briefly holding a just-written file is retried for up to 15 s.
    4. Once the database is open, `restore.pending` is recorded in it, the backup's unprinted print jobs are abandoned, a crash-recovery cart whose sale is already in the backup is dropped, and the marker is removed.
  - **While a restore is pending, nothing is written under this counter's identity.** `audit::record` and the outbox refuse (`RESTORE_PENDING`); the sync pull, which runs with `sync_ctx.applying = 1`, is the only exception. So no sale and no login, and the sync worker stays paused, because a routine pull would move the cursor past this counter's own post-backup rows. The recovery screen replaces the login, and the Owner authorises it with their PIN, which is checked without an audit row.
  - **Recovery** ends in every mode with `rebuild_derived` (the derived tables and the search index) and one `BACKUP_RESTORE` audit row, written last:
    - **Online:** the recovery pull with `include_own` from the backup's cursor. Then the audit chain head is checked against `sync_health`'s `last_audit_seq` / `last_audit_hash` for this device (migration `20261110000100_health_audit_head.sql`); a mismatch refuses (`RESTORE_CHAIN_MISMATCH`) and leaves the offline path. Then the receipt counters are reset from the orders, and the backup's queued ops are requeued (acknowledged ones resolve as replays). The SKU generator already skips SKUs in use.
    - **Offline:** a new device id and the smallest code no counter of this store is known to have used (receipt prefixes, `STORE_INITIALIZED` / `DEVICE_REGISTER` codes). The receipt counters are cleared, and the pull cursor restarts at 0, so once the new identity is registered (Settings → Sync, adopt) it receives the old identity's history as another counter's. The backup's queued ops belong to the old identity: the worker sends them under it first (`sync.retired_identity`, bounded by the outbox seq at retirement; its token is still in the keyring on the same PC), and never under the new one.
    - **Local:** a store that never registered with the cloud keeps its identity; the counters continue from the restored data (decision #21).
  - **A fresh install** (a new PC, a reinstall) offers "Restore this counter from a backup" in the first-run wizard. There is nothing to compare or back up, so the file is only checked and staged. A reinstall on the same PC keeps the Windows Credential Manager token, so the online path is open; on a new PC only the offline path is.
  - **Not built in 4.1:** the optional automatic copy to a remembered USB folder, and backup encryption (Block 4.3 decides SQLCipher).
- Auto-update via `tauri-plugin-updater` (signed). **Windows code signing**. The NSIS installer bundles the WebView2 bootstrapper.
- Security:
  - Capability least-privilege (including a scoped `dialog` permission), CSP, and no secrets in the bundle.
  - RLS tested with a foreign store token.
  - Optional SQLCipher DB encryption.
  - Enforced PIN length for Manager and Owner.
- Observability: rotating logs, a support bundle (logs + sync_logs + integrity check + chain verification), and optional crash reporting.
- Performance budget:
  - Scan to cart in under 100 ms.
  - F2 quick sale to cart line in under 50 ms after Enter.
  - Phone lookup in under 30 ms.
  - Checkout to receipt in under 1.5 s.
  - Search in under 50 ms with 50k products.
  - Yearly report and charts in under 300 ms.
  - A manual backup of a 500 MB DB in under 30 s to USB 3.

**Exit:** a two-week pilot at 2–3 counters with zero data-loss incidents, a restore drill passed, an update shipped through the updater, and an audit chain that verifies for every device.

### Phase 5: Later (future backlog)
- **Exchanges and credit notes.** Deferred by the owner, and v1 sells everything as final. The design notes to pick up from:
  - **Policy draft:**
    - Exchange within N days (7 was proposed) with a receipt, with later exchanges needing supervisor approval.
    - The customer pays any difference, and a credit note is issued for a negative balance, never cash.
  - **Model:**
    - One `type = 'exchange'` order with return lines (`qty < 0`, `return_of_item_id`, `return_disposition` restock/damaged) plus new-item lines.
    - A return line is valued at what the customer paid: the original `line_total` after the item discount **and** the stored `order_discount_share`, with exact remainder handling on partial returns.
  - **Credit notes:**
    - An insert-only `credit_notes` header.
    - The balance is derived from `order_payments` rows (issue negative, redeem positive) rather than a mutable column.
    - Offline over-redemption is flagged, not blocked.
    - It's a liability, not revenue, and kept out of expected cash.
  - **Already in place:** `orders.type`, reserved permission bits 6 and 7, and the per-line discount shares.
  - **To add:**
    - `returns_net` in the rollups.
    - A `return` stock reason.
    - Migrations that widen the `CHECK`s on `orders.type`, `order_items.qty` and `order_payments.amount`.
- An owner web dashboard (Supabase + React, read-only reports with an `audit_logs` viewer) and multi-store support.
- Loyalty points and customer credit sales, foreign phone numbers, and supplier management.
- Tamil UI. Raster (bitmap) Sinhala receipts and labels.
- Promotions (multi-buy, batch clearance pricing).
- Denomination-level cash management.

---

## 9. Setup guide (Phase 0 commands)

Prerequisites (Windows), all present on the dev machine as of 2026-11-08:
1. **Rust** 1.99.1 (MSVC). In Git Bash, run `export PATH="$HOME/.cargo/bin:$PATH"` first.
2. **Visual Studio Build Tools** with the "Desktop development with C++" workload.
3. WebView2 runtime.
4. Node.js 24+ and pnpm 11+.

**Gotchas found while scaffolding:**
- `cargo search` hangs on this network, so let `cargo add` resolve versions.
- `shadcn init` stalls on an interactive preset prompt even with `-y`. Write `components.json`, `src/lib/utils.ts` and the CSS tokens by hand, then use `shadcn add … -y`.
- After `shadcn add`, check the generated imports. They came out as `from "cn"` instead of `@/lib/utils`.
- Add the `@/*` alias to `tsconfig.json` *and* `vite.config.ts` before adding components.

```powershell
# create-tauri-app refuses/prompts on a non-empty dir (docs/ exists), so scaffold beside it and move in:
cd "D:\Program Data\Project"
pnpm create tauri-app@latest gayanshop-scaffold -m pnpm -t react-ts --identifier com.gayanshop.pos -y --tauri-version 2
# move everything from gayanshop-scaffold\ into GayanShop\, rename package/crate, delete the scaffold folder
cd GayanShop
pnpm install

# Tailwind v4 + shadcn (manual init — see gotchas above)
pnpm add tailwindcss @tailwindcss/vite tw-animate-css clsx tailwind-merge class-variance-authority
pnpm dlx shadcn@latest add button input label card badge alert table separator dialog select sonner chart -y

# Frontend libs
pnpm add @tanstack/react-query @tanstack/react-router zustand zod react-hook-form @hookform/resolvers lucide-react date-fns
pnpm add @tanstack/react-table@^8 @tanstack/react-virtual recharts      # tables (v8 pinned) + charts
pnpm add i18next react-i18next @fontsource/noto-sans-sinhala            # en / si UI, offline font
pnpm add -D vitest @testing-library/react jsdom fast-check

# Tauri plugins
pnpm tauri add single-instance
pnpm tauri add log
pnpm tauri add window-state
pnpm tauri add dialog           # folder picker for "Backup Database Now"
pnpm tauri add updater          # Phase 4

# Rust crates (in src-tauri/)
cargo add sqlx --features "runtime-tokio sqlite migrate uuid chrono json"
cargo add tokio --features full
cargo add serde --features derive
cargo add serde_json thiserror chrono chrono-tz bitflags sha2 hex
cargo add uuid --features "v5 v7 serde"
cargo add argon2 rand
cargo add reqwest --features "json rustls-tls" --no-default-features
cargo add keyring serialport
cargo add specta@=2.0.0-rc.26 --features derive,function
cargo add tauri-specta@=2.0.0-rc.26 --features derive,typescript
cargo add specta-typescript@0.0.13
cargo add unicode-normalization
# Cargo.toml: [profile.dev.package.argon2] / blake2 / sha2 → opt-level = 3 (keeps PIN tests fast)
cargo add windows --features "Win32_Graphics_Printing Win32_Foundation"
cargo add --dev proptest
```

---

## 10. Non-negotiables (for code review)

1. No floats for money, anywhere: Rust, TS or SQL.
2. No `UPDATE` of stock quantities. Stock changes only through `stock_movements`, and every movement references a cost layer (`batch_item_id`).
3. Every mutating command follows guard → transaction (business rows + audit + outbox) → commit → emit.
4. Orders, order items, payments, stock movements, cash movements and audit logs are never updated or deleted. The only exception is the order void transition. Corrections are new rows.
5. Batch costs are locked once received.
6. Permissions are enforced in Rust. UI checks are cosmetic.
7. The webview never runs SQL, never sees `pin_hash`, and never holds a Supabase key beyond the device session.
8. A sale never depends on the network, a printer, or the sync worker succeeding.
9. Mutable synced tables have `id`, `store_id`, `updated_at`, `updated_by_device`, `deleted_at` and `version`. Insert-only tables have `id`, `store_id`, `device_id` and `created_at`. Every financial or stock row has a `business_date` computed at write time.
10. A schema change means a migration locally **and** in `supabase/migrations/`, in the same PR, plus an update to `rebuild_derived` if derived logic changes.
11. Permission bits are append-only. Never renumber, and never reuse the reserved bits 6 and 7.
12. **No refunds, no negative payments.** The only correction is a same-session void. A closed session's orders are final for every role.
13. No user-visible string is hard-coded in components or produced by Rust. Everything goes through i18n keys in both `en` and `si`.
14. Anything tappable is ≥ 48 px on till screens and has a keyboard path.
15. Never copy live `pos.db` / `-wal` files. Backups always use `VACUUM INTO` plus an integrity check.
16. Customer phone numbers live only on `customers` and are never printed unmasked or logged in full.
17. Local orders are always inserted `pending` and **sealed**. Never insert `completed` locally, and never touch `sync_ctx` outside the sync pull.
18. No `unwrap` / `expect` / `panic` in production Rust (clippy-denied), no floats outside label geometry, and no mock placeholders. Each Atomic Block gets one git commit after its gate is green (`AGENT.md` §2–§4).

---

## 11. Store configuration (owner-confirmed) and remaining open questions

**Confirmed and seeded at first run** (`store_settings`):

| Key | Value |
|---|---|
| `store.currency` / `store.currency_exponent` | `LKR` / `2` |
| `store.timezone` | `Asia/Colombo` |
| `store.day_cutoff` | `04:00` |
| `store.week_start` | `monday` |
| `tax.inclusive` / `tax.default_rate_bp` | `true` / `0` (not VAT/SSCL registered) |
| `discount.cashier_max_bp` | `500`, which seeds the Cashier role's `max_discount_bp` |
| `sales.policy` | `final` (no refunds, no exchanges) |
| `void.max_minutes` | `null`, **fixed (owner-confirmed)**: a void is allowed only while the order's cash session is open. It requires a supervisor PIN (for cashiers) and a mandatory reason. |
| `quick_sale.auto_tag_batch` | `false` (owner: never auto-tag) |
| `store.name` | `GayanShop` (printed on labels and receipts) |
| `receipt.footer` | **"All sales are final. No refunds or exchanges. Thank you, come again!"** (owner-confirmed, verbatim) |
| `receipt.mask_customer_phone` | `true`: printed as `07X XXX X123` |
| `customer.capture` | `optional`, with **verbal consent at the counter**. There's no popup, no notice dialog and no blocking step. Enter or Esc on an empty phone field skips it. |
| `ui.default_language` (device) | `en`. Staff can switch to `si` (Sinhala), and the choice is saved per staff member. |
| `ui.touch_mode` (device) | `false` when absent (not seeded). Settings → Touch screen turns on the on-screen keypads and PIN-pad digit keys for touch counters (§1.9). |
| `label.templates` (device) | The `two_up_2x1` and `three_up_1x1` built-ins (§7.4). **Default 203 dpi, with a 2 mm row (die-cut) gap.** Column gap and left margin are roll-specific and set by calibration. |
| `backup.reminder_days` | `7` |
| Stores | 1 store with 2–3 counters for the next 12 months. Multi-store stays in the design, and delivery is in Phase 5. |

**All §11 questions are answered (owner, 2026-11-08).** The only measurement left is physical: when the label rolls arrive, run the Settings → Labels alignment test to set the column gap and left margin (§7.4). That's calibration, not a design decision.

## 12. Decisions (owner-confirmed 2026-11-08)

| # | Decision | Alternative rejected | Why |
|---|---|---|---|
| 1 | **FIFO by batch receive date**; when oversold, the newest layer goes negative | Average costing; blocking the sale | FIFO matches how a discount store clears batches, and it makes batch P&L meaningful. A sale is never blocked offline. |
| 2 | **Allocation rounding** goes into `allocation_variance` (< 1 cent per unit), recognised as COGS on the receive date | Pushing the remainder onto one item's unit cost | The remainder can't be spread exactly in integer cents, and this way both reports reconcile with what was paid. |
| 3 | **Late items get cost 0**, and full re-allocation only happens via reopen when nothing has moved yet | Re-allocating after sales | Re-allocating would change the cost of sales already made, which rewrites history. |
| 4 | **Quick-sale cost is optional**; unknown-cost revenue is reported separately | Requiring a cost; assuming a margin % | Requiring a cost breaks the speed target, and assuming a margin invents numbers. Showing it transparently is honest. |
| 5 | **Promotion is forward-only**, with no retroactive batch attribution | Re-attributing past quick sales to the batch | Past lines are immutable, and historical P&L shouldn't move when a manager tidies up. |
| 6 | **All sales final; the only correction is a same-session, whole-order void** (a cashier needs supervisor approval) | Refunds; 7-day exchanges with credit notes (v3); voiding past orders | Owner simplification (v4). Keeps closed Z-reports fixed and cash reconciliation true. Exchanges and credit notes move to Phase 5, and the schema keeps extension points. |
| 6b | **Cashier discount ≤ 5%, combined per line (item + bill share)**, bill discounts apportioned pro-rata by largest remainder | Separate 5% caps for item and bill (which allows ~9.75% stacked) | Owner rule. Pro-rata shares keep batch margins exact, and future exchange values with them. |
| 10 | **Dual-language UI (en/si), with receipts and labels in English** | Sinhala on receipts | Generic ESC/POS and TSPL printers have no Sinhala code page. Raster printing is Phase 5. |
| 11 | **Customer phone optional, keyed by `UUIDv5(phone)`, never copied onto orders** | A mandatory customer; a phone snapshot on each order | Never slows a sale, merges cleanly offline, and makes PDPA erasure possible without touching immutable orders. |
| 12 | **3-up 1″×1″ labels use a 6-digit Code128 SKU** | EAN-13 on 1″ labels | EAN-13 can't fit at a scannable module size on 1″ at 203 dpi. |
| 13 | **Manual backup via `VACUUM INTO` on a read connection** | Copying `pos.db`; the SQLite backup API under the writer lock | A consistent snapshot without pausing the till. Raw copies of WAL databases can be torn. |
| 14 | **TanStack Table v8 + Virtual, shadcn Charts (Recharts)** | TanStack Table v9 (newer); a heavier grid library | Owner choice. v8 is what the shadcn data-table pattern targets. Revisit v9 later. |
| 7 | **The audit hash chain is per device**, and breaks are flagged, not rejected | A global chain; rejecting rows | A global chain forks under offline interleaving. Rejecting rows would lose evidence. |
| 8 | **PIN hashes sync** to all counters | Online-only login | Offline login is mandatory. The risk is documented and mitigated (longer manager PINs, optional DB encryption). |
| 9 | **Every terminal holds the full history** | Only the device's own orders | Store-wide reports and customer history at any counter, and the Phase 5 exchanges later. |
| 15 | **`synchronous = FULL`** | `NORMAL` (faster) | Under WAL, `NORMAL` can lose the last committed sale on a power cut after its receipt has printed. `FULL` costs one fsync (~1–5 ms) per commit (§13.2). |
| 16 | **Seal pattern** (`pending → completed`) with SQL-verified totals | Inserting orders as `completed` | It closes the "add a line or payment to a sold order later" hole and makes SQL re-check every total (§13.5). |
| 17 | **Negative stock layers are flagged, never auto-rebalanced** | Automatic layer rebalancing | Rebalancing would rewrite COGS on past sales, and the system can't know physical truth. Only a stock count can (§13.1). |
| 18 | **A new counter joins the store from the first-run wizard** ("Join existing store"): the owner signs in with the store's cloud owner account, the counter gets the next free code (T02, T03), pulls the full history, and staff log in with their existing PINs (owner-confirmed 2026-11-09) | A pairing code generated on T01 | One flow for the first counter (adopting its offline ids) and for later ones. The owner's password never stays on a counter; each counter has its own device identity. |
| 19 | **The owner's cloud account is one email + password user created in the Supabase dashboard.** The first counter that registers with it claims the store; later counters must use the same account. Public sign-up is off (owner-confirmed 2026-11-09) | A shared registration secret | Whoever holds a shared secret could join a counter to the store. |
| 20 | **JSON-in-text columns stay `text` in the cloud** (§4.4) | `jsonb` | Byte-identical round trips. The audit hash chain depends on them. |
| 21 | **A store that never registered with the cloud keeps its counter identity on restore** (owner-confirmed 2026-11-10) | Retiring the identity as an offline cloud restore does | Nothing after the backup exists anywhere to collide with; the receipt and SKU counters continue from the restored data. |
| 22 | **Successful automatic backups are not audited; a failure is audited once until one succeeds** (owner-confirmed 2026-11-10) | Auditing every hourly backup | About 24 audit rows a day, all synced, would bury the audit log. The status shows the last automatic backup. Manual backups and restores stay fully audited. |
| 23 | **SQLCipher deferred; standard SQLite WAL with OS file permissions retained** (Block 4.3) | Bundling SQLCipher / full-database encryption | SQLite WAL with Windows NTFS ACLs meets the single-store offline threat model with zero read overhead; full-disk BitLocker protects against physical theft. |
| 24 | **Local rotating logs + support bundle export; no SaaS crash reporting** (Block 4.4) | Third-party cloud telemetry (Sentry, etc.) | Preserves customer privacy and strict offline operation; support bundle ZIP is exported on demand by the store owner for diagnostics. |


---

## 13. Architecture stress test (2026-11-08)

This section is the result of attacking the design with real-world retail failure modes. Each finding says **what can go wrong**, **what the design now does**, and **which named test proves it**.

The §4.3 schema's triggers and invariants were first exercised in a scratch harness against SQLite 3.50.4. **Block 0.3 re-proved them against the app's bundled SQLite 3.46.0** (libsqlite3-sys 0.30.1): `tests/migration_invariants.rs` has 19 scenarios and `tests/fts.rs` has 2, all passing. The FTS5 trigram tokenizer is confirmed available. Other rows in this section remain targets until their named blocks run.

### 13.1 Concurrency and drift: three offline counters selling the last units of one layer

**Scenario.** Layer L (Shoe batch, size 42) has `on_hand = 2`. T01, T02 and T03 are offline, and each sells 1 pair. T02 then sells a second pair.

| Step | What happens |
|---|---|
| Local sale | Each counter's FIFO (`domain::fifo`) sees `on_hand = 2` locally and draws from L. No counter blocks: `allow_negative_stock` defaults to true, and a sale must never wait on the network. |
| IDs | Every row is UUIDv7 with the device id inside the row. Receipt numbers are `T0x-YYYYMMDD-seq`, unique per device. **No two counters can create the same primary key or receipt number.** |
| Sync | `stock_movements` is insert-only, so the merge is a set union. `INSERT OR IGNORE` is idempotent, and there's no conflict to resolve and no last-write-wins on quantities. After all three sync, every counter computes `on_hand(L) = 2 − 4 = −2`, the same everywhere. |
| After sync | FIFO only draws from layers with `on_hand > 0`, so new sales take the next layer automatically. The layer appears in **Oversold layers** (`ix_bis_negative`). COGS for the four sales used L's cost, which is the best-known cost when they were sold. |

**What is automatic:** the merge itself, with no collisions and no lost sales; identical totals on all counters; FIFO moving on; and the flag.

**What is deliberately not automatic: the physical truth.** −2 means one of three things, and software can't tell which:
- The batch held more pairs than were recorded
- Pairs from another layer were sold under L's barcode
- A data-entry error

Automatically "rebalancing" (moving −2 from L into the next layer) would **rewrite the COGS of sales already made** and could hide shrinkage. So the design keeps the flag and fixes it with one `count_correction` (`STOCK_COUNT`, audited).

**The owner asked for reconciliation "without human intervention". The data side achieves that; the physical-stock side needs a count, and that's the correct accounting.** (Decision #17.)

**Proof:** Block 3.6 chaos test `three_counters_oversell_last_units`, with the assertion "identical ledger on 3 nodes, layer = −2, flagged, next sale uses the next layer".

### 13.2 Abrupt power loss or crash mid-transaction

| Failure point | Outcome | Why |
|---|---|---|
| During `checkout_order`, before `COMMIT` | **Nothing was sold.** No order, slices, payments, receipt-counter bump, audit row, outbox row or print job. The cart is still in the `held_carts` `__current__` row (≤ 500 ms old) and offered back on restart ("Recover cart?"). | One SQLite transaction. The WAL rolls uncommitted frames back on open. There's no orphan risk: every child row is in the same transaction as its parent. |
| Just after `COMMIT`, before the receipt prints | The sale exists. The `print_jobs` row (same transaction) lists it as unprinted on restart. The recovered cart **can't be re-sold**, because the checkout transaction deleted `__current__`. | The print job and the held-cart delete are inside the sale transaction (§7.5, §4.3). |
| Power cut seconds after the receipt printed | **The sale survives.** | **`synchronous = FULL`** (§1.1). It was changed from `NORMAL`, which under WAL can drop the last commits on OS crash or power loss. |
| Double-tap on **Pay**, or a UI retry after an IPC timeout | Exactly one order. The second call returns the first order. | `orders.checkout_token` is UNIQUE and generated once per cart (§4.3). |
| Crash with the cart in an unusual state | On restart: `quick_check`, the integrity probes (`v_integrity_pending_orders`, `v_integrity_void_unreversed` must be empty), cart recovery, and unprinted-receipt recovery. | §1.1, §4.3 |
| Disk full | `SQLITE_FULL` aborts the transaction. The sale isn't recorded, the cart is kept, and a red banner appears. A free-space check at startup warns below 500 MB. | Transactions are atomic. |
| Migration interrupted | Each migration runs in its own transaction, and an automatic `VACUUM INTO backups/pre-migration-*.db` runs first (Block 0.3). | sqlx migrator |

**Corruption:** SQLite in WAL mode with `FULL` doesn't corrupt on power loss on compliant storage. Cheap USB flash or SD cards are *not* compliant, so the DB always lives on the internal SSD (`%APPDATA%`), and USB is used only for backups.

**Proof:** Block 1.4 test `crash_mid_checkout_leaves_nothing` kills the process at every `await` point using a fault-injection hook. Block 1.12 adds a pull-the-plug manual drill on real counter hardware.

### 13.3 Hardware latency and race conditions

**Scanner (HID wedge, keys < 30 ms apart).** The full design is in §7.1.

| Race | Mitigation |
|---|---|
| The first characters of a scan land in a focused input before detection | A configured **prefix** (`F13`) makes capture deterministic. The timing-heuristic fallback provisionally swallows from the 2nd fast key, removes the 1 leaked char, and replays the buffer if it turns out to be a human typing. |
| The scan's terminating `Enter` presses Pay or submits a form | `Enter` is consumed in the window capture phase while capture is open |
| A Sinhala keyboard layout garbles scans | Decode from `KeyboardEvent.code`, not `.key` |
| Two quick scans, with lookups resolving out of order | FIFO scan queue with one worker; functional state updates (`qty + 1`) |
| An F-key shortcut fires mid-scan | Shortcuts are suspended while capture is open |

**Proof:** Block 1.8 vitest suite `scanner.race.test.ts`, which synthesises bursts at 5, 15 and 29 ms, human typing at 80 ms, a Sinhala layout, focus in an input, and an `Enter` over the Pay button.

**Printer (paper-out, spooler freeze).** The full design is in §7.5. The checkout command returns after `COMMIT` and never awaits I/O.
- Jobs are persisted in the sale's own transaction.
- One worker runs per printer, with one in-flight job and hard timeouts.
- `WritePrinter` runs on a dedicated thread under a timeout, and a hung printer is marked unhealthy without piling up threads.
- Paper-out is detected via `DLE EOT` where the transport supports it, and shown as a persistent banner with Retry.

**Proof:** Block 1.7 tests with a fake transport that blocks forever, errors and reports paper-out. The assertion is "a checkout completes in < 100 ms while the printer is hung; the job ends `failed`; no second thread is spawned".

### 13.4 Database scale budget (targets, verified by named tests)

**Dataset for the benchmark:**
- **Gate:** 50,000 products and **100,000 order items** (≈ 35k orders).
- **Headroom run:** 1,000,000 order items (~3 years at 3 counters). It must stay within 2× of the gate numbers.
- Generated deterministically by `tests/perf/seed.rs`.

**Hardware:** the slowest counter PC. A CI runner is used for regression only.

| Query | Index / structure | Target (p95) |
|---|---|---|
| Barcode scan → product | `ux_products_barcode` (partial unique), then `ux_products_sku` | < 2 ms |
| Type-ahead `"blu* buc*"` (LIMIT 20) | `products_fts` unicode61 + `prefix='2 3'` | < 15 ms |
| Sinhala substring search | `products_fts_si` (trigram) | < 25 ms |
| Phone → customer | `ux_customers_phone` | < 2 ms |
| Checkout transaction (5 lines, FIFO, seal checks, outbox), `FULL` fsync included | PK and FK indexes; seal subqueries are per-order | < 40 ms |
| Daily, weekly, monthly or yearly P&L | `daily_sales_summary` (≤ 365 × 3 rows per year) + `ix_expenses_bdate` + `ix_mov_writeoffs` | < 20 ms |
| Order history page (500 rows, keyset) | `ix_orders_keyset (completed_at DESC, id DESC)` | < 20 ms |
| Customer list sorted by spend | **Derived `customer_stats`** + `ix_customer_stats_spend`. A view aggregating `orders` was replaced, because it would scan orders. | < 20 ms |
| Quick-sale review sorted by frequency | **Derived `quick_sale_stats`** + `ix_qss_frequency`. This replaced a correlated-subquery view. | < 20 ms |
| Batch report / stock valuation | `batch_item_stock` + `ix_bis_batch` | < 30 ms |
| Audit log page | `ix_audit_keyset` | < 20 ms |

- **Plan assertions:** every hot query also runs `EXPLAIN QUERY PLAN` in the test and fails on `SCAN <table>` over orders, order items, movements, products or audit logs. That catches a missing index even when the data is small.
- **Pragmas:** `cache_size` 64 MiB, `mmap_size` 256 MiB, `temp_store = MEMORY`, `PRAGMA optimize` on close and daily, and `ANALYZE` after bootstrap or import (§1.1).
- **Sinhala FTS: verified in Block 1.1** (`tests/fts_sinhala_names.rs`, 20 real product names, bundled SQLite 3.46.0):
  - **unicode61 is unusable for Sinhala.** It treats every dependent vowel sign and the virama (al-lakuna) as a separator, so it indexes bare consonants ('ක', 'ද', …) and no whole word survives. Words that differ only in their vowel signs collide: "බෝල" (ball) and "බිල" (bill) both match "බාල්දිය" (bucket), and "කපා" (cut) matches "කපු" (cotton).
  - **trigram is correct.** It finds all 20 names by any 3+ character substring from inside a word, with none of those false matches. `products_fts_si` stays trigram.
  - Queries of 1–2 characters can't use a trigram `MATCH`, so the till falls back to `LIKE` over `products_fts_si` (a scan of the Sinhala index only; outside the budget table).
  - The zero-width joiner in conjuncts such as "ශ්‍රී" is part of the stored name: a query typed the same way matches. A keyboard that omits the ZWJ won't match across it; revisit if staff report misses.
- **Block 1.1 measurement** (`cargo test --release --test perf_budgets -- --ignored search`, dev machine, 50,000 products / 20,000 customers, p95): barcode scan 0.14 ms, SKU scan 0.25 ms, type-ahead 1.7 ms, Sinhala substring 4.3 ms, phone → customer 0.10 ms. All within budget.

**Proof:** Block 1.1 runs `cargo test --release --test perf_budgets -- --ignored` for the search and lookup rows. Block 2.10 runs the full table above, including the headroom run.

**Block 2.10 measurement** (release build, dev machine, p95; details in `docs/qa/phase2-acceptance.md`). Gate: 50,000 products, 20,000 customers, 33,307 orders and 100,000 order items over a year at three counters. Headroom: 1,000,000 order items, within 2× of every budget.

| Row | Gate | Headroom |
|---|---|---|
| Checkout | 21.2 ms | 22.3 ms |
| P&L day / week / month / year | 0.46 / 0.47 / 0.51 / 1.5 ms | 0.44 / 0.45 / 0.46 / 1.2 ms |
| Order history page | 5.6 ms | 4.6 ms |
| Customer list by spend | 4.1 ms | 18.7 ms |
| Quick-sale review | 2.0 ms | 1.8 ms |
| Batch report (30 items) | 17.3 ms | 12.2 ms |
| Audit page | 3.1 ms | 2.3 ms |

- **Customer list.** The unfiltered spend and last-visit lists walk `customer_stats`' indexes.
- **Order history.** It filters `+o.store_id`, so `ux_orders_receipt` never displaces the keyset index.
- **Store-wide valuation.** Every layer, by batch and by category, is an on-demand report outside this table. It aggregates in SQL (191 ms at 50,000 layers) and is guarded at < 250 ms.
- **Scrolling.** The 60 fps criterion is checked in the real WebView2 by `tests/e2e/phase2.e2e.ts`. It scrolls 50,000 products, and the year's orders while pages load, and both keep the display's frame rate.

### 13.5 Integrity invariants enforced inside SQLite

These hold even if Rust has a bug or someone runs SQL against the file. Each one was checked in a throwaway scratch script during the stress test, which isn't kept in the repo. Block 0.3 rebuilds them as `tests/migration_invariants.rs`, from the scenario list in `AGENT.md`.

| Invariant | Enforced by | Class |
|---|---|---|
| **No negative payments**, and no refund-shaped rows | `CHECK (amount >= 0)`; `tendered ≥ amount` and cash-only | Structural |
| Payments always equal the bill exactly; change only from cash tendered | `CHECK (paid_total = grand_total)` + seal check | Structural + policy |
| **Completed orders can't be mutated.** Money, customer, staff, session, date and receipt are frozen. The only transitions are `pending → completed` (seal) and `completed → voided`. Voided is terminal, and deletion is impossible. | `trg_orders_state_machine`, `trg_orders_no_delete` | Structural |
| **No lines, payments or sale slices added to a sealed order** | `trg_items_only_into_pending`, `trg_payments_only_into_pending`, `trg_mov_sale_void_guard` | Policy |
| **Local orders must be sealed through verification.** Header totals = Σ lines; grand = lines + rounding (+ tax when exclusive); payments = grand; stock slices cover each tracked line exactly in qty, revenue and cost | `trg_orders_insert_pending`, `trg_orders_seal_verify` | Policy |
| Price below list needs `PRICE_OVERRIDE` or an approver holding it | Seal check via `v_staff_effective` | Policy (structural) |
| A discount above the cashier's limit needs an approver whose limit covers it | Seal check | Policy (structural) |
| **No unapproved voids:** same device, session still open, the voider holds `VOID_ORDER` or a *different* active approver does, reason required (and a note for `OTHER`) | `trg_orders_void_verify`, the `void_reason` CHECKs | Policy (structural) |
| A closed session can't reopen, so a void can't be made possible again after the Z-report | `trg_cash_session_no_reopen` | Structural |
| A void slice must mirror a real sale slice and can't over-reverse it | `trg_mov_sale_void_guard` | Policy |
| Append-only: order items, payments, stock movements, cash movements, audit logs | no-update / no-delete triggers | Structural |
| Audit chain extends gaplessly with a linked `prev_hash` | `trg_audit_chain_guard` | Policy |
| Batch cost and qty locked after receive | `trg_batch_item_cost_lock` | Structural |

**Limits, stated honestly:**
- **SQL can't verify a PIN.** The triggers check that an approver *exists, is active and holds the bit*. The PIN check, the single-use 60 s approval token, and the binding of approval to target all live in Rust (`auth::guard`, §3.5), and are tested in Block 0.6.
- **Policy triggers stand down while `sync_ctx.applying = 1`** (§4.1, §6.2). Pulled rows were checked at their origin and by the server's `sync_push` invariants (Block 3.2).
- **"Every sale slice is reversed by a void"** can't be enforced at commit time in SQLite, which has no deferred triggers. Rust does it in the void transaction, `tests/void_reversal.rs` proves it, and the probe `v_integrity_void_unreversed` must stay empty at startup.
