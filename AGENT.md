# AGENT.md: GayanShop POS execution rules and roadmap

> **This file is the canonical, tool-agnostic operating manual for every coding agent working in this repo**, including Claude Code. (`CLAUDE.md` imports it.)
> It holds the non-negotiable rules and the **Atomic Block roadmap**, with each block's verification gate and status.
>
> **Design truth lives in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).** If this file and the architecture doc disagree, the architecture doc wins: fix this file, then continue.
>
> Never change an architectural decision silently. Propose the doc change to the owner first.

**Current state (2026-11-08): ⏸ PHASE 0 IS ON HOLD.** Don't write application code, scaffold or install anything until the owner explicitly says to start (e.g. "start Block 0.1" or "begin the /goal run for Phase 0").

---

## 1. Project identity and mission

**GayanShop POS** is an offline-first desktop point of sale for one discount store (Sri Lanka, LKR) with 2–3 counters. The owner buys bulk **batches** of mixed goods and sells them, so the system must answer "did this batch make money?".

- **Stack:** Tauri v2 (Rust core) + React 19 + TypeScript + Vite + Tailwind CSS v4 + shadcn/ui.
  - Local SQLite (WAL, `sqlx`), owned by Rust.
  - Supabase cloud sync later (Phase 3).
  - English and Sinhala UI; receipts and labels in English.
- **Policies (owner-final):**
  - All sales are final: no refunds, no exchanges.
  - The only correction is a same-session void with a supervisor PIN and a mandatory reason.
  - The cashier discount limit is 5%, combined item and bill discount, per line.
  - Prices are tax-inclusive at 0% tax.
  - Asia/Colombo, 04:00 business-day cutoff, Monday week start.
  - Receipt footer, verbatim: *"All sales are final. No refunds or exchanges. Thank you, come again!"*

### Architectural principles (detail in the doc)
1. **The local SQLite file is the source of truth for the till.** A sale never waits on the network, a printer or sync.
2. **The webview never runs SQL.** Every read and write goes through typed Rust `#[tauri::command]`s.
3. **Every mutating command:** `guard (RBAC)` → **one transaction** (business rows + audit row + outbox rows [+ print job]) → `commit` → emit `db://changed`.
4. **History is immutable.** Orders are sealed (`pending → completed`) with SQL-verified totals. The only later change is `completed → voided` inside the open session. Stock is an append-only ledger of cost layers (FIFO).
5. **Integer money everywhere:** `i64` minor units (cents). Never floats.
6. **Defence in depth.** Rust enforces the rules, and SQLite triggers re-enforce structure (§13.5 of the doc).

---

## 2. Non-negotiable engineering rules

Every block must comply. Reviewers and gates enforce them.

### Rust
- **No `unwrap()`, `expect()`, `panic!`, `todo!`, `unimplemented!` or `unreachable!`** in production code paths. Use the typed `AppError` (`thiserror`) and `?`.
  - Enforced at the crate root: `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable))]`.
  - These are allowed in `#[cfg(test)]` code only.
  - Lock poisoning is mapped to `AppError::Internal`, never `.expect()`.
  - **The single sanctioned exception:** `#![allow(clippy::unreachable)]` in `src-tauri/src/commands/mod.rs`, because the `#[tauri::command]` expansion for async commands emits `unreachable!()`. Hand-written `unreachable!(` is still banned everywhere by `pnpm guard`.
- **No floats for money:** `#![deny(clippy::float_arithmetic)]` at the crate root. The **only** `#[allow]` is the label layout module (`hardware::label::layout`, millimetre geometry), and it must never touch money.
- **Type safety across IPC:** every command and DTO derives `specta::Type`, and `src/db/bindings.ts` is **generated** (tauri-specta) and never hand-edited. Every IPC `i64` field carries `#[specta(type = specta_typescript::Number)]`.
- Use runtime `sqlx::query` / `query_as` with `FromRow`, **not** the `query!` macros (they need `DATABASE_URL` / offline data).
- **All writes go through `Db::write()`**, the single-writer lock. Never write through the pool directly.
- `cargo fmt` is clean and `cargo clippy --all-targets -- -D warnings` is clean.

