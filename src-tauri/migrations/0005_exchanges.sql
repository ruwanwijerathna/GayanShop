-- GayanShop POS — schema v4.5: item-for-item EXCHANGES (decision #26).
-- Policy: goods can be exchanged within 7 days of the sale's business date (a supervisor/owner PIN lifts the
-- window); strictly item-for-item: never a cash refund and never a credit note. Returned goods go straight back
-- into saleable stock, into the cost layers they were sold from.
--
-- Model. An exchange is a normal sale of the REPLACEMENT lines (`orders.exchange_credit > 0`) plus insert-only
-- `order_returns` rows naming the returned lines of earlier orders. The returned goods are credited at what the
-- customer actually paid for them (prorated per unit with cumulative flooring); the customer pays only
-- `grand_total − exchange_credit` through ordinary payments. `paid_total = grand_total` still holds (credit
-- counts as paid). Returned goods come back as `return` stock slices that mirror the original sale slices, so
-- `batch_item_stock` and every batch/product margin net out automatically. The `returned_*` header columns
-- carry the returned amounts so the daily rollups (sales net of returns) stay a pure function of order rows.

ALTER TABLE orders ADD COLUMN exchange_credit INTEGER NOT NULL DEFAULT 0 CHECK (exchange_credit >= 0);
ALTER TABLE orders ADD COLUMN returned_items INTEGER NOT NULL DEFAULT 0 CHECK (returned_items >= 0);
ALTER TABLE orders ADD COLUMN returned_gross INTEGER NOT NULL DEFAULT 0 CHECK (returned_gross >= 0);
ALTER TABLE orders ADD COLUMN returned_discount INTEGER NOT NULL DEFAULT 0 CHECK (returned_discount >= 0);
ALTER TABLE orders ADD COLUMN returned_net INTEGER NOT NULL DEFAULT 0 CHECK (returned_net >= 0);
ALTER TABLE orders ADD COLUMN returned_tax INTEGER NOT NULL DEFAULT 0 CHECK (returned_tax >= 0);
ALTER TABLE orders ADD COLUMN returned_cost INTEGER NOT NULL DEFAULT 0 CHECK (returned_cost >= 0);
ALTER TABLE orders ADD COLUMN late_approved_by TEXT REFERENCES staff(id);   -- EXCHANGE_PROCESS / EXCHANGE_LATE approver
CREATE INDEX ix_orders_exchange ON orders(business_date) WHERE exchange_credit > 0;

CREATE TABLE order_returns (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  order_id TEXT NOT NULL REFERENCES orders(id),                 -- the exchange (replacement) order
  source_item_id TEXT NOT NULL REFERENCES order_items(id),      -- the returned line of an earlier order
  qty INTEGER NOT NULL CHECK (qty > 0),
  line_gross INTEGER NOT NULL CHECK (line_gross >= 0),
  line_discount INTEGER NOT NULL CHECK (line_discount >= 0),    -- line + bill discount share = gross − total
  line_total INTEGER NOT NULL CHECK (line_total >= 0),          -- credited: what the customer paid for these units
  line_net INTEGER NOT NULL CHECK (line_net >= 0),              -- ex tax
  tax_amount INTEGER NOT NULL CHECK (tax_amount >= 0),
  cost_total INTEGER NOT NULL CHECK (cost_total >= 0),          -- Σ of the restocked slices' cost
  created_at TEXT NOT NULL,
  UNIQUE (order_id, source_item_id)
);
CREATE INDEX ix_returns_source ON order_returns(source_item_id);
CREATE TRIGGER trg_returns_no_update BEFORE UPDATE ON order_returns
BEGIN SELECT RAISE(ABORT, 'order_returns are immutable'); END;
CREATE TRIGGER trg_returns_no_delete BEFORE DELETE ON order_returns
BEGIN SELECT RAISE(ABORT, 'order_returns are immutable'); END;
-- POLICY: returns can only be added to an exchange order that is still pending (inside its own checkout).
CREATE TRIGGER trg_returns_only_into_pending BEFORE INSERT ON order_returns
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0
 AND (SELECT status FROM orders WHERE id = NEW.order_id) IS NOT 'pending'
BEGIN SELECT RAISE(ABORT, 'order_returns: cannot add returns to a sealed order'); END;

