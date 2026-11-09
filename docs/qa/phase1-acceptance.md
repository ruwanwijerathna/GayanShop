# Phase 1 acceptance

> Block 1.12 gate (AGENT.md §5). Each item shows **how it was verified**:
> - **E2E**: `pnpm e2e`: WebdriverIO + `tauri-driver` drive the real release binary through WebView2, in a throwaway data folder (`GAYANSHOP_DATA_DIR`), keyboard only. Spec: `tests/e2e/phase1.e2e.ts`.
> - **Automated**: a named Rust or Vitest test that runs in `pnpm verify`.
> - **Manual**: needs a person at the counter, or hardware this machine doesn't have (receipt printer, cash drawer, scanner, touchscreen, a plug to pull).
>
> Items marked **Manual — pending** are for the owner on the counter PC. None of them blocks the code; they qualify the hardware and the people.

Environment: Windows 10 Pro 19045 dev machine, 1366×768 logical window (1708×960 at 125 %), WebView2 157.0.4341.42, msedgedriver 157.0.4341.42, tauri-driver 2.1.0, rustc 1.99.1, Node 24, pnpm 11.
Build: `pnpm tauri build --no-bundle` → `src-tauri/target/release/gayanshop-pos.exe`.

## Running the e2e suite

```bash
cargo install tauri-driver --locked                 # once
# msedgedriver must match the WebView2 runtime (Edge WebView2 version in Apps & features):
#   https://msedgedriver.microsoft.com/<version>/edgedriver_win64.zip → msedgedriver.exe into ~/.cargo/bin
pnpm e2e                                            # builds the release exe, then runs tests/e2e/phase1.e2e.ts
E2E_SKIP_BUILD=1 pnpm e2e                           # reuse an existing release build
```

The suite never touches `%APPDATA%\com.gayanshop.pos`. Close any running GayanShop first, because the single-instance guard would hand the session to it. A failing step saves a screenshot to `%TEMP%\gayanshop-e2e-<step>.png`.

The first runs found two real defects, both fixed in this block:
- **The till scrolled at 1366×768.** With the printer banner showing, PAY was pushed below the fold. The till now fills exactly the space left under the banners, and the e2e asserts PAY is inside the viewport.
- The e2e itself first typed at WebDriver speed (< 30 ms per key, then Enter). The scanner's timing fallback correctly took that for a barcode scan, so the suite now types at a person's pace. This confirms §7.1 on the real app.

## 1. Doc §8 Phase 1 exit criteria

| # | Exit criterion | Verified by | Result |
|---|---|---|---|
| 1.1 | Offline: a cashier completes mixed sales (scanned/searched plus F2 quick sales, some with a customer phone) | E2E: 5 sales, each a quick sale (`n* Plastic Basin 10L 300`) plus a searched catalog item, the first with phone `077 123 4567` ("New customer"), all paid cash with change. No network is used by the app at all in Phase 1. Automated: `checkout.rs`, `checkout_props.rs` (48 random carts), `customers.rs`. | ✅ 2026-11-09 |
| 1.2 | A quick sale is F2, typing and Enter, with no mouse; a phone is F4, typing and Enter | E2E (keyboard only). Automated: `PosScreen.test.tsx`, `parseQuickSale.test.ts`. | ✅ 2026-11-09 |
| 1.3 | A cashier's price override prompts for supervisor approval and is audited | Automated: `checkout.rs` (override needs approval; `PRICE_OVERRIDE` audit; below `min_price` always needs someone else). UI path: `PosScreen.test.tsx` (step-up then retry with the approval). | ✅ |
| 1.4 | A 6% bill discount by a cashier prompts for approval; a 5% one is accepted and its shares sum exactly | E2E: 5% → Rs 427.50, no prompt; 6% → "Supervisor approval", Kamal approves, sale completes. Automated: `checkout.rs`, `pricing.test.ts` (TS preview == Rust on the golden carts). | ✅ 2026-11-09 |
| 1.5 | A cashier's void prompts for approval, restores stock to the exact layers and drops out of the Z-report | E2E: F10 → void → Kamal approves → "voided". Automated: `void_reversal.rs` (exact slice reversal, rollups, `v_integrity_void_unreversed` empty), `cash_session.rs` (Z-report voids by reason and approver). | ✅ 2026-11-09 |
| 1.6 | After the session closes, the same void is refused for every role | E2E: the Owner closes the session with the Z-report; the till then refuses to sell ("Open a cash session before selling."), so no void list is reachable. Automated: `void_reversal.rs › closed session ⇒ refused for every role`, and the SQL trigger. | ✅ 2026-11-09 |
| 1.7 | A negative payment is rejected by Rust and by the SQL `CHECK` | Automated: `checkout.rs` (`PAYMENT_NEGATIVE`) and `migration_invariants.rs` (the CHECK). The UI has no way to enter one. | ✅ |
| 1.8 | Pulling the plug mid-sale loses nothing that was committed and sells nothing twice | Automated: `crash_mid_checkout.rs` (fault at every step and every await ⇒ nothing or everything), `held_carts.rs` (a sold cart is never offered back). **Manual — pending:** pull the power on the counter PC during a sale, then restart: the sale is either complete with its receipt queued for "Print now / Skip", or absent with "Recover cart?". | ☐ |
| 1.9 | Backup to USB works while selling | Automated: `backup.rs` (concurrent checkout, consistent snapshot; USB pulled ⇒ no `.partial`). **Manual — pending:** "Backup Database Now" to a real USB stick, then open the `.db.json` manifest. | ☐ |

