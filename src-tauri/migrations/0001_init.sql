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