### SQL / data
- §4.3 of the doc is the schema, and `src-tauri/migrations/0001_init.sql` must equal it until Phase 0 ships. After that, schema changes are **new migration files only**, plus a doc update, plus a `rebuild_derived` update if derived logic changes.
- Never `UPDATE` stock quantities; never update or delete append-only tables; never bypass the seal pattern; never set `sync_ctx.applying = 1` outside the sync pull.
- Money columns are `INTEGER`. Every financial or stock row carries a `business_date` computed at write time.

### TypeScript / React
- `strict` TypeScript. No `any` (`@typescript-eslint/no-explicit-any` set to error) and no `@ts-ignore`.
- Money only through `src/lib/money.ts` (integer minor units). `parseFloat`, `toFixed` and `Number()` on money are lint-banned.
- Only `src/db/` and `src/hardware/` call `invoke`. Features never import from other features.
- **No hard-coded user-visible strings.** Everything is an i18n key present in **both** `en` and `si`.

### Quality
- **No lazy shortcuts:** no mock placeholders, stub implementations, `TODO` / `FIXME`, commented-out code, or tests that assert nothing.
  - A block is either fully implemented to its deliverables or not done.
  - A guard script (`pnpm guard`) greps production sources for `todo!|unimplemented!|TODO|FIXME|console.log|debugger`.
- **Never weaken, skip, `#[ignore]` or delete a test to make a gate pass.**
  - Perf tests are `#[ignore]`d by default *by design* and run via an explicit `-- --ignored`.
  - Never add `#[allow(...)]` to silence a rule above.
- Tests cover the behaviour in the block's deliverables, including the failure paths: invariant rejections, permission denials, crash points.

### UI/UX (doc §1.4, §1.8, §1.9)
- Tailwind v4 tokens + shadcn/ui. Components use semantic tokens only, never raw hex.
- **Touch targets ≥ 48×48 px**, with ≥ 8 px between them, on the till and in dialogs. **Pay** and the keypad are 64 px.
- **Keyboard-first:**
  - F1 help, **F2 quick sale, F3 search, F4 phone, F9 Pay**, F6/F7 line and bill discount, F8 hold, F10 void, F11 recall, Esc cancel, Ctrl+L lock, Ctrl+Shift+L language, Numpad `+` pay, Numpad `/` phone.
  - Visible `<Kbd>` badges on till buttons.
  - Focus always returns to the scan field.
- Contrast: till text ≥ 7:1, other text ≥ 4.5:1, control borders and focus ring ≥ 3:1. Light and dark themes come from tokens (zinc neutrals, **emerald** for success/Pay).
- Feedback: first frame ≤ 50 ms on cart add; error tone and red toast for an unknown barcode; **never a modal that blocks the next scan**. Respect `prefers-reduced-motion`.
- Lists use the shared `DataTable` (TanStack Table **v8** + TanStack Virtual). Charts use shadcn Chart (Recharts).
- Everything is bundled locally (fonts, audio, icons). No CDN.

### Security
- No secrets in the repo or the bundle. `pin_hash` never crosses IPC. Tauri capabilities stay least-privilege.

---

## 3. Execution protocol (long autonomous runs)

1. **Start only on the owner's explicit instruction**, naming the phase or block. Work **one block at a time, in order**. A block's prerequisites are all earlier blocks.
2. **At the start of every block**, re-read this file and the doc sections the block cites. Then check `git status` and `git log --oneline -5`, to confirm the previous block's commit exists and the tree is clean.
3. **Implement the block's deliverables completely**, as specified in the doc.
4. **Run the block's gate** (§5) exactly as written, from the repo root, in Git Bash, with cargo on PATH (§6).
   - **All commands must exit 0.**
   - **A cargo or vitest gate only counts if the output shows a nonzero number of tests run for every named target.** Cargo exits 0 when a filter matches nothing, so `0 passed` is a red gate.
   - Paste or summarise the real output in your report. Never claim a pass you didn't observe.
5. **Green gate:**
   - Tick the block's box in §5 (`[x]` + date).
   - Create **one atomic commit** for the block (§4).
   - Then, and only then, move to the next block.
6. **Red gate:** fix the root cause and re-run.
   - After **3 genuine fix attempts** on the same failure, **stop** and report: what failed, the output, what you tried, and your hypothesis.
   - **Never** advance with a red gate. Never tick an unpassed block.
