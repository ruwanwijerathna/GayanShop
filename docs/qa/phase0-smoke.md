# Phase 0 smoke checklist

> Block 0.9 gate (AGENT.md §5). Each item shows **how it was verified**:
> - **Automated**: a named test that runs in `pnpm verify`.
> - **Launch**: the release binary started on the dev machine.
> - **Manual**: needs a person at the screen.
>
> The coding agent cannot click a GUI, so items marked **Manual — pending** must be run by the owner or a tester on the counter PC. Tick each one and add the date.

Environment: Windows 10 Pro 19045 dev machine, rustc 1.99.1, Node 24, pnpm 11, WebView2 runtime.
Build: `pnpm tauri build --no-bundle` → `src-tauri/target/release/gayanshop-pos.exe`.

## 1. Build and launch

| # | Check | Verified by | Result |
|---|---|---|---|
| 1.1 | Release binary builds | Launch: `pnpm tauri build --no-bundle` (9 min 07 s, exit 0, 9.9 MB exe) | ✅ 2026-11-08 |
| 1.2 | Binary starts and stays running; creates `%APPDATA%\com.gayanshop.pos\pos.db` and applies the migration | Launch: process alive after 10 s, window title "GayanShop POS", 31 MB working set. `pos.db` + `-wal` + `-shm` created; `_sqlx_migrations` = version 1 ok; 48 tables, 33 triggers; `journal_mode=wal`; `quick_check=ok`; not yet initialized (wizard pending) | ✅ 2026-11-08 |
| 1.3 | Window opens at 1366×768 and cannot be resized below 1024×768 | `tauri.conf.json` (`minWidth`/`minHeight`) + **Manual — pending** | ☐ |
| 1.4 | Layout correct at 1024×768 and 1920×1080, with no clipped controls | **Manual — pending** | ☐ |

## 2. First-run wizard (EN and SI)

| # | Check | Verified by | Result |
|---|---|---|---|
| 2.1 | Fresh counter opens the wizard | Automated: `App.test.tsx › starts the first-run wizard` | ✅ |
| 2.2 | Choosing සිංහල switches the whole wizard to Sinhala instantly | Automated: `FirstRunWizard.test.tsx › runs in Sinhala…` | ✅ |
| 2.3 | Setup seeds every owner-confirmed value, including the verbatim receipt footer, roles, 203 dpi / 2 mm label templates, and system batches | Automated: `tests/first_run.rs` (7 tests) | ✅ |
| 2.4 | Mismatched PINs and invalid input are refused, with translated messages | Automated: `FirstRunWizard.test.tsx`, `first_run.rs › rejects_invalid_input…` | ✅ |
| 2.5 | Complete the wizard on the real app in EN, then again (fresh data folder) in SI | **Manual — pending** | ☐ |
| 2.6 | Sinhala text renders in Noto Sans Sinhala, with no tofu (□) glyphs | **Manual — pending** | ☐ |

## 3. PIN login (offline)

| # | Check | Verified by | Result |
|---|---|---|---|
| 3.1 | Staff picker, then a touch PIN pad or the keyboard, then login | Automated: `LoginScreen.test.tsx` | ✅ |
| 3.2 | Wrong PIN shows the attempts left; 5 failures lock for 5 minutes; all audited | Automated: `tests/rbac.rs › five_wrong_pins_lock_for_five_minutes` | ✅ |
| 3.3 | Idle auto-lock after 5 minutes returns to login with "locked after inactivity" | Automated: `rbac.rs › idle_session_locks`, `LoginScreen.test.tsx › explains an idle lock` | ✅ |
| 3.4 | Log in on the real app with the network unplugged | **Manual — pending** | ☐ |

## 4. Permissions and approvals

| # | Check | Verified by | Result |
|---|---|---|---|
| 4.1 | A cashier gets `Forbidden` on an admin command (`AUDIT_VIEW`, `USER_MANAGE`), and the audit nav is hidden | Automated: `rbac.rs › guard_distinguishes…`, `App.test.tsx › hides the audit log from a cashier` | ✅ |
| 4.2 | A supervisor approval works **once**, only for its target, and expires after 60 s; no self-approval | Automated: `rbac.rs › supervisor_approval_works_exactly_once…`, `› approval_rules`, `ApprovalDialog.test.tsx` | ✅ |
| 4.3 | On the real app: log in as a Cashier, confirm "Audit log" is absent; log in as Owner, confirm it is present | **Manual — pending** | ☐ |

## 5. Audit trail

| # | Check | Verified by | Result |
|---|---|---|---|
| 5.1 | Every login, failure, lockout and approval writes an audit row in the same transaction | Automated: `rbac.rs`, `audit_chain.rs` | ✅ |
| 5.2 | "Verify chain" reports intact chains, and detects an edited or relinked row | Automated: `audit_chain.rs › tampering_is_detected`, `› relinking_a_forged_row…` | ✅ |
| 5.3 | On the real app: open the Audit log as Owner, scroll, press "Verify chain" → "intact" | **Manual — pending** | ☐ |

## 6. Preferences

| # | Check | Verified by | Result |
|---|---|---|---|
| 6.1 | Language toggle EN ↔ සිං (and Ctrl+Shift+L) switches instantly and is saved per staff member | Automated: `TopBar.test.tsx`, `tests/prefs.rs` | ✅ |
| 6.2 | Theme toggle light ↔ dark is saved per counter | Automated: `tests/prefs.rs` | ✅ |
| 6.3 | On the real app: switch to dark, restart, and confirm it is still dark; Ctrl+L locks | **Manual — pending** | ☐ |