## 2. Hardware (the owner's printer, drawer and scanner)

| # | Check | Verified by | Result |
|---|---|---|---|
| 2.1 | Receipt printer prints the test page (ruler, code page, QR, cut) and kicks the drawer | Automated: `print_pipeline.rs` against a fake transport and a real TCP 9100 socket; ESC/POS byte tests. **Manual — pending:** Settings → Printer, choose USB / network / COM, "Print test page". | ☐ |
| 2.2 | A cash sale kicks the drawer *before* the receipt; the receipt shows the masked phone and the footer verbatim | Automated: `print_pipeline.rs › a_cash_sale_kicks_the_drawer_then_prints_the_receipt`. **Manual — pending** on the real printer. | ☐ |
| 2.3 | Paper out shows the amber banner with Retry; selling continues | Automated: `print_pipeline.rs` (paper-out, hang, errors), `PrinterBanner.test.tsx`. **Manual — pending:** open the cover or run out of paper during a sale. | ☐ |
| 2.4 | The scanner adds items; an unknown barcode beeps, toasts and pre-fills Quick Sale; Enter never presses Pay | Automated: `scanner.race.test.ts` (5/15/29 ms bursts, 80 ms typing, Sinhala layout, Enter over Pay, F13 prefix), `PosScreen.test.tsx`. **Manual — pending:** configure the scanner with prefix F13 and suffix Enter, scan 20 items with the Sinhala keyboard layout active. | ☐ |

## 3. People: touch-only and keyboard-only, in English and Sinhala

| # | Check | Verified by | Result |
|---|---|---|---|
| 3.1 | Keyboard-only sale in English | E2E (the whole suite is keyboard-driven after login). | ✅ 2026-11-09 |
| 3.2 | Keyboard-only sale in Sinhala | Automated: i18n parity (`pnpm i18n:check`, 11 namespaces), shortcut routing is layout-independent (`shortcuts.test.ts`, `decode.ts`). **Manual — pending:** one sale with the UI in සිංහල. A native speaker should review the new till, printing, backup and staff wording. | ☐ |
| 3.3 | Touch-only sale (no keyboard) in English and Sinhala | Every till action has a 48 px button (64 px for PAY and the keypad); the keypad types into the focused field. **Manual — pending** on the touchscreen. | ☐ |

## 4. Staff and settings

| # | Check | Verified by | Result |
|---|---|---|---|
| 4.1 | The Owner adds a cashier and a supervisor who can log in with their PINs | E2E. Automated: `staff.rs` (8 tests: PIN rules, overrides, owner invariant, PIN reset lifts the lockout). | ✅ 2026-11-09 |
| 4.2 | The last Owner can't be disabled or demoted | Automated: `staff.rs`, `StaffScreen.test.tsx` (role and active switch locked with an explanation). | ✅ |
