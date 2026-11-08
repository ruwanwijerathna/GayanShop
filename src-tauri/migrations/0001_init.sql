-- GayanShop POS — initial schema (docs/ARCHITECTURE.md §4.3 mirrors this file; this file is authoritative).
-- Conventions: money = INTEGER minor units (LKR cents); ids = UUID TEXT; timestamps = ISO-8601 UTC TEXT;
-- business_date = 'YYYY-MM-DD' in store timezone after day cutoff, computed by the writing device.

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
  is_active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  deleted_at TEXT, version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX ix_staff_role ON staff(role_id);

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
  name TEXT NOT NULL, sku TEXT, barcode TEXT,
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

-- Standalone FTS keyed by id (NOT external-content on rowid: TEXT PK ⇒ VACUUM/restore may renumber rowids).
CREATE VIRTUAL TABLE products_fts USING fts5(id UNINDEXED, name, sku, barcode, tokenize = 'unicode61');
CREATE TRIGGER trg_products_fts_ins AFTER INSERT ON products BEGIN
  INSERT INTO products_fts(id, name, sku, barcode) VALUES (NEW.id, NEW.name, NEW.sku, NEW.barcode);
END;
CREATE TRIGGER trg_products_fts_upd AFTER UPDATE OF name, sku, barcode ON products BEGIN
  DELETE FROM products_fts WHERE id = OLD.id;
  INSERT INTO products_fts(id, name, sku, barcode) VALUES (NEW.id, NEW.name, NEW.sku, NEW.barcode);
END;
CREATE TRIGGER trg_products_fts_del AFTER DELETE ON products BEGIN
  DELETE FROM products_fts WHERE id = OLD.id;
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
-- Cost lock: unit_cost / qty_received may not change once the parent batch is received or closed.
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

-- ═════════ orders (sale | exchange) ═════════
CREATE TABLE orders (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  cash_session_id TEXT NOT NULL REFERENCES cash_sessions(id),
  receipt_no TEXT NOT NULL,
  type TEXT NOT NULL CHECK (type IN ('sale','exchange')),
  status TEXT NOT NULL CHECK (status IN ('completed','voided')),
  original_order_id TEXT REFERENCES orders(id),
  staff_id TEXT NOT NULL,
  approved_by TEXT,
  customer_name TEXT, customer_phone TEXT,
  gross_sales INTEGER NOT NULL,                      -- Σ line_gross of new (qty > 0) lines
  discount_total INTEGER NOT NULL DEFAULT 0,         -- Σ line_discount + bill discount
  returns_net INTEGER NOT NULL DEFAULT 0 CHECK (returns_net <= 0),
  net_sales INTEGER NOT NULL,                        -- Σ line_net of all lines (ex tax)
  tax_total INTEGER NOT NULL DEFAULT 0,
  rounding INTEGER NOT NULL DEFAULT 0,
  grand_total INTEGER NOT NULL,                      -- Σ line_total + rounding (exchange may be ≤ 0)
  paid_total INTEGER NOT NULL,
  change_due INTEGER NOT NULL DEFAULT 0 CHECK (change_due >= 0),
  cost_total INTEGER NOT NULL DEFAULT 0,
  quick_sale_net INTEGER NOT NULL DEFAULT 0,
  unknown_cost_net INTEGER NOT NULL DEFAULT 0,
  item_count INTEGER NOT NULL,
  business_date TEXT NOT NULL,
  completed_at TEXT NOT NULL,
  voided_at TEXT, voided_by TEXT, void_approved_by TEXT, void_reason TEXT,
  note TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, updated_by_device TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  CHECK (type = 'sale' OR original_order_id IS NOT NULL),   -- exchanges require the receipt
  CHECK (type = 'exchange' OR returns_net = 0)
);
CREATE UNIQUE INDEX ux_orders_receipt ON orders(store_id, receipt_no);
CREATE INDEX ix_orders_bdate ON orders(business_date, device_id);
CREATE INDEX ix_orders_session ON orders(cash_session_id);
CREATE INDEX ix_orders_original ON orders(original_order_id) WHERE original_order_id IS NOT NULL;
CREATE TRIGGER trg_orders_immutable BEFORE UPDATE ON orders
WHEN NOT (OLD.status = 'completed' AND NEW.status = 'voided'
          AND NEW.net_sales = OLD.net_sales AND NEW.grand_total = OLD.grand_total
          AND NEW.business_date = OLD.business_date AND NEW.device_id = OLD.device_id)
