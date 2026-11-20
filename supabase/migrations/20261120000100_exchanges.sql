-- Mirror of SQLite migration 0005 (item-for-item exchanges, decision #26): an exchange is an ordinary sale of the
-- replacement goods (`orders.exchange_credit > 0`) plus insert-only `order_returns` rows naming the returned lines
-- of earlier orders. Returned goods come back as `return` stock movements mirroring the original sale slices.
-- `paid_total = grand_total` still holds: the credit counts as paid; payments cover `grand_total − exchange_credit`.
-- There is never a cash refund or a credit note. Older terminals that push orders without the new columns get
-- the defaults (0 / NULL), so this migration is safe to deploy before every counter is updated.

-- ═════════ orders: the exchange header ═════════
ALTER TABLE pos.orders
  ADD COLUMN exchange_credit bigint NOT NULL DEFAULT 0 CHECK (exchange_credit >= 0),
  ADD COLUMN returned_items bigint NOT NULL DEFAULT 0 CHECK (returned_items >= 0),
  ADD COLUMN returned_gross bigint NOT NULL DEFAULT 0 CHECK (returned_gross >= 0),
  ADD COLUMN returned_discount bigint NOT NULL DEFAULT 0 CHECK (returned_discount >= 0),
  ADD COLUMN returned_net bigint NOT NULL DEFAULT 0 CHECK (returned_net >= 0),
  ADD COLUMN returned_tax bigint NOT NULL DEFAULT 0 CHECK (returned_tax >= 0),
  ADD COLUMN returned_cost bigint NOT NULL DEFAULT 0 CHECK (returned_cost >= 0),
  ADD COLUMN late_approved_by uuid REFERENCES pos.staff(id),   -- EXCHANGE_PROCESS / EXCHANGE_LATE approver
  ADD CONSTRAINT orders_exchange_credit_within_total CHECK (exchange_credit <= grand_total);
CREATE INDEX ix_orders_exchange ON pos.orders(store_id, business_date) WHERE exchange_credit > 0;

-- A terminal that predates exchanges pushes order rows without the new columns, which arrive as NULL. This
-- trigger runs before every other BEFORE trigger on the table (name order) and turns them into the defaults (an
-- insert) or leaves them as they were (the void transition), so a counter that has not been updated yet keeps
-- syncing. An updated counter always sends every column.
CREATE FUNCTION pos.orders_exchange_defaults() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    NEW.exchange_credit := COALESCE(NEW.exchange_credit, 0);
    NEW.returned_items := COALESCE(NEW.returned_items, 0);
    NEW.returned_gross := COALESCE(NEW.returned_gross, 0);
    NEW.returned_discount := COALESCE(NEW.returned_discount, 0);
    NEW.returned_net := COALESCE(NEW.returned_net, 0);
    NEW.returned_tax := COALESCE(NEW.returned_tax, 0);
    NEW.returned_cost := COALESCE(NEW.returned_cost, 0);
  ELSE
    NEW.exchange_credit := COALESCE(NEW.exchange_credit, OLD.exchange_credit);
    NEW.returned_items := COALESCE(NEW.returned_items, OLD.returned_items);
    NEW.returned_gross := COALESCE(NEW.returned_gross, OLD.returned_gross);
    NEW.returned_discount := COALESCE(NEW.returned_discount, OLD.returned_discount);
    NEW.returned_net := COALESCE(NEW.returned_net, OLD.returned_net);
    NEW.returned_tax := COALESCE(NEW.returned_tax, OLD.returned_tax);
    NEW.returned_cost := COALESCE(NEW.returned_cost, OLD.returned_cost);
    NEW.late_approved_by := COALESCE(NEW.late_approved_by, OLD.late_approved_by);
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER trg_orders_0_exchange_defaults BEFORE INSERT OR UPDATE ON pos.orders
  FOR EACH ROW EXECUTE FUNCTION pos.orders_exchange_defaults();

-- STRUCTURAL: completed → voided stays the only change; the exchange columns are as immutable as the totals.
CREATE OR REPLACE FUNCTION pos.orders_state_machine() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NOT (OLD.status = 'completed' AND NEW.status = 'voided'
      AND NEW.id = OLD.id AND NEW.store_id = OLD.store_id AND NEW.device_id = OLD.device_id
      AND NEW.cash_session_id = OLD.cash_session_id
      AND NEW.receipt_no = OLD.receipt_no AND NEW.checkout_token = OLD.checkout_token
      AND NEW.staff_id = OLD.staff_id AND NEW.sold_by IS NOT DISTINCT FROM OLD.sold_by
      AND NEW.approved_by IS NOT DISTINCT FROM OLD.approved_by
      AND NEW.late_approved_by IS NOT DISTINCT FROM OLD.late_approved_by
      AND NEW.customer_id IS NOT DISTINCT FROM OLD.customer_id
      AND NEW.line_count = OLD.line_count AND NEW.item_count = OLD.item_count
      AND NEW.gross_sales = OLD.gross_sales AND NEW.discount_total = OLD.discount_total
      AND NEW.net_sales = OLD.net_sales AND NEW.tax_total = OLD.tax_total AND NEW.rounding = OLD.rounding
      AND NEW.grand_total = OLD.grand_total AND NEW.paid_total = OLD.paid_total AND NEW.change_due = OLD.change_due
      AND NEW.exchange_credit = OLD.exchange_credit AND NEW.returned_items = OLD.returned_items
      AND NEW.returned_gross = OLD.returned_gross AND NEW.returned_discount = OLD.returned_discount
      AND NEW.returned_net = OLD.returned_net AND NEW.returned_tax = OLD.returned_tax
      AND NEW.returned_cost = OLD.returned_cost
      AND NEW.cost_total = OLD.cost_total AND NEW.quick_sale_net = OLD.quick_sale_net
      AND NEW.unknown_cost_net = OLD.unknown_cost_net
      AND NEW.business_date = OLD.business_date AND NEW.completed_at = OLD.completed_at
      AND NEW.updated_by_device = OLD.device_id) THEN
    RAISE EXCEPTION 'orders: illegal state transition or mutation of a sold order' USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;

-- ═════════ order_returns (append-only) ═════════
CREATE TABLE pos.order_returns (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), device_id uuid NOT NULL,
  order_id uuid NOT NULL REFERENCES pos.orders(id),              -- the exchange (replacement) order
  source_item_id uuid NOT NULL REFERENCES pos.order_items(id),   -- the returned line of an earlier order
  qty bigint NOT NULL CHECK (qty > 0),
  line_gross bigint NOT NULL CHECK (line_gross >= 0),
  line_discount bigint NOT NULL CHECK (line_discount >= 0),
  line_total bigint NOT NULL CHECK (line_total >= 0),            -- credited: what the customer paid for these units
  line_net bigint NOT NULL CHECK (line_net >= 0),
  tax_amount bigint NOT NULL CHECK (tax_amount >= 0),
  cost_total bigint NOT NULL CHECK (cost_total >= 0),
  created_at timestamptz NOT NULL,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  CHECK (line_total = line_gross - line_discount),
  UNIQUE (order_id, source_item_id)
);
CREATE INDEX ix_returns_source ON pos.order_returns(source_item_id);