7. **Stop and ask the owner** when any of these apply:
   - A spec is ambiguous or contradicts the doc.
   - A decision would change architecture or policy.
   - Something needs credentials, payment, accounts, a Supabase project, code-signing certificates or physical hardware.
   - An operation is destructive beyond this block's own uncommitted work.
8. **Pre-v4 draft files.** Some files already on disk predate v4:
   - `src-tauri/migrations/0001_init.sql`
   - `src-tauri/src/{error,clock}.rs`
   - `src-tauri/src/{domain,db,audit,auth}/**`

   They contain exchange and credit-note code and `.expect()` calls. They're **not compiled** until a block wires them in. The block that owns a file must review it against v4 and these rules, fix or replace it, and test it before declaring the module.
9. **Context hygiene:** after each block, the status in §5 plus the commit log *is* the progress record. After a context reset, resume from the first unticked block.

---

## 4. Git version control and atomic commits (owner rule)

- **Repository:** already initialised by the owner (branch `main`, root commit `ca36dcc` "Initial commit", 2026-11-08), which contains the docs and the paused scaffold. If `.git` is ever missing, run `git init` and create `.gitignore` *before* the first commit.
  - Block 0.1 **extends** `.gitignore` so `*.db`, `*.db-wal`, `*.db-shm`, `.env*`, `src-tauri/gen/`, `backups/` and `coverage/` are never committed. `node_modules`, `dist` and `src-tauri/target` are already covered.
- **One atomic commit per Atomic Block**, created immediately after the block's gate is 100% green.
  - **Never** bundle multiple blocks into one commit.
  - **Never** commit a red or partial block.
  - Fixes discovered later get their own commit, scoped to the block they fix.
- **Message convention** (Conventional Commits, block-scoped):
  - `feat(block-X.Y): <concise summary>` for a block delivering features
  - `test(block-X.Y): <summary>` for a block that is test-only
  - `fix(block-X.Y): …`, `docs(block-X.Y): …` and `chore(block-X.Y): …` are allowed when that's the truthful type
  - Body: deliverables, plus the gate commands that passed.
- **Each commit is an immutable safety checkpoint:**
  - Never amend, rebase or force-push committed blocks.
  - Never use `--no-verify`.
  - To undo a bad block, use `git revert <sha>`.
  - Discarding *the current block's own uncommitted changes* (`git restore`, `git clean` of files this block created) is allowed. Resetting past a committed block needs the owner's approval.
- **Never push** unless the owner asks. No remote is configured.

---

## 5. Atomic Block roadmap (canonical)

**How to run a gate.** Run every gate from the repo root in Git Bash, after `export PATH="$HOME/.cargo/bin:$PATH"`.

**The base gate** that every block runs last is `pnpm verify`. It exists from Block 0.1 onwards and runs, in order:

```bash
pnpm lint            # eslint (flat config: typescript-eslint, react-hooks, jsx-a11y) + prettier --check
pnpm typecheck       # tsc --noEmit
pnpm test            # vitest run
pnpm i18n:check      # en/si key parity
pnpm guard           # bans todo!/unimplemented!/TODO/FIXME/console.log/debugger in production sources
pnpm rust:fmt        # cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
pnpm rust:clippy     # cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm rust:test       # cargo test --manifest-path src-tauri/Cargo.toml
pnpm bindings:check  # regenerate src/db/bindings.ts, then: git diff --exit-code -- src/db/bindings.ts
```

Block 0.1 creates `verify` **without** `bindings:check`. Block 0.4 adds it, once bindings exist and are committed.

Each block's **gate = its specific commands + `pnpm verify`**.

### Phase 0: Foundations (⏸ on hold until the owner says go)

- [x] **0.1 Repo hygiene, git and tooling baseline**: done 2026-11-08
  - **Deliverables:**
    - Extend `.gitignore` (§4). The repo already exists; don't re-init.
    - The ESLint flat config, Prettier, `rustfmt.toml`, and the crate-root lint attributes from §2 (in `src-tauri/src/lib.rs`).
    - `package.json` scripts for everything in the base gate, plus `scripts/guard.mjs`.
    - Delete `src-tauri/src/domain/exchange.rs`.
    - Replace `src-tauri/migrations/0001_init.sql` with doc §4.3 **verbatim**.
    - Remove the leftover scaffold assets.
    - `README.md` pointing to the doc and this file.
  - **Gate:**
    - `git rev-parse --is-inside-work-tree`
    - `pnpm verify`
    - `git status --porcelain` is empty after the commit
  - **Commit:** `chore(block-0.1): repo hygiene, git, lint/format tooling, v4 migration`