-- ═════════ stock_movements: add the 'return' reason (table rebuild: SQLite cannot alter a CHECK) ═════════
-- Everything that reads the table is dropped first and re-created below, so the rename never meets a dangling reference.
DROP VIEW v_integrity_void_unreversed;
DROP TRIGGER trg_orders_seal_verify;
CREATE TABLE stock_movements_v5 (
  id TEXT PRIMARY KEY, store_id TEXT NOT NULL, device_id TEXT NOT NULL,
  product_id TEXT NOT NULL REFERENCES products(id),
  batch_item_id TEXT NOT NULL REFERENCES batch_items(id),
  qty_delta INTEGER NOT NULL CHECK (qty_delta <> 0),
  reason TEXT NOT NULL CHECK (reason IN ('batch_intake','batch_reopen','sale','void','return',
                                         'damage','count_correction','adjustment')),
  ref_type TEXT, ref_id TEXT,
  order_item_id TEXT REFERENCES order_items(id),
  unit_cost INTEGER NOT NULL CHECK (unit_cost >= 0),
  revenue_net INTEGER,
  business_date TEXT NOT NULL,
  staff_id TEXT, approved_by TEXT, note TEXT,
  created_at TEXT NOT NULL,
  CHECK ((reason IN ('sale','void','return')) = (order_item_id IS NOT NULL)),
  CHECK (reason <> 'sale' OR (qty_delta < 0 AND revenue_net >= 0)),
  CHECK (reason <> 'void' OR (qty_delta > 0 AND revenue_net <= 0)),
  CHECK (reason <> 'return' OR (qty_delta > 0 AND revenue_net <= 0 AND ref_id IS NOT NULL))
);
INSERT INTO stock_movements_v5 SELECT id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, ref_type,
  ref_id, order_item_id, unit_cost, revenue_net, business_date, staff_id, approved_by, note, created_at FROM stock_movements;
DROP TABLE stock_movements;
ALTER TABLE stock_movements_v5 RENAME TO stock_movements;
CREATE INDEX ix_mov_product ON stock_movements(product_id);
CREATE INDEX ix_mov_layer ON stock_movements(batch_item_id);
CREATE INDEX ix_mov_date_reason ON stock_movements(business_date, reason);
CREATE INDEX ix_mov_order_item ON stock_movements(order_item_id) WHERE order_item_id IS NOT NULL;
CREATE INDEX ix_mov_writeoffs ON stock_movements(business_date)
  WHERE reason IN ('damage','count_correction','adjustment');
CREATE TRIGGER trg_mov_no_update BEFORE UPDATE ON stock_movements
BEGIN SELECT RAISE(ABORT, 'stock_movements are append-only'); END;
CREATE TRIGGER trg_mov_no_delete BEFORE DELETE ON stock_movements
BEGIN SELECT RAISE(ABORT, 'stock_movements are append-only'); END;
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

-- POLICY: return slices only inside their pending exchange, only for a returned-lines row, and each must mirror
-- an existing sale slice (same line, layer and cost) without returning more than was sold.
CREATE TRIGGER trg_mov_return_guard BEFORE INSERT ON stock_movements
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0 AND NEW.reason = 'return' BEGIN
  SELECT RAISE(ABORT, 'stock: return slices only inside a pending exchange')
  WHERE NOT EXISTS (SELECT 1 FROM orders o WHERE o.id = NEW.ref_id AND o.status = 'pending' AND o.exchange_credit > 0);
  SELECT RAISE(ABORT, 'stock: a return slice needs its order_returns row')
  WHERE NOT EXISTS (SELECT 1 FROM order_returns r WHERE r.order_id = NEW.ref_id AND r.source_item_id = NEW.order_item_id);
  SELECT RAISE(ABORT, 'stock: return slice must mirror a sale slice and not exceed what was sold')
  WHERE NOT EXISTS (SELECT 1 FROM stock_movements m WHERE m.order_item_id = NEW.order_item_id
                      AND m.batch_item_id = NEW.batch_item_id AND m.unit_cost = NEW.unit_cost AND m.reason = 'sale')
     OR COALESCE((SELECT SUM(qty_delta) FROM stock_movements m WHERE m.order_item_id = NEW.order_item_id
                      AND m.batch_item_id = NEW.batch_item_id AND m.reason IN ('sale','void','return')), 0) + NEW.qty_delta > 0;