-- Registered after the payments so a pull applies the exchange order, its payments, then its returns.
INSERT INTO pos.synced_tables (name, kind, pk, apply_order) VALUES ('order_returns', 'insert_only', '{id}', 19);

-- The same per-table plumbing the schema migration gives every synced table (insert_only branch).
CREATE TRIGGER trg_order_returns_server_seq BEFORE INSERT OR UPDATE ON pos.order_returns
  FOR EACH ROW EXECUTE FUNCTION pos.stamp_server_seq();
CREATE INDEX ix_order_returns_cursor ON pos.order_returns (store_id, server_seq);
ALTER TABLE pos.order_returns ENABLE ROW LEVEL SECURITY;
CREATE POLICY order_returns_select ON pos.order_returns FOR SELECT TO pos_sync
  USING (store_id = pos.claim_store_id());
CREATE POLICY order_returns_insert ON pos.order_returns FOR INSERT TO pos_sync
  WITH CHECK (store_id = pos.claim_store_id());
CREATE TRIGGER trg_order_returns_append_only BEFORE UPDATE OR DELETE ON pos.order_returns
  FOR EACH ROW EXECUTE FUNCTION pos.reject_change();
REVOKE ALL ON pos.order_returns FROM anon, authenticated, PUBLIC;
GRANT SELECT, INSERT ON pos.order_returns TO pos_sync;

-- ═════════ stock_movements: the 'return' reason ═════════
-- Three checks name the reasons: the reason list, and the tie between sale/void and order_item_id. Dropped by
-- definition (their generated names are not stable) and recreated with 'return' alongside 'sale' and 'void'.
DO $$
DECLARE c record;
BEGIN
  FOR c IN SELECT conname FROM pg_constraint
           WHERE conrelid = 'pos.stock_movements'::regclass AND contype = 'c'
             AND (pg_get_constraintdef(oid) LIKE '%batch_intake%'
                  OR pg_get_constraintdef(oid) LIKE '%order_item_id IS NOT NULL%') LOOP
    EXECUTE format('ALTER TABLE pos.stock_movements DROP CONSTRAINT %I', c.conname);
  END LOOP;