- [x] **0.2 Domain core (pure Rust, no IO)**: done 2026-11-08
  - **Deliverables:**
    - `src-tauri/src/domain/`: `money`, `discount` (largest remainder plus limit check), `pricing` (inclusive/exclusive tax), `payments` (no negatives; change only from cash), `business_date` (Colombo, 04:00, Monday), `allocation`, `fifo`, `phone` (LK → E.164), `receipt_no` (+ 6-digit SKU generator), `quick_sale` (custom_key), `permissions` (frozen bits; 6/7 reserved; 29 `CUSTOMER_VIEW`, 30 `BACKUP_CREATE`).
    - Unit tests, plus `proptest` property tests: shares sum exactly, allocation variance bound, pricing identities.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --lib domain::`
    - `pnpm verify`

- [x] **0.3 Database layer and schema invariants**: done 2026-11-08 (bundled SQLite 3.46.0)
  - **Deliverables:**
    - `db/mod.rs`: pool, PRAGMAs (WAL, **`synchronous=FULL`**, FK, cache, mmap), single-writer `Db::write()`, migrator, pre-migration `VACUUM INTO` backup.
    - `db/outbox.rs`.
    - `db/sync_ctx.rs` (apply-flag helper, crate-private to `sync`).
    - `db/integrity.rs` (startup probes + `quick_check`).
    - `tests/migration_invariants.rs`, written from the scenarios listed here (the stress-test harness was a throwaway session script, not a repo file). Each scenario must be its own `#[test]`:
      - Every row of doc §13.5. Each rejection asserts its specific error message.
      - A happy-path checkout: pending → lines, payments and slices → seal, with the rollup, payment summary, `customer_stats` and `batch_item_stock` values checked.
      - The 5% discount accepted and the 6% discount rejected without an approver. A price override rejected without, and accepted with, a supervisor approver.
      - A cashier void rejected without an approver and when self-approved. A void with a supervisor approval, then exact slice reversal, an over-reversal rejected, `v_integrity_void_unreversed` empty, and the rollups and `customer_stats` recomputed.
      - Session closed ⇒ void refused for every role, and a reopen refused.
      - **The pull path:** with `sync_ctx.applying = 1`, another counter's *closed* session, then its completed order, a custom line, a payment, and the void update, all applied parents-first. Assert the per-device rollups, the payment summary and `quick_sale_stats` reconcile.
      - The audit chain guard rejecting a seq gap and a wrong `prev_hash`.
    - `tests/fts.rs`: prefix and trigram tokenizers available in the bundled SQLite.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test migration_invariants --test fts`
    - `pnpm verify`

- [x] **0.4 Errors, clock, state and typed IPC**: done 2026-11-08
  - **Deliverables:**
    - `error.rs`: `AppError` as stable codes (no display strings for the UI).
    - `clock.rs` (`Clock`, `SystemClock`, `ManualClock`; no `.expect`).
    - `state.rs`.
    - The tauri-specta builder, plus a `#[test] export_bindings` that writes `src/db/bindings.ts`.
    - `src/db/client.ts`, which maps `AppError` kinds to `errors.<KIND>` i18n keys.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml export_bindings`
    - `git diff --exit-code -- src/db/bindings.ts` after the first commit of bindings (on this first block, the commit includes them)
    - `pnpm typecheck`
    - `pnpm verify`

- [x] **0.5 Audit core**: done 2026-11-08
  - **Deliverables:**
    - `audit/{mod,chain}.rs`: `record()` inside the caller's transaction, `verify()`, canonical JSON (sorted keys, fixed field order).
    - `tests/audit_chain.rs`: a golden hash; a tamper test (drop the trigger in a test DB, edit a row, expect the break); the chain guard rejecting gaps and wrong `prev_hash`.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --lib audit::`
    - `cargo test --manifest-path src-tauri/Cargo.toml --test audit_chain`
    - `pnpm verify`