BEGIN SELECT RAISE(ABORT, 'orders are immutable except completed→voided'); END;
CREATE TRIGGER trg_orders_no_delete BEFORE DELETE ON orders
BEGIN SELECT RAISE(ABORT, 'orders cannot be deleted'); END;

CREATE TABLE order_items (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  order_id TEXT NOT NULL REFERENCES orders(id),
  line_no INTEGER NOT NULL,
  product_id TEXT REFERENCES products(id),
  is_custom INTEGER NOT NULL DEFAULT 0,
  custom_key TEXT,
  batch_id TEXT REFERENCES batches(id),
  name TEXT NOT NULL, barcode TEXT,
  list_price INTEGER NOT NULL, unit_price INTEGER NOT NULL CHECK (unit_price >= 0),
  qty INTEGER NOT NULL CHECK (qty <> 0),
  line_gross INTEGER NOT NULL,                       -- unit_price × qty
  line_discount INTEGER NOT NULL DEFAULT 0,
  order_discount_share INTEGER NOT NULL DEFAULT 0,
  tax_rate_bp INTEGER NOT NULL DEFAULT 0,
  tax_amount INTEGER NOT NULL DEFAULT 0,
  line_total INTEGER NOT NULL,                       -- what the customer pays (incl. tax)
  line_net INTEGER NOT NULL,                         -- ex tax
  cost_total INTEGER NOT NULL DEFAULT 0,
  cost_known INTEGER NOT NULL DEFAULT 1,
  price_overridden INTEGER NOT NULL DEFAULT 0, override_reason TEXT, approved_by TEXT,
  return_of_item_id TEXT REFERENCES order_items(id),
  return_disposition TEXT CHECK (return_disposition IN ('restock','damaged','none')),
  created_at TEXT NOT NULL,
  CHECK ((is_custom = 1 AND product_id IS NULL AND custom_key IS NOT NULL)
      OR (is_custom = 0 AND product_id IS NOT NULL)),
  CHECK (line_gross = unit_price * qty),
  CHECK (line_total = line_gross - line_discount - order_discount_share),
  -- return lines (qty < 0) must reference the receipt line and carry a disposition; new lines must not
  CHECK ((qty > 0 AND return_of_item_id IS NULL AND return_disposition IS NULL)
      OR (qty < 0 AND return_of_item_id IS NOT NULL AND return_disposition IS NOT NULL)),
  UNIQUE (order_id, line_no)
);
CREATE INDEX ix_items_product ON order_items(product_id);
CREATE INDEX ix_items_custom ON order_items(custom_key) WHERE is_custom = 1;
CREATE INDEX ix_items_batch ON order_items(batch_id) WHERE batch_id IS NOT NULL;
CREATE INDEX ix_items_return_of ON order_items(return_of_item_id) WHERE return_of_item_id IS NOT NULL;
CREATE TRIGGER trg_items_no_update BEFORE UPDATE ON order_items
BEGIN SELECT RAISE(ABORT, 'order_items are immutable'); END;
CREATE TRIGGER trg_items_no_delete BEFORE DELETE ON order_items
BEGIN SELECT RAISE(ABORT, 'order_items are immutable'); END;