END $$;
ALTER TABLE pos.stock_movements
  ADD CONSTRAINT stock_movements_reason_check CHECK (reason IN ('batch_intake','batch_reopen','sale','void','return',
                                                                 'damage','count_correction','adjustment')),
  ADD CONSTRAINT stock_movements_order_item_tie CHECK ((reason IN ('sale','void','return')) = (order_item_id IS NOT NULL)),
  -- A return puts goods back: positive quantity, revenue reversed (≤ 0), and it names the exchange order.
  ADD CONSTRAINT stock_movements_return_shape CHECK (reason <> 'return'
    OR (qty_delta > 0 AND revenue_net <= 0 AND ref_id IS NOT NULL));

-- ═════════ derived views: sold figures and sales are net of returns ═════════
CREATE OR REPLACE VIEW pos.batch_item_stock WITH (security_invoker = true) AS
SELECT bi.store_id, bi.id AS batch_item_id, bi.batch_id, bi.product_id,
  COALESCE(SUM(m.qty_delta) FILTER (WHERE m.reason IN ('batch_intake','batch_reopen')), 0)::bigint AS received_qty,
  COALESCE(SUM(m.qty_delta), 0)::bigint AS on_hand,
  COALESCE(SUM(-m.qty_delta) FILTER (WHERE m.reason IN ('sale','void','return')), 0)::bigint AS sold_qty,
  COALESCE(SUM(m.revenue_net) FILTER (WHERE m.reason IN ('sale','void','return')), 0)::bigint AS sold_revenue,
  COALESCE(SUM(-m.qty_delta * m.unit_cost) FILTER (WHERE m.reason IN ('sale','void','return')), 0)::bigint AS sold_cost,
  COALESCE(SUM(-m.qty_delta) FILTER (WHERE m.reason IN ('damage','count_correction','adjustment')), 0)::bigint
    AS written_off_qty,
  COALESCE(SUM(-m.qty_delta * m.unit_cost) FILTER (WHERE m.reason IN ('damage','count_correction','adjustment')), 0)::bigint
    AS written_off_cost
FROM pos.batch_items bi JOIN pos.stock_movements m ON m.batch_item_id = bi.id
GROUP BY bi.store_id, bi.id, bi.batch_id, bi.product_id;

-- Existing columns keep their names and types (their sums now net out returned goods); the exchange figures are
-- appended.
CREATE OR REPLACE VIEW pos.daily_sales_summary WITH (security_invoker = true) AS
SELECT store_id, business_date, device_id,
  COUNT(*) FILTER (WHERE status = 'completed')::bigint AS sales_count,
  COUNT(*) FILTER (WHERE status = 'voided')::bigint AS void_count,
  COALESCE(SUM(grand_total) FILTER (WHERE status = 'voided'), 0)::bigint AS void_value,
  COALESCE(SUM(gross_sales - returned_gross) FILTER (WHERE status = 'completed'), 0)::bigint AS gross_sales,
  COALESCE(SUM(discount_total - returned_discount) FILTER (WHERE status = 'completed'), 0)::bigint AS discounts,
  COALESCE(SUM(net_sales - returned_net) FILTER (WHERE status = 'completed'), 0)::bigint AS net_sales,
  COALESCE(SUM(tax_total - returned_tax) FILTER (WHERE status = 'completed'), 0)::bigint AS tax_total,
  COALESCE(SUM(cost_total - returned_cost) FILTER (WHERE status = 'completed'), 0)::bigint AS cogs,
  COALESCE(SUM(quick_sale_net) FILTER (WHERE status = 'completed'), 0)::bigint AS quick_sale_net,
  COALESCE(SUM(unknown_cost_net) FILTER (WHERE status = 'completed'), 0)::bigint AS unknown_cost_net,
  COUNT(*) FILTER (WHERE status = 'completed' AND customer_id IS NOT NULL)::bigint AS customer_orders,
  COUNT(*) FILTER (WHERE status = 'completed' AND exchange_credit > 0)::bigint AS exchange_count,
  COALESCE(SUM(exchange_credit) FILTER (WHERE status = 'completed'), 0)::bigint AS exchange_credit,
  COALESCE(SUM(returned_items) FILTER (WHERE status = 'completed'), 0)::bigint AS returned_items
FROM pos.orders GROUP BY store_id, business_date, device_id;

CREATE OR REPLACE VIEW pos.customer_stats WITH (security_invoker = true) AS
SELECT o.store_id, o.customer_id, COUNT(*)::bigint AS visit_count,
  SUM(o.grand_total - o.exchange_credit)::bigint AS total_spend, SUM(o.net_sales - o.returned_net)::bigint AS total_net,
  MIN(o.business_date) AS first_visit_date, MAX(o.business_date) AS last_visit_date
FROM pos.orders o WHERE o.status = 'completed' AND o.customer_id IS NOT NULL
GROUP BY o.store_id, o.customer_id;