- [ ] **0.6 Auth and RBAC core**
  - **Deliverables:**
    - `auth/{pin,session,approvals,guard}.rs`: argon2id (fast params in tests only), 5-strike / 5-minute lockout, idle auto-lock, single-use 60 s approval tokens bound to (perm, target, requester), `require()` returning `Forbidden` vs `ApprovalRequired`, the owner invariant.
    - `tests/rbac.rs`, using `ManualClock`.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --lib auth::`
    - `cargo test --manifest-path src-tauri/Cargo.toml --test rbac`
    - `pnpm verify`

- [ ] **0.7 First-run setup, identity and seeds**
  - **Deliverables:**
    - `commands/{setup,system,auth}.rs`: generate `store_id`/`device_id`/`device_code`; seed in one transaction the 4 roles with masks (Cashier 500 bp), the Owner (PIN ≥ 6), expense categories, the `OPENING-STOCK`/`ADJUSTMENTS` batches, every §11 store setting (the **footer verbatim**), and the device settings (`ui.default_language=en`, label templates **203 dpi, 2 mm row gap**).
    - Outbox rows for seeds, and one `STORE_INITIALIZED` audit row.
    - `tests/first_run.rs`, which asserts every seeded value exactly.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test first_run`
    - `pnpm verify`

- [ ] **0.8 Frontend foundation and design system**
  - **Deliverables:**
    - Tailwind v4 `@theme` tokens from doc §1.9 (light and dark).
    - Fonts bundled: Inter, Noto Sans Sinhala, JetBrains Mono.
    - shadcn components with `size="pos"`/`"pos-lg"`, plus `<Kbd>`, `<Can>`, `LanguageToggle`, `ThemeToggle`.
    - i18n (`src/i18n`, en + si), `scripts/i18n-check.mjs`, `scripts/contrast-check.mjs` (asserts every token pair against its ratio in §1.9).
    - The router and the `PosLayout`/`BackOfficeLayout` shells.
    - The shared `DataTable` (TanStack Table v8 + Virtual), the TanStack Query client, and the bundled audio cues.
    - Vitest tests for `money.ts`, `phone.ts`, the DataTable virtualisation and the i18n fallback.
  - **Gate:**
    - `pnpm i18n:check`
    - `pnpm contrast:check`
    - `pnpm test`
    - `pnpm build`
    - `pnpm verify`

- [ ] **0.9 Phase 0 screens and flows**
  - **Deliverables:**
    - The first-run wizard (EN/SI), PIN login and lock screen, the session header, `ApprovalDialog`, the audit log viewer (DataTable, keyset paging) with "Verify chain", the language and theme toggles, and a window `minWidth/minHeight` of 1024×768.
    - Commands wired end to end.
  - **Gate:**
    - `pnpm verify`
    - `pnpm tauri build --no-bundle`
    - **The manual smoke checklist, written to `docs/qa/phase0-smoke.md`:**
      - Launch at 1024×768 and 1920×1080.
      - Complete the wizard in EN and SI.
      - PIN login offline.
      - A cashier gets `Forbidden` on an admin command.
      - A supervisor approval works once.
      - The chain verifies.

- [ ] **0.10 Continuous integration**
  - **Deliverables:** `.github/workflows/ci.yml` (`windows-latest`: pnpm + Rust cache, `pnpm verify`, `pnpm tauri build --no-bundle`).
  - **Gate:**
    - `pnpm verify` locally.
    - CI green needs a GitHub remote, which is **the owner's decision**. Until then, record "CI pending remote" in the block note.
  - **Phase 0 exit:** doc §8 Phase 0 criteria.

### Phase 1: Offline MVP till

- [ ] **1.1 Products, categories and search**
  - **Deliverables:**
    - CRUD commands + UI, including `name_si`.
    - SKU and in-store barcode generators.
    - FTS search (prefix + Sinhala trigram).
    - `tests/fts_sinhala_names.rs` (20 real Sinhala names; record the tokenizer result in the doc).
    - `tests/perf_budgets.rs` (search and lookup rows of §13.4).
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test fts_sinhala_names`
    - `cargo test --manifest-path src-tauri/Cargo.toml --release --test perf_budgets -- --ignored search`
    - `pnpm verify`

- [ ] **1.2 Minimal batches, stock ledger and FIFO**
  - **Deliverables:** draft → receive (manual cost), opening stock, `batch_item_stock`/`stock_levels` derivation, FIFO consumption service, `tests/stock_ledger.rs`.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test stock_ledger`
    - `pnpm verify`