-- ═════════ credit notes (exchange balance owed to the customer — never cash) ═════════
CREATE TABLE credit_notes (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  code TEXT NOT NULL,                                -- CN-T01-20261108-0003
  issued_order_id TEXT NOT NULL REFERENCES orders(id),
  amount INTEGER NOT NULL CHECK (amount > 0),
  business_date TEXT NOT NULL,
  expires_on TEXT,
  staff_id TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_credit_notes_code ON credit_notes(store_id, code);
CREATE TRIGGER trg_cn_no_update BEFORE UPDATE ON credit_notes
BEGIN SELECT RAISE(ABORT, 'credit_notes are append-only'); END;
CREATE TRIGGER trg_cn_no_delete BEFORE DELETE ON credit_notes
BEGIN SELECT RAISE(ABORT, 'credit_notes are append-only'); END;

CREATE TABLE order_payments (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  order_id TEXT NOT NULL REFERENCES orders(id),
  method TEXT NOT NULL CHECK (method IN ('cash','card','bank_transfer','qr','other','credit_note')),
  amount INTEGER NOT NULL,
  tendered INTEGER,
  credit_note_id TEXT REFERENCES credit_notes(id),
  reference TEXT,
  created_at TEXT NOT NULL,
  -- NO CASH REFUNDS: only the credit-note issue row may be negative.
  CHECK (amount >= 0 OR method = 'credit_note'),
  CHECK ((method = 'credit_note') = (credit_note_id IS NOT NULL)),
  CHECK (tendered IS NULL OR (method = 'cash' AND tendered >= amount))
);
CREATE INDEX ix_payments_order ON order_payments(order_id);
CREATE INDEX ix_payments_credit_note ON order_payments(credit_note_id) WHERE credit_note_id IS NOT NULL;
CREATE TRIGGER trg_payments_no_update BEFORE UPDATE ON order_payments
BEGIN SELECT RAISE(ABORT, 'order_payments are append-only'); END;
CREATE TRIGGER trg_payments_no_delete BEFORE DELETE ON order_payments
BEGIN SELECT RAISE(ABORT, 'order_payments are append-only'); END;

-- Balance = −Σ payments on completed orders (issue row is negative, redemptions positive).
CREATE VIEW v_credit_note_balance AS
SELECT cn.id, cn.code, cn.amount AS issued, cn.expires_on,
       -COALESCE(SUM(CASE WHEN o.status = 'completed' THEN p.amount ELSE 0 END), 0) AS balance
FROM credit_notes cn
LEFT JOIN order_payments p ON p.credit_note_id = cn.id
LEFT JOIN orders o ON o.id = p.order_id
GROUP BY cn.id;

-- ═════════ stock ledger (append-only) ═════════
CREATE TABLE stock_movements (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  product_id TEXT NOT NULL REFERENCES products(id),
  batch_item_id TEXT NOT NULL REFERENCES batch_items(id),
  qty_delta INTEGER NOT NULL CHECK (qty_delta <> 0),
  reason TEXT NOT NULL CHECK (reason IN ('batch_intake','batch_reopen','sale','return','void',
                                         'damage','count_correction','adjustment')),
  ref_type TEXT, ref_id TEXT,
  order_item_id TEXT REFERENCES order_items(id),
  unit_cost INTEGER NOT NULL,
  revenue_net INTEGER,
  business_date TEXT NOT NULL,
  staff_id TEXT, approved_by TEXT, note TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX ix_mov_product ON stock_movements(product_id);
CREATE INDEX ix_mov_layer ON stock_movements(batch_item_id);
CREATE INDEX ix_mov_date_reason ON stock_movements(business_date, reason);
CREATE INDEX ix_mov_order_item ON stock_movements(order_item_id) WHERE order_item_id IS NOT NULL;
CREATE TRIGGER trg_mov_no_update BEFORE UPDATE ON stock_movements
BEGIN SELECT RAISE(ABORT, 'stock_movements are append-only'); END;
CREATE TRIGGER trg_mov_no_delete BEFORE DELETE ON stock_movements
BEGIN SELECT RAISE(ABORT, 'stock_movements are append-only'); END;

-- Derived caches (rebuildable; not synced)
CREATE TABLE stock_levels (product_id TEXT PRIMARY KEY, on_hand INTEGER NOT NULL DEFAULT 0);
CREATE TABLE batch_item_stock (
  batch_item_id TEXT PRIMARY KEY, batch_id TEXT NOT NULL, product_id TEXT NOT NULL,
  received_qty INTEGER NOT NULL DEFAULT 0, on_hand INTEGER NOT NULL DEFAULT 0,
  sold_qty INTEGER NOT NULL DEFAULT 0, sold_revenue INTEGER NOT NULL DEFAULT 0, sold_cost INTEGER NOT NULL DEFAULT 0,
  written_off_qty INTEGER NOT NULL DEFAULT 0, written_off_cost INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX ix_bis_batch ON batch_item_stock(batch_id);

-- Sign convention: a sale slice has qty_delta −2 and revenue_net +900 ⇒ sold_qty +2, sold_revenue +900.
-- return/void slices mirror it (+qty, −revenue).
CREATE TRIGGER trg_mov_derive AFTER INSERT ON stock_movements BEGIN
  INSERT INTO stock_levels(product_id, on_hand) VALUES (NEW.product_id, NEW.qty_delta)
  ON CONFLICT(product_id) DO UPDATE SET on_hand = on_hand + excluded.on_hand;

  INSERT INTO batch_item_stock(batch_item_id, batch_id, product_id)
  SELECT NEW.batch_item_id, bi.batch_id, NEW.product_id FROM batch_items bi WHERE bi.id = NEW.batch_item_id
  ON CONFLICT(batch_item_id) DO NOTHING;

  UPDATE batch_item_stock SET
    on_hand          = on_hand + NEW.qty_delta,
    received_qty     = received_qty     + CASE WHEN NEW.reason IN ('batch_intake','batch_reopen') THEN NEW.qty_delta ELSE 0 END,
    sold_qty         = sold_qty         + CASE WHEN NEW.reason IN ('sale','return','void') THEN -NEW.qty_delta ELSE 0 END,
    sold_revenue     = sold_revenue     + CASE WHEN NEW.reason IN ('sale','return','void') THEN COALESCE(NEW.revenue_net, 0) ELSE 0 END,
    sold_cost        = sold_cost        + CASE WHEN NEW.reason IN ('sale','return','void') THEN -NEW.qty_delta * NEW.unit_cost ELSE 0 END,
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

CREATE VIEW v_quick_sale_summary AS
SELECT oi.custom_key,
       (SELECT x.name FROM order_items x WHERE x.custom_key = oi.custom_key AND x.is_custom = 1
        GROUP BY x.name ORDER BY COUNT(*) DESC LIMIT 1)                AS display_name,
       COUNT(*)                                                         AS times_sold,
       SUM(oi.qty)                                                      AS units_sold,
       SUM(oi.line_net)                                                 AS revenue_net,
       (SELECT y.unit_price FROM order_items y WHERE y.custom_key = oi.custom_key AND y.is_custom = 1
        ORDER BY y.created_at DESC LIMIT 1)                             AS last_price,
       MIN(oi.unit_price) AS min_price, MAX(oi.unit_price) AS max_price,
       MIN(oi.created_at) AS first_sold_at, MAX(oi.created_at) AS last_sold_at,
       COUNT(DISTINCT oi.device_id)                                     AS device_count,
       COALESCE(l.action, 'new')                                        AS status,
       l.product_id
FROM order_items oi
JOIN orders o ON o.id = oi.order_id AND o.status = 'completed'
LEFT JOIN quick_sale_links l ON l.custom_key = oi.custom_key AND l.deleted_at IS NULL
WHERE oi.is_custom = 1 AND oi.qty > 0
GROUP BY oi.custom_key;

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
CREATE INDEX ix_audit_created ON audit_logs(created_at);
CREATE INDEX ix_audit_entity ON audit_logs(entity_type, entity_id);
CREATE INDEX ix_audit_action ON audit_logs(action, business_date);
CREATE TRIGGER trg_audit_no_update BEFORE UPDATE ON audit_logs
BEGIN SELECT RAISE(ABORT, 'audit_logs are append-only'); END;
CREATE TRIGGER trg_audit_no_delete BEFORE DELETE ON audit_logs
BEGIN SELECT RAISE(ABORT, 'audit_logs are append-only'); END;

-- ═════════ reporting rollups (derived, per device per business day) ═════════
CREATE TABLE daily_sales_summary (
  business_date TEXT NOT NULL, device_id TEXT NOT NULL,
  sales_count INTEGER NOT NULL DEFAULT 0, exchange_count INTEGER NOT NULL DEFAULT 0, void_count INTEGER NOT NULL DEFAULT 0,
  gross_sales INTEGER NOT NULL DEFAULT 0, discounts INTEGER NOT NULL DEFAULT 0,
  sales_net INTEGER NOT NULL DEFAULT 0, returns_net INTEGER NOT NULL DEFAULT 0,
  tax_total INTEGER NOT NULL DEFAULT 0, cogs INTEGER NOT NULL DEFAULT 0,
  quick_sale_net INTEGER NOT NULL DEFAULT 0, unknown_cost_net INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (business_date, device_id)
);
-- method key: real method, or 'credit_note_issued' (negative rows) / 'credit_note_redeemed' (positive rows)
CREATE TABLE daily_payment_summary (
  business_date TEXT NOT NULL, device_id TEXT NOT NULL, method TEXT NOT NULL,
  amount INTEGER NOT NULL DEFAULT 0, txn_count INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (business_date, device_id, method)
);

CREATE TRIGGER trg_rollup_order_insert AFTER INSERT ON orders WHEN NEW.status = 'completed' BEGIN
  INSERT INTO daily_sales_summary(business_date, device_id) VALUES (NEW.business_date, NEW.device_id)
  ON CONFLICT DO NOTHING;
  UPDATE daily_sales_summary SET
    sales_count      = sales_count    + (NEW.type = 'sale'),
    exchange_count   = exchange_count + (NEW.type = 'exchange'),
    gross_sales      = gross_sales    + NEW.gross_sales,
    discounts        = discounts      + NEW.discount_total,
    sales_net        = sales_net      + (NEW.net_sales - NEW.returns_net),
    returns_net      = returns_net    + NEW.returns_net,
    tax_total        = tax_total      + NEW.tax_total,
    cogs             = cogs           + NEW.cost_total,
    quick_sale_net   = quick_sale_net   + NEW.quick_sale_net,
    unknown_cost_net = unknown_cost_net + NEW.unknown_cost_net
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
END;
-- A pulled order that arrives already voided is never counted, except in void_count.
CREATE TRIGGER trg_rollup_order_insert_voided AFTER INSERT ON orders WHEN NEW.status = 'voided' BEGIN
  INSERT INTO daily_sales_summary(business_date, device_id) VALUES (NEW.business_date, NEW.device_id)
  ON CONFLICT DO NOTHING;
  UPDATE daily_sales_summary SET void_count = void_count + 1
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
END;
CREATE TRIGGER trg_rollup_order_void AFTER UPDATE OF status ON orders
WHEN OLD.status = 'completed' AND NEW.status = 'voided' BEGIN
  UPDATE daily_sales_summary SET
    sales_count      = sales_count    - (NEW.type = 'sale'),
    exchange_count   = exchange_count - (NEW.type = 'exchange'),
    void_count       = void_count + 1,
    gross_sales      = gross_sales - NEW.gross_sales,
    discounts        = discounts   - NEW.discount_total,
    sales_net        = sales_net   - (NEW.net_sales - NEW.returns_net),
    returns_net      = returns_net - NEW.returns_net,
    tax_total        = tax_total   - NEW.tax_total,
    cogs             = cogs        - NEW.cost_total,
    quick_sale_net   = quick_sale_net   - NEW.quick_sale_net,
    unknown_cost_net = unknown_cost_net - NEW.unknown_cost_net
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
  UPDATE daily_payment_summary SET
    amount = amount - (SELECT COALESCE(SUM(p.amount), 0) FROM order_payments p
                       WHERE p.order_id = NEW.id
                         AND (CASE WHEN p.method <> 'credit_note' THEN p.method
                                   WHEN p.amount < 0 THEN 'credit_note_issued'
                                   ELSE 'credit_note_redeemed' END) = daily_payment_summary.method),
    txn_count = txn_count - (SELECT COUNT(*) FROM order_payments p
                             WHERE p.order_id = NEW.id
                               AND (CASE WHEN p.method <> 'credit_note' THEN p.method
                                         WHEN p.amount < 0 THEN 'credit_note_issued'
                                         ELSE 'credit_note_redeemed' END) = daily_payment_summary.method)
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
END;
CREATE TRIGGER trg_rollup_payment AFTER INSERT ON order_payments
WHEN (SELECT status FROM orders WHERE id = NEW.order_id) = 'completed' BEGIN
  INSERT INTO daily_payment_summary(business_date, device_id, method, amount, txn_count)
  SELECT o.business_date, o.device_id,
         CASE WHEN NEW.method <> 'credit_note' THEN NEW.method
              WHEN NEW.amount < 0 THEN 'credit_note_issued'
              ELSE 'credit_note_redeemed' END,
         NEW.amount, 1
  FROM orders o WHERE o.id = NEW.order_id
  ON CONFLICT(business_date, device_id, method) DO UPDATE SET
    amount = amount + excluded.amount, txn_count = txn_count + 1;
END;
-- Voids are same-session only (§3.3), so a void never rewrites a closed Z-report.

-- ═════════ local-only plumbing ═════════
CREATE TABLE receipt_counters (day TEXT PRIMARY KEY, last_seq INTEGER NOT NULL);
CREATE TABLE credit_note_counters (day TEXT PRIMARY KEY, last_seq INTEGER NOT NULL);
CREATE TABLE held_carts (
  id TEXT PRIMARY KEY, label TEXT, payload TEXT NOT NULL,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);

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
