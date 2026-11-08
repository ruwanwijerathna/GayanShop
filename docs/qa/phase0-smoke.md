# Phase 0 smoke checklist

> Block 0.9 gate (AGENT.md §5). Each item shows **how it was verified**:
> - **Automated**: a named test that runs in `pnpm verify`.
> - **Launch**: the release binary started on the dev machine.
> - **GUI-auto**: the real release app driven by the coding agent through the WebView2 DevTools protocol (CDP) and Win32 calls. It clicks buttons, types into fields, reads the screen and takes screenshots. It is the real app, not a mock, but it is not a person at the counter.
> - **Manual**: needs a person at the screen.
>
> The owner may re-run any GUI-auto item by hand on the counter PC. Items still marked **Manual — pending** could not be automated.

Environment: Windows 10 Pro 19045 dev machine with a 1920×1080 screen at 125 % scaling, rustc 1.99.1, Node 24, pnpm 11, WebView2 157.
Build: `pnpm tauri build --no-bundle` → `src-tauri/target/release/gayanshop-pos.exe`.

GUI-auto runs on 2026-11-08 used fresh data folders. The earlier folders were renamed, not deleted, to `%APPDATA%\com.gayanshop.pos.bak-*`, and are safe to delete. The smoke run found three defects. Each was fixed in its own commit, and the affected items were re-run on a rebuilt exe:

- `7d49fa3 fix(block-0.9)`: the language toggle, Ctrl+Shift+L and the wizard's language step were reverted instantly. The audit table also overflowed at 1024×768.
- `ed06533 fix(block-0.8)`: `<html lang>` did not follow the UI language.

## 1. Build and launch

| # | Check | Verified by | Result |
|---|---|---|---|
| 1.1 | Release binary builds | Launch: `pnpm tauri build --no-bundle`, exit 0, 9.9 MB exe. Also reproduced from a **clean clone**: `pnpm install --frozen-lockfile`, `pnpm verify`, then `pnpm tauri build --no-bundle`, which are exactly the CI steps. All exit 0, and the clone's tree stayed clean. | ✅ 2026-11-08 |
| 1.2 | Binary starts and stays running; creates `%APPDATA%\com.gayanshop.pos\pos.db` and applies the migration | Launch: process alive, window title "GayanShop POS". `pos.db` + `-wal` + `-shm` created; `_sqlx_migrations` = version 1 ok; 48 tables, 33 triggers; `journal_mode=wal`; `quick_check=ok` | ✅ 2026-11-08 |
| 1.3a | Window opens at 1366×768 | GUI-auto (Win32 `GetClientRect`, DPI-aware): client area 1708×960 physical = **1366×768 logical** at 125 %. Maximize → 1536×802 logical (the full screen); restore → 1366×768 again. | ✅ 2026-11-08 |
| 1.3b | Windows Snap cannot make it smaller than 1024×768 | GUI-auto: Win+← (left half = 768 logical wide on this screen) leaves the window at **1037×793 logical**. The OS applies the configured minimum. | ✅ 2026-11-08 |
| 1.3c | Dragging the border cannot make it smaller than 1024×768 | Same minimum-size path as 1.3b. Injected mouse drags and Alt+Space keyboard resizes did not reach the window from the agent's session. **Manual — pending** (30-second check on the counter PC). | ☐ |
| 1.4 | Layout correct at 1024×768 and 1920×1080, with no clipped controls | GUI-auto: CDP viewport emulation at both sizes on the wizard, login, counter-status and audit screens, in EN and SI. No horizontal page overflow, no control outside the viewport, and no button under 44 px. Screenshots reviewed. The audit table overflowed at 1024 px (950 px of columns in a ~895 px grid); fixed in `7d49fa3` and re-checked: grid 911/911 px. | ✅ 2026-11-08 |

## 2. First-run wizard (EN and SI)

| # | Check | Verified by | Result |
|---|---|---|---|
| 2.1 | Fresh counter opens the wizard | Automated: `App.test.tsx › starts the first-run wizard` | ✅ |
| 2.2 | Choosing සිංහල switches the whole wizard to Sinhala instantly | Automated: `FirstRunWizard.test.tsx › runs in Sinhala…` and, inside `App`, `App.test.tsx › lets the wizard switch to Sinhala…`. GUI-auto: every step shows Sinhala text and `<html lang="si">`. On the real app this switch was broken (reverted at once) until `7d49fa3`. | ✅ 2026-11-08 |
| 2.3 | Setup seeds every owner-confirmed value, including the verbatim receipt footer, roles, 203 dpi / 2 mm label templates, and system batches | Automated: `tests/first_run.rs` (7 tests) | ✅ |
| 2.4 | Mismatched PINs and invalid input are refused, with translated messages | Automated: `FirstRunWizard.test.tsx`, `first_run.rs › rejects_invalid_input…`. GUI-auto: "The two PINs do not match." (EN) and "PIN අංක දෙක නොගැලපේ." (SI) | ✅ 2026-11-08 |
| 2.5 | Complete the wizard on the real app in EN, then again (fresh data folder) in SI | GUI-auto: EN run (owner "Gayan"), then a fresh-folder SI run on the rebuilt exe (owner "ගයාන්"). Both reach the login screen. | ✅ 2026-11-08 |
| 2.6 | Sinhala text renders in Noto Sans Sinhala, with no tofu (□) glyphs | GUI-auto: `document.fonts.check('16px "Noto Sans Sinhala"', 'සිංහල')` = true, and the bundled font is loaded (`U+0D81–0DF4`). Screenshots of the SI wizard, counter status and audit log were reviewed, with no tofu found. A native speaker should still review the wording. | ✅ 2026-11-08 |