- [ ] **1.3 Cash sessions and the Z-report**
  - **Deliverables:**
    - Open and close sessions, count and variance, pay-in and pay-out, and the no-sale command (`DRAWER_NO_SALE`, audited).
    - The Z-report data model.
    - `tests/cash_session.rs`, including "a closed session can't reopen".
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test cash_session`
    - `pnpm verify`

- [ ] **1.4 Checkout engine (the heart)**
  - **Deliverables:**
    - `checkout_order` with the **seal pattern**, in exactly the step order of doc §8 Phase 1: token check → customer → receipt counter → insert `pending` → lines, payments and slices → seal → audit, held-cart delete, print job, outbox. It covers the idempotent `checkout_token`, line and bill discounts with apportionment, the 5% limit and approvals, price override, payments ≥ 0 with cash change, the customer link, the receipt counter, deletion of the `__current__` held cart, the `print_jobs` row, audit and outbox.
    - Tests:
      - `tests/checkout.rs`
      - `tests/checkout_props.rs` (proptest: random carts ⇒ Rust totals == SQL seal == UI preview fixtures)
      - `tests/crash_mid_checkout.rs` (fault injection at every await ⇒ nothing or everything)
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test checkout --test checkout_props --test crash_mid_checkout`
    - `pnpm verify`

- [ ] **1.5 Void engine**
  - **Deliverables:**
    - `void_order`: whole order only, same device and open session, the reason list (note required for `OTHER`), and supervisor approval for cashiers.
    - Exact slice reversal, rollup removal, and print jobs for the VOID slip and drawer kick.
    - `tests/void_reversal.rs`, including "closed session ⇒ refused for every role" and `v_integrity_void_unreversed` empty.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test void_reversal`
    - `pnpm verify`

- [ ] **1.6 Customers at the till**
  - **Deliverables:**
    - `customer_lookup` and create-at-checkout using `UUIDv5(phone)`.
    - The masked phone on receipts.
    - Erasure-safe design (no phone stored on orders).
    - `tests/customers.rs`.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test customers`
    - `pnpm verify`

- [ ] **1.7 Printing pipeline**
  - **Deliverables:**
    - Transports: spooler RAW, TCP 9100 and serial.
    - The ESC/POS builder: footer, QR, cut, drawer kick.
    - The per-printer worker: one in-flight job, timeouts, the unhealthy state, `DLE EOT` status, retries, and startup recovery of unprinted jobs.
    - The test page.
    - `tests/print_pipeline.rs`, with a fake transport that hangs, errors and reports paper-out. It asserts that a checkout takes < 100 ms while the printer hangs and that no thread pile-up happens.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test print_pipeline`
    - `pnpm verify`

- [ ] **1.8 Scanner capture and feedback**
  - **Deliverables:**
    - `ScannerProvider`: prefix capture (F13), the timing fallback with leak removal and replay, `event.code` decoding, `Enter` swallowing, the FIFO queue.
    - Audio cues, plus the unknown-barcode toast and Quick Sale prefill.
    - `src/hardware/scanner/scanner.race.test.ts`.
  - **Gate:**
    - `pnpm exec vitest run src/hardware/scanner`
    - `pnpm verify`

- [ ] **1.9 Till UI**
  - **Deliverables:**
    - The POS screen per doc §1.9 layout (1024×768 baseline).
    - The cart (virtualised), the Quick Sale bar with `parseQuickSale`, the phone field, the payment dialog (split, cash change), held carts with debounced recovery, the shortcut map with `<Kbd>` badges, and micro-interactions.
    - Vitest tests for `parseQuickSale`, the cart store and shortcut routing.
  - **Gate:**
    - `pnpm exec vitest run src/features/pos`
    - `pnpm verify`

- [ ] **1.10 Manual "Backup Database Now"**
  - **Deliverables:**
    - `backup_now` (folder picker, `VACUUM INTO` on a read connection, `.partial` → integrity check → SHA-256 → manifest → rename), the audit rows `BACKUP_CREATE`/`BACKUP_FAILED`, and the reminder banner.
    - `tests/backup.rs`, including "concurrent checkout during backup" and "destination removed mid-write ⇒ no `.partial` left".
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test backup`
    - `pnpm verify`

