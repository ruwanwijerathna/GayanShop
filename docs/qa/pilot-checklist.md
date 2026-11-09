# Pilot readiness and rollout checklist

> Document reference: `docs/ARCHITECTURE.md` §8 Phase 4 Exit & Pilot criteria.
> Target environment: GayanShop POS (LKR currency, `Asia/Colombo` timezone, 04:00 AM cutoff, Monday week start, 1 Store, 2–3 Counters).

---

## 1. Hardware & Environment Verification

| # | Check Item | Requirement / Configuration | Verified By / Notes | Status |
|---|------------|-----------------------------|---------------------|:------:|
| 1.1 | **Operating System** | Windows 10 (64-bit) or Windows 11 with latest updates | System Info / Settings | [ ] |
| 1.2 | **WebView2 Runtime** | Bundled or downloaded via Evergreen bootstrapper | Handled by NSIS installer | [ ] |
| 1.3 | **Display Resolution** | Minimum 1024×768 (Recommended 1366×768 or 1080p touch display) | Display Settings | [ ] |
| 1.4 | **Receipt Printer** | Generic ESC/POS 80mm thermal receipt printer (USB / Virtual COM) | RAW text mode / `print_test_page` | [ ] |
| 1.5 | **Cash Drawer** | RJ11 drawer connected to printer kick port (`ESC p 0 25 250`) | Tested on cash sale / `no_sale` | [ ] |
| 1.6 | **Barcode Scanner** | USB HID keyboard-wedge scanner (physical keycode translation) | Scanned in Sinhala/English layout | [ ] |
| 1.7 | **Label Printer** | 203 DPI TSPL/ZPL barcode label printer (2-up 2″×1″ or 3-up 1″×1″) | Calibrated in Settings → Labels | [ ] |
| 1.8 | **Power & Storage** | Internal SSD with ≥ 1 GB free space; compliant power protection / UPS | `%APPDATA%\com.gayanshop.pos` | [ ] |

---

## 2. Store Setup & Counter Registration

| # | Step | Action | Expected Outcome | Status |
|---|------|--------|------------------|:------:|
| 2.1 | **Counter T01 Initialization** | Run First-Run Wizard on Counter 1; create store name, currency (LKR), Owner PIN (6+ digits) | Store created; local `pos.db` initialized with 48 tables and 33 triggers; initial cash session available | [ ] |
| 2.2 | **Cloud Connection** | In Settings → Sync, link Supabase project and sign in with Owner account | Device registered as T01; sync status pill shows "Connected / Synced" | [ ] |
| 2.3 | **Catalog & Staff Seed** | Add categories, standard products, batches, and staff (Cashiers, Supervisors, Managers) | All products searchable via Sinhala FTS (trigram) and SKU/barcodes; staff PINs hashed with argon2id | [ ] |
| 2.4 | **Counters T02 & T03 Join** | Install GayanShop POS on Counter 2 & Counter 3; select "Join existing store", sign in with Owner credentials | Counters receive codes T02 and T03; full catalog, staff, and settings bootstrapped with progress bar | [ ] |

---

## 3. Daily Operational Workflows

### 3.1 Morning Opening
- [ ] Open cash session on each active till.
- [ ] Enter opening float amount in LKR cents.
- [ ] Drawer opens automatically on session start.

### 3.2 Trading & Checkout
- [ ] Scan standard barcode items (sub-100 ms scan-to-cart).
- [ ] Add quick items via `F2` -> type price -> `Enter` (sub-50 ms).
- [ ] Customer phone lookup via `F4` (phone masked on receipt: `07X XXX X123`).
- [ ] Line and bill discounts (cashier limited to max 5%, supervisor override for higher).
- [ ] Payment in cash (`F9`) with change calculation, or Card / Bank transfer.
- [ ] Receipt prints within 1.5 seconds; drawer kicks for cash transactions.

### 3.3 Void & Correction Rules
- [ ] All sales final (no cash refunds).
- [ ] Void restricted strictly to the same open cash session.
- [ ] Cashier void requires supervisor PIN approval; approval dialog records single-use authorization.
- [ ] Voided sale slices immediately reverse in stock ledger; zero net quantity variance.

### 3.4 Midday Drawer Pay-outs
- [ ] Record petty cash expense paid from drawer (`EXPENSE_MANAGE` + `CASH_PAY_IN_OUT`).
- [ ] Drawer kicks; cash session running balance reflects payout.

### 3.5 End-of-Day Closing
- [ ] Count cash drawer by denomination.
- [ ] Close cash session; view generated Z-report summary.
- [ ] Discrepancies (overage/shortage) logged and audited.

---

## 4. Resilience & Disaster Recovery Verification

| Test Scenario | Verification Procedure | Pass Criteria | Status |
|---------------|------------------------|---------------|:------:|
| **Internet Outage** | Disconnect WAN / router during trading | Counters continue selling normally offline; outbox queues operations; zero UI blocking | [ ] |
| **Cloud Reconnection** | Reconnect internet after offline sales | Outbox pushes queued operations; incoming changes pulled; daily totals converge across all counters | [ ] |
| **Power Interruption** | Simulate abrupt power loss during operation | SQLite WAL `synchronous = FULL` guarantees zero data loss; restart verifies quick_check and unprinted receipts | [ ] |
| **Audit Integrity** | Run audit chain verification (`audit::verify`) | Gapless sequential device hashes for all counters; zero breaks detected | [ ] |
| **Automatic Backups** | Verify hourly rotation in `backups/` directory | Backups created hourly, rotated daily; zero sale disruption | [ ] |
| **Manual USB Backup** | Settings → Backup → "Backup Database Now" to USB drive | Snapshot created via `VACUUM INTO` in under 30 s for 500 MB DB | [ ] |
| **Full Restore Drill** | Execute restore drill using pre-restore backup | Online or offline recovery path completes; derived tables rebuilt; audit row logged | [ ] |
| **Support Bundle** | Export Support Bundle zip from Settings | Archive contains `diagnostics.json`, rotating `logs/*.log`, sync logs, and integrity status | [ ] |

---

## 5. Pilot Sign-Off Gates

- [ ] **Two-Week Continuous Trading:** 2–3 counters operating in production with 0 data loss incidents.
- [ ] **Performance SLA Met:**
  - Barcode scan to cart < 100 ms
  - Quick sale < 50 ms
  - Phone lookup < 30 ms
  - Checkout to receipt < 1.5 s
  - Search < 50 ms (up to 50,000 products)
  - Daily/Yearly reports < 300 ms
- [ ] **Restore Drill Passed:** Successful database recovery on a counter terminal.
- [ ] **Signed Update Verified:** Next minor release deployed and verified via `tauri-plugin-updater`.