## 3. PIN login (offline)

| # | Check | Verified by | Result |
|---|---|---|---|
| 3.1 | Staff picker, then a touch PIN pad or the keyboard, then login | Automated: `LoginScreen.test.tsx`. GUI-auto: on-screen PIN pad login works. | ✅ 2026-11-08 |
| 3.2 | Wrong PIN shows the attempts left; 5 failures lock for 5 minutes; all audited | Automated: `tests/rbac.rs › five_wrong_pins_lock_for_five_minutes`. GUI-auto: a wrong PIN shows "Incorrect PIN. 4 attempts left." and writes a "Wrong PIN" audit row. | ✅ 2026-11-08 |
| 3.3 | Idle auto-lock after 5 minutes returns to login with "locked after inactivity" | Automated: `rbac.rs › idle_session_locks`, `LoginScreen.test.tsx › explains an idle lock` | ✅ |
| 3.4a | Setup and login need no network | GUI-auto: the CDP Network log during setup and login shows only `ipc.localhost` (the app's own IPC) and `tauri.localhost` (bundled assets). `cargo tree` has no HTTP client crate. | ✅ 2026-11-08 |
| 3.4b | Log in on the counter PC with the network cable physically unplugged | The agent did not touch network adapters. **Manual — optional** (3.4a already shows nothing leaves the machine). | ☐ |

## 4. Permissions and approvals

| # | Check | Verified by | Result |
|---|---|---|---|
| 4.1 | A cashier gets `Forbidden` on an admin command (`AUDIT_VIEW`, `USER_MANAGE`), and the audit nav is hidden | Automated: `rbac.rs › guard_distinguishes…`, `App.test.tsx › hides the audit log from a cashier` | ✅ |
| 4.2 | A supervisor approval works **once**, only for its target, and expires after 60 s; no self-approval | Automated: `rbac.rs › supervisor_approval_works_exactly_once…`, `› approval_rules`, `ApprovalDialog.test.tsx` | ✅ |
| 4.3 | On the real app: log in as a Cashier, confirm "Audit log" is absent; log in as Owner, confirm it is present | GUI-auto: Owner sees the "Audit log" link. Cashier: the link is absent, and opening `#/audit` directly shows "You don't have permission to do this." (Rust refuses `AUDIT_VIEW`). Phase 0 has no staff-management screen, so the test cashier was inserted into the throw-away test database by SQL. | ✅ 2026-11-08 |

## 5. Audit trail

| # | Check | Verified by | Result |
|---|---|---|---|
| 5.1 | Every login, failure, lockout and approval writes an audit row in the same transaction | Automated: `rbac.rs`, `audit_chain.rs` | ✅ |
| 5.2 | "Verify chain" reports intact chains, and detects an edited or relinked row | Automated: `audit_chain.rs › tampering_is_detected`, `› relinking_a_forged_row…` | ✅ |
| 5.3 | On the real app: open the Audit log as Owner, scroll, press "Verify chain" → "intact" | GUI-auto: EN "01a1bb92: 3 entries — intact"; SI "01a1bba4: සටහන් 2 — නිවැරදියි". With only a handful of rows there is nothing to scroll; virtual scrolling is covered by `DataTable.test.tsx`. | ✅ 2026-11-08 |

## 6. Preferences

| # | Check | Verified by | Result |
|---|---|---|---|
| 6.1 | Language toggle EN ↔ සිං (and Ctrl+Shift+L) switches instantly and is saved per staff member | Automated: `TopBar.test.tsx`, `tests/prefs.rs`, `App.test.tsx › switches language live…`, `i18n.test.ts › keeps <html lang> in step`. GUI-auto on the rebuilt exe: button and shortcut both switch instantly, `<html lang>` follows, and `staff.ui_language` is saved. Before `7d49fa3` the switch was saved but not shown until the next login. | ✅ 2026-11-08 |
| 6.2 | Theme toggle light ↔ dark is saved per counter | Automated: `tests/prefs.rs` | ✅ |
| 6.3 | On the real app: switch to dark, restart, and confirm it is still dark; Ctrl+L locks | GUI-auto: dark → Ctrl+L returns to "Who is working?" → process killed and relaunched → still dark. | ✅ 2026-11-08 |

## Observations (not defects in Phase 0 scope)

- Role names ("Owner", "Cashier") show in English in the Sinhala UI because they are seeded data, not translation keys. Decide in Phase 1 (staff management) whether system roles get translated labels.
- With a single staff member, the login tile sits in the left column of the two-column grid rather than centered. This is cosmetic.
- An external Win32 `SetWindowPos` call (not a user action) left the window with a 65535 px height. Maximize and restore by the user behave correctly. Snap itself is verified (1.3b).