END;
CREATE TRIGGER trg_mov_derive AFTER INSERT ON stock_movements BEGIN
  INSERT INTO stock_levels(product_id, on_hand) VALUES (NEW.product_id, NEW.qty_delta)
  ON CONFLICT(product_id) DO UPDATE SET on_hand = on_hand + excluded.on_hand;

  INSERT INTO batch_item_stock(batch_item_id, batch_id, product_id)
  SELECT NEW.batch_item_id, bi.batch_id, NEW.product_id FROM batch_items bi WHERE bi.id = NEW.batch_item_id
  ON CONFLICT(batch_item_id) DO NOTHING;

  UPDATE batch_item_stock SET
    on_hand          = on_hand + NEW.qty_delta,
    received_qty     = received_qty     + CASE WHEN NEW.reason IN ('batch_intake','batch_reopen') THEN NEW.qty_delta ELSE 0 END,
    sold_qty         = sold_qty         + CASE WHEN NEW.reason IN ('sale','void','return') THEN -NEW.qty_delta ELSE 0 END,
    sold_revenue     = sold_revenue     + CASE WHEN NEW.reason IN ('sale','void','return') THEN COALESCE(NEW.revenue_net, 0) ELSE 0 END,
    sold_cost        = sold_cost        + CASE WHEN NEW.reason IN ('sale','void','return') THEN -NEW.qty_delta * NEW.unit_cost ELSE 0 END,
    written_off_qty  = written_off_qty  + CASE WHEN NEW.reason IN ('damage','count_correction','adjustment') THEN -NEW.qty_delta ELSE 0 END,
    written_off_cost = written_off_cost + CASE WHEN NEW.reason IN ('damage','count_correction','adjustment') THEN -NEW.qty_delta * NEW.unit_cost ELSE 0 END
  WHERE batch_item_id = NEW.batch_item_id;
END;

CREATE VIEW v_integrity_void_unreversed AS           -- voided orders whose sale slices aren't fully reversed
SELECT o.id AS order_id, m.order_item_id, m.batch_item_id, SUM(m.qty_delta) AS net_qty
FROM orders o JOIN order_items oi ON oi.order_id = o.id
JOIN stock_movements m ON m.order_item_id = oi.id AND m.reason IN ('sale','void')
WHERE o.status = 'voided'
GROUP BY o.id, m.order_item_id, m.batch_item_id
HAVING SUM(m.qty_delta) <> 0;
CREATE VIEW v_integrity_over_returned AS             -- a line returned more often than it was sold (a Rust bug)
SELECT oi.id AS order_item_id, oi.qty AS sold_qty, SUM(r.qty) AS returned_qty
FROM order_items oi JOIN order_returns r ON r.source_item_id = oi.id
GROUP BY oi.id HAVING SUM(r.qty) > oi.qty;