- [ ] **1.11 Staff management**
  - **Deliverables:** create staff, assign role, reset PIN, set UI language, grant and deny overrides (`USER_MANAGE`), the owner invariant in the UI, `tests/staff.rs`.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --test staff`
    - `pnpm verify`

- [ ] **1.12 Phase 1 acceptance**
  - **Deliverables:**
    - WebdriverIO + `tauri-driver` e2e suite in `tests/e2e/phase1.e2e.ts`, scripting doc §8 Phase 1 exit criteria.
    - `docs/qa/phase1-acceptance.md`: touch-only and keyboard-only runs in EN and SI, a pull-the-plug drill, and real printer and scanner checks.
  - **Gate:**
    - `pnpm e2e` (needs `msedgedriver` matching WebView2)
    - `pnpm verify`
    - `pnpm tauri build --no-bundle`

### Phase 2: Batches, labels, customers, expenses and reports
- [ ] **2.1 Batch intake wizard and lifecycle**
  - **Deliverables:** allocation methods with live variance, receive, reopen, late items, close.
  - **Gate:** `--test batches` + `pnpm verify`
- [ ] **2.2 Label engine**
  - **Deliverables:** `label/{layout,templates,tspl,zpl}.rs` (2-up 2″×1″, 3-up 1″×1″, 203 dpi, 2 mm gap), barcode-fit rules, the SVG preview, the alignment test and calibration.
  - **Gate:** `--test labels` (golden TSPL/ZPL byte snapshots, barcode-fit cases) + `pnpm verify`
- [ ] **2.3 Stock operations**
  - **Deliverables:** adjustments, damage, counts, movement history, valuation (now and as-of), oversold layers.
  - **Gate:** `--test stock_ops` + `pnpm verify`
- [ ] **2.4 Quick-sale review and promotion**
  - **Deliverables:** the review screen over `quick_sale_stats`, promote and ignore.
  - **Gate:** `--test quick_sales` + `pnpm verify`
- [ ] **2.5 Customer back office**
  - **Deliverables:** list, profile, history, analytics, erasure.
  - **Gate:** `--test customer_backoffice` + `pnpm verify`
- [ ] **2.6 Expenses**
  - **Deliverables:** categories, entry, drawer pay-out integration.
  - **Gate:** `--test expenses` + `pnpm verify`
- [ ] **2.7 Reports engine**
  - **Deliverables:** all §5 periods and metrics, plus `rebuild_derived`.
  - **Gate:** `--test reports` (rollups == raw sums on random data; monthly == Σ daily) + `pnpm verify`
- [ ] **2.8 Charts and dashboards**
  - **Deliverables:** shadcn Chart components per doc §1.4, both themes, both languages.
  - **Gate:** `pnpm exec vitest run src/components/charts` + `pnpm verify`
- [ ] **2.9 Back-office tables and export**
  - **Deliverables:** all lists on keyset DataTables, CSV export (`DATA_EXPORT`, audited).
  - **Gate:** `--test export` + `pnpm verify`
- [ ] **2.10 Phase 2 acceptance and scale**
  - **Deliverables:** the full §13.4 budget table, including the 1M-item headroom run, a 60 fps scroll benchmark, `tests/e2e/phase2.e2e.ts`, and `docs/qa/phase2-acceptance.md`.
  - **Gate:**
    - `cargo test --manifest-path src-tauri/Cargo.toml --release --test perf_budgets -- --ignored`
    - `pnpm e2e`
    - `pnpm verify`

(`--test X` means `cargo test --manifest-path src-tauri/Cargo.toml --test X`.)

### Phase 3: Cloud sync (needs a Supabase project, Supabase CLI and Docker Desktop, so **ask the owner first**)
- [ ] **3.1 Supabase schema**
  - **Deliverables:** `supabase/migrations` (mirror of §4.3 + `server_seq`, `processed_ops`, `devices`, RLS incl. no-UPDATE/DELETE policies), pgTAP tests.
  - **Gate:** `supabase test db` + `pnpm verify`
- [ ] **3.2 Sync RPCs**
  - **Deliverables:** `sync_push` (advisory lock, idempotency, invariants: owner, cost lock, order state machine, audit chain), `sync_pull` (global cursor, parent closure).
  - **Gate:** `supabase test db` + `pnpm verify`
- [ ] **3.3 Device registration**
  - **Deliverables:** an edge function that adopts the existing ids, with the token stored in the Windows keyring.
  - **Gate:** `--test registration` + `pnpm verify`
- [ ] **3.4 Rust sync worker**
  - **Deliverables:** outbox push, pull apply (`sync_ctx` + topological order + apply rules), barcode-conflict handling, `sync_logs`.
  - **Gate:** `--test sync_apply` + `pnpm verify`
- [ ] **3.5 Bootstrap and sync UI**
  - **Deliverables:** full-history bootstrap with progress, the status pill, and Settings → Sync.
  - **Gate:** `--test bootstrap` (limit=7, parents edited after children) + `pnpm verify`
- [ ] **3.6 Chaos suite**
  - **Deliverables:** a three-terminal harness with toxiproxy covering the doc §8 Phase 3 exit list, including `three_counters_oversell_last_units`.
  - **Gate:** `cargo test --manifest-path src-tauri/Cargo.toml --test chaos -- --ignored` + `pnpm verify`

### Phase 4: Production hardening
- [ ] **4.1 Automatic backups and restore**
  - **Deliverables:** hourly/daily rotation, restore with recovery (online/offline paths).
  - **Gate:** `--test restore` + `pnpm verify`
- [ ] **4.2 Updater, signing and installer**
  - **Deliverables:** `tauri-plugin-updater`, NSIS with WebView2 bootstrapper. Code-signing certificates are **the owner's purchase**.
  - **Gate:** `pnpm tauri build` + a signed-update smoke test + `pnpm verify`
- [ ] **4.3 Security hardening**
  - **Deliverables:** CSP, capability audit, optional SQLCipher, PIN-length enforcement, RLS foreign-token tests.
  - **Gate:** `--test security` + `pnpm verify`
- [ ] **4.4 Observability**
  - **Deliverables:** rotating logs, a support bundle (logs, sync_logs, integrity probes, chain verify).
  - **Gate:** `--test support_bundle` + `pnpm verify`
- [ ] **4.5 Pilot readiness**
  - **Deliverables:** a soak test (8 h scripted selling at 3 counters), the final perf budgets, and `docs/qa/pilot-checklist.md`.
  - **Gate:** the soak test + `pnpm verify` + `pnpm tauri build`

---

## 6. Commands cheat-sheet

```bash
# Git Bash on this machine: cargo is installed but not on PATH by default
export PATH="$HOME/.cargo/bin:$PATH"

pnpm install                      # JS deps
pnpm dev                          # Vite dev server only (UI in a browser, no Rust)
pnpm tauri dev                    # full desktop app, hot reload
pnpm build                        # tsc + vite build
pnpm tauri build --no-bundle      # release binary, no installer
pnpm tauri build                  # installer (Phase 4)
pnpm test                         # vitest run
pnpm verify                       # the base gate (§5)

cargo test   --manifest-path src-tauri/Cargo.toml                       # all Rust tests
cargo test   --manifest-path src-tauri/Cargo.toml --test <name>         # one integration test
cargo test   --manifest-path src-tauri/Cargo.toml --release --test perf_budgets -- --ignored
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt    --manifest-path src-tauri/Cargo.toml --all -- --check
```

**Environment notes:**
- The path contains a space (`D:\Program Data\...`), so always quote it.
- `cargo search` hangs on this network, so let `cargo add` resolve versions.
- `shadcn init` stalls on a prompt, so use `components.json` + `shadcn add … -y`, and check imports afterwards. They came out as `from "cn"`.
- The first Rust build takes minutes. Run it in the background.
- Pinned versions: `tauri-specta =2.0.0-rc.26`, `specta =2.0.0-rc.26`, `specta-typescript 0.0.13`, and `@tanstack/react-table` **^8** on purpose.