-- ═════════ order policy and rollups, re-created with the exchange columns ═════════
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
  SELECT RAISE(ABORT, 'seal: payments plus exchange credit must equal grand_total exactly; change only from cash tendered')
  WHERE NEW.grand_total <> (SELECT COALESCE(SUM(amount), 0) FROM order_payments WHERE order_id = NEW.id) + NEW.exchange_credit
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

  -- EXCHANGE (decision #26): item-for-item only. Goods come back at what the customer paid, the customer
  -- pays only the difference, nobody is ever paid out, and a late exchange needs EXCHANGE_LATE.
  SELECT RAISE(ABORT, 'seal: exchange credit and returned totals must equal the returned lines')
  WHERE NEW.exchange_credit  <> (SELECT COALESCE(SUM(line_total), 0)   FROM order_returns WHERE order_id = NEW.id)
     OR NEW.returned_items   <> (SELECT COALESCE(SUM(qty), 0)          FROM order_returns WHERE order_id = NEW.id)
     OR NEW.returned_gross   <> (SELECT COALESCE(SUM(line_gross), 0)   FROM order_returns WHERE order_id = NEW.id)
     OR NEW.returned_discount <> (SELECT COALESCE(SUM(line_discount), 0) FROM order_returns WHERE order_id = NEW.id)
     OR NEW.returned_net     <> (SELECT COALESCE(SUM(line_net), 0)     FROM order_returns WHERE order_id = NEW.id)
     OR NEW.returned_tax     <> (SELECT COALESCE(SUM(tax_amount), 0)   FROM order_returns WHERE order_id = NEW.id)
     OR NEW.returned_cost    <> (SELECT COALESCE(SUM(cost_total), 0)   FROM order_returns WHERE order_id = NEW.id);
  SELECT RAISE(ABORT, 'seal: exchange only, no refund: the replacement must be worth at least the returned goods')
  WHERE NEW.exchange_credit > NEW.grand_total;
  SELECT RAISE(ABORT, 'seal: only stock-tracked catalog lines of another completed order can be returned')
  WHERE EXISTS (
    SELECT 1 FROM order_returns r JOIN order_items oi ON oi.id = r.source_item_id JOIN orders so ON so.id = oi.order_id
    WHERE r.order_id = NEW.id
      AND (so.status <> 'completed' OR so.id = NEW.id OR oi.is_custom = 1
           OR COALESCE((SELECT track_stock FROM products p WHERE p.id = oi.product_id), 0) = 0));
  SELECT RAISE(ABORT, 'seal: returned lines must be valued at what the customer paid, prorated by unit, within the quantity sold')
  WHERE EXISTS (
    SELECT 1 FROM order_returns r JOIN order_items oi ON oi.id = r.source_item_id
    WHERE r.order_id = NEW.id
      AND (r.qty + (SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id) > oi.qty
           OR r.line_gross <> (oi.line_gross * ((SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id) + r.qty)) / oi.qty - (oi.line_gross * (SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id)) / oi.qty OR r.line_total <> (oi.line_total * ((SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id) + r.qty)) / oi.qty - (oi.line_total * (SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id)) / oi.qty
           OR r.line_net <> (oi.line_net * ((SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id) + r.qty)) / oi.qty - (oi.line_net * (SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id)) / oi.qty OR r.tax_amount <> (oi.tax_amount * ((SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id) + r.qty)) / oi.qty - (oi.tax_amount * (SELECT COALESCE(SUM(x.qty), 0) FROM order_returns x WHERE x.source_item_id = r.source_item_id AND x.order_id <> r.order_id)) / oi.qty
           OR r.line_discount <> r.line_gross - r.line_total));
  SELECT RAISE(ABORT, 'seal: return slices must restock exactly the returned qty, revenue and cost into the original layers')
  WHERE EXISTS (
    SELECT 1 FROM order_returns r WHERE r.order_id = NEW.id
      AND (r.qty        <>  (SELECT COALESCE(SUM(qty_delta), 0)              FROM stock_movements m WHERE m.order_item_id = r.source_item_id AND m.reason = 'return' AND m.ref_id = r.order_id)
        OR r.line_net   <> -(SELECT COALESCE(SUM(revenue_net), 0)            FROM stock_movements m WHERE m.order_item_id = r.source_item_id AND m.reason = 'return' AND m.ref_id = r.order_id)
        OR r.cost_total <>  (SELECT COALESCE(SUM(qty_delta * unit_cost), 0)  FROM stock_movements m WHERE m.order_item_id = r.source_item_id AND m.reason = 'return' AND m.ref_id = r.order_id)));
  SELECT RAISE(ABORT, 'seal: an exchange needs EXCHANGE_PROCESS or an approver who holds it')
  WHERE NEW.exchange_credit > 0
    AND COALESCE((SELECT perms & 64 FROM v_staff_effective WHERE staff_id = NEW.staff_id AND is_active = 1), 0) = 0
    AND COALESCE((SELECT perms & 64 FROM v_staff_effective WHERE staff_id = NEW.late_approved_by AND is_active = 1), 0) = 0;
  SELECT RAISE(ABORT, 'seal: an exchange after the window needs EXCHANGE_LATE (supervisor or owner)')
  WHERE EXISTS (
    SELECT 1 FROM order_returns r JOIN order_items oi ON oi.id = r.source_item_id JOIN orders so ON so.id = oi.order_id
    WHERE r.order_id = NEW.id
      AND julianday(NEW.business_date) - julianday(so.business_date)
          > COALESCE((SELECT json_extract(value, '$') FROM store_settings WHERE key = 'exchange.window_days'), 7))
    AND COALESCE((SELECT perms & 128 FROM v_staff_effective WHERE staff_id = NEW.staff_id AND is_active = 1), 0) = 0
    AND (NEW.late_approved_by IS NULL OR NEW.late_approved_by = NEW.staff_id
         OR COALESCE((SELECT perms & 128 FROM v_staff_effective WHERE staff_id = NEW.late_approved_by AND is_active = 1), 0) = 0);
END;

DROP TRIGGER trg_orders_void_verify;
CREATE TRIGGER trg_orders_void_verify BEFORE UPDATE OF status ON orders
WHEN (SELECT applying FROM sync_ctx WHERE id = 1) = 0 AND OLD.status = 'completed' AND NEW.status = 'voided' BEGIN
  SELECT RAISE(ABORT, 'void: only on the device that made the sale')
  WHERE NEW.device_id IS NOT json_extract((SELECT value FROM device_settings WHERE key = 'device.id'), '$');
  SELECT RAISE(ABORT, 'void: the order''s cash session is closed — sales are final')
  WHERE (SELECT status FROM cash_sessions WHERE id = NEW.cash_session_id) IS NOT 'open';
  SELECT RAISE(ABORT, 'void: an exchange, or an order whose goods were returned, cannot be voided')
  WHERE NEW.exchange_credit > 0
     OR EXISTS (SELECT 1 FROM order_returns r JOIN order_items oi ON oi.id = r.source_item_id WHERE oi.order_id = NEW.id);
  SELECT RAISE(ABORT, 'void: requires VOID_ORDER or a supervisor approval')
  WHERE COALESCE((SELECT perms & 32 FROM v_staff_effective WHERE staff_id = NEW.voided_by AND is_active = 1), 0) = 0
    AND (NEW.void_approved_by IS NULL OR NEW.void_approved_by = NEW.voided_by
         OR COALESCE((SELECT perms & 32 FROM v_staff_effective WHERE staff_id = NEW.void_approved_by AND is_active = 1), 0) = 0);
END;

DROP TRIGGER trg_orders_state_machine;
CREATE TRIGGER trg_orders_state_machine BEFORE UPDATE ON orders
WHEN NOT (
      ((OLD.status = 'pending' AND NEW.status = 'completed') OR (OLD.status = 'completed' AND NEW.status = 'voided'))
  AND NEW.id = OLD.id AND NEW.device_id = OLD.device_id AND NEW.cash_session_id = OLD.cash_session_id
  AND NEW.receipt_no = OLD.receipt_no AND NEW.checkout_token = OLD.checkout_token
  AND NEW.staff_id = OLD.staff_id AND NEW.sold_by IS OLD.sold_by AND NEW.late_approved_by IS OLD.late_approved_by
  AND NEW.exchange_credit = OLD.exchange_credit AND NEW.returned_items = OLD.returned_items
  AND NEW.returned_gross = OLD.returned_gross AND NEW.returned_discount = OLD.returned_discount
  AND NEW.returned_net = OLD.returned_net AND NEW.returned_tax = OLD.returned_tax
  AND NEW.returned_cost = OLD.returned_cost
  AND NEW.approved_by IS OLD.approved_by AND NEW.customer_id IS OLD.customer_id
  AND NEW.line_count = OLD.line_count AND NEW.item_count = OLD.item_count
  AND NEW.gross_sales = OLD.gross_sales AND NEW.discount_total = OLD.discount_total
  AND NEW.net_sales = OLD.net_sales AND NEW.tax_total = OLD.tax_total AND NEW.rounding = OLD.rounding
  AND NEW.grand_total = OLD.grand_total AND NEW.paid_total = OLD.paid_total AND NEW.change_due = OLD.change_due
  AND NEW.cost_total = OLD.cost_total AND NEW.quick_sale_net = OLD.quick_sale_net
  AND NEW.unknown_cost_net = OLD.unknown_cost_net
  AND NEW.business_date = OLD.business_date AND NEW.completed_at = OLD.completed_at
)
BEGIN SELECT RAISE(ABORT, 'orders: illegal state transition or mutation of a sold order'); END;

DROP TRIGGER trg_rollup_seal;
CREATE TRIGGER trg_rollup_seal AFTER UPDATE OF status ON orders
WHEN OLD.status = 'pending' AND NEW.status = 'completed' BEGIN
  INSERT INTO daily_sales_summary(business_date, device_id) VALUES (NEW.business_date, NEW.device_id)
  ON CONFLICT DO NOTHING;
  UPDATE daily_sales_summary SET
    sales_count = sales_count + 1, gross_sales = gross_sales + NEW.gross_sales - NEW.returned_gross,
    discounts = discounts + NEW.discount_total - NEW.returned_discount, net_sales = net_sales + NEW.net_sales - NEW.returned_net,
    tax_total = tax_total + NEW.tax_total - NEW.returned_tax, cogs = cogs + NEW.cost_total - NEW.returned_cost,
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

DROP TRIGGER trg_rollup_insert_completed;
CREATE TRIGGER trg_rollup_insert_completed AFTER INSERT ON orders WHEN NEW.status = 'completed' BEGIN
  INSERT INTO daily_sales_summary(business_date, device_id) VALUES (NEW.business_date, NEW.device_id)
  ON CONFLICT DO NOTHING;
  UPDATE daily_sales_summary SET
    sales_count = sales_count + 1, gross_sales = gross_sales + NEW.gross_sales - NEW.returned_gross,
    discounts = discounts + NEW.discount_total - NEW.returned_discount, net_sales = net_sales + NEW.net_sales - NEW.returned_net,
    tax_total = tax_total + NEW.tax_total - NEW.returned_tax, cogs = cogs + NEW.cost_total - NEW.returned_cost,
    quick_sale_net = quick_sale_net + NEW.quick_sale_net, unknown_cost_net = unknown_cost_net + NEW.unknown_cost_net,
    customer_orders = customer_orders + (NEW.customer_id IS NOT NULL)
  WHERE business_date = NEW.business_date AND device_id = NEW.device_id;
END;

DROP TRIGGER trg_customer_stats_add;
CREATE TRIGGER trg_customer_stats_add AFTER UPDATE OF status ON orders
WHEN OLD.status = 'pending' AND NEW.status = 'completed' AND NEW.customer_id IS NOT NULL BEGIN
  INSERT INTO customer_stats(customer_id, visit_count, total_spend, total_net, first_visit_date, last_visit_date)
  VALUES (NEW.customer_id, 1, NEW.grand_total - NEW.exchange_credit, NEW.net_sales - NEW.returned_net, NEW.business_date, NEW.business_date)
  ON CONFLICT(customer_id) DO UPDATE SET
    visit_count = visit_count + 1, total_spend = total_spend + excluded.total_spend, total_net = total_net + excluded.total_net,
    first_visit_date = MIN(COALESCE(first_visit_date, excluded.first_visit_date), excluded.first_visit_date),
    last_visit_date  = MAX(COALESCE(last_visit_date,  excluded.last_visit_date),  excluded.last_visit_date);
END;

DROP TRIGGER trg_customer_stats_add_pulled;
CREATE TRIGGER trg_customer_stats_add_pulled AFTER INSERT ON orders
WHEN NEW.status = 'completed' AND NEW.customer_id IS NOT NULL BEGIN
  INSERT INTO customer_stats(customer_id, visit_count, total_spend, total_net, first_visit_date, last_visit_date)
  VALUES (NEW.customer_id, 1, NEW.grand_total - NEW.exchange_credit, NEW.net_sales - NEW.returned_net, NEW.business_date, NEW.business_date)
  ON CONFLICT(customer_id) DO UPDATE SET
    visit_count = visit_count + 1, total_spend = total_spend + excluded.total_spend, total_net = total_net + excluded.total_net,
    first_visit_date = MIN(COALESCE(first_visit_date, excluded.first_visit_date), excluded.first_visit_date),
    last_visit_date  = MAX(COALESCE(last_visit_date,  excluded.last_visit_date),  excluded.last_visit_date);
END;

DROP TRIGGER trg_rollup_void;
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
  SELECT c.cid, COUNT(o.id), COALESCE(SUM(o.grand_total - o.exchange_credit), 0), COALESCE(SUM(o.net_sales - o.returned_net), 0),
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

