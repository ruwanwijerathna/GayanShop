-- GayanShop POS — cloud schema v1 (Block 3.1): the Postgres mirror of src-tauri/migrations (doc §4.3, §4.4).
--
-- Layout
--   * Business tables live in schema `pos`, which PostgREST does not expose. Devices never touch them directly:
--     they call the RPCs in `public` (sync_push / sync_pull / sync_health, Block 3.2), which run as the
--     NOLOGIN role `pos_sync`. That role has no BYPASSRLS, so every row the RPCs read or write still passes the
--     RLS policies below: store isolation holds at the table layer even if an RPC had a bug.
--   * Plain Postgres only. Claims are read from `request.jwt.claims` (what PostgREST sets from the verified JWT),
--     not from Supabase's `auth.jwt()`, so the same schema runs behind a bare PostgREST on the owner's own VPS.
--
-- Types (doc §4.4): ids uuid, timestamps timestamptz (millisecond precision on the wire), business dates date,
-- money and masks bigint, flags boolean. JSON-in-text columns (store_settings.value, products.attributes,
-- cash_sessions.denominations, audit_logs.metadata) stay `text` with a JSON validity check: jsonb would reorder
-- keys and drop whitespace, so a row pulled by another terminal would no longer be byte-identical to what its
-- origin wrote, and audit_logs.metadata feeds the audit hash chain.
--
-- Foreign keys mirror the SQLite schema exactly. sync_pull's parent closure walks these constraints, so the
-- ancestors it ships are precisely the ones a terminal's own foreign keys need.

CREATE SCHEMA IF NOT EXISTS pos;
REVOKE ALL ON SCHEMA pos FROM PUBLIC;

-- The role the sync RPCs run as (they are SECURITY DEFINER functions owned by it).
DO $$ BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'pos_sync') THEN
    CREATE ROLE pos_sync NOLOGIN NOINHERIT NOBYPASSRLS;
  END IF;
END $$;
GRANT pos_sync TO postgres;               -- so migrations can hand function ownership to it
GRANT USAGE ON SCHEMA pos TO pos_sync;

-- ═════════ claims (portable: PostgREST sets request.jwt.claims from the verified JWT) ═════════
CREATE FUNCTION pos.claims() RETURNS jsonb LANGUAGE sql STABLE AS $$
  SELECT COALESCE(NULLIF(current_setting('request.jwt.claims', true), ''), '{}')::jsonb
$$;
-- NULL unless the caller is a registered device identity (app_metadata is written only by the service role).
CREATE FUNCTION pos.claim_store_id() RETURNS uuid LANGUAGE sql STABLE AS $$
  SELECT CASE WHEN pos.claims() -> 'app_metadata' ->> 'role' = 'device'
              THEN (pos.claims() -> 'app_metadata' ->> 'store_id')::uuid END
$$;
CREATE FUNCTION pos.claim_device_id() RETURNS uuid LANGUAGE sql STABLE AS $$
  SELECT CASE WHEN pos.claims() -> 'app_metadata' ->> 'role' = 'device'
              THEN (pos.claims() -> 'app_metadata' ->> 'device_id')::uuid END
$$;

-- The device id stamped on rows the server itself changes (doc §6.3, barcode conflicts): never a real device,
-- so those rows come back to every terminal, their origin included.
CREATE FUNCTION pos.system_device() RETURNS uuid LANGUAGE sql IMMUTABLE AS $$
  SELECT '00000000-0000-0000-0000-000000000000'::uuid
$$;

-- The terminal's timestamp format, exactly: `2026-11-08T09:30:00.000Z` (clock::iso). Clients compare
-- updated_at as text for last-write-wins, so the server must render timestamps back the same way.
CREATE FUNCTION pos.ts(t timestamptz) RETURNS text LANGUAGE sql IMMUTABLE AS $$
  SELECT to_char(t AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
$$;

CREATE FUNCTION pos.is_json(t text) RETURNS boolean LANGUAGE plpgsql IMMUTABLE AS $$
BEGIN
  PERFORM t::jsonb;
  RETURN true;
EXCEPTION WHEN others THEN
  RETURN false;
END $$;

-- ═════════ cursor: one store-wide ordering (doc §6.1, §6.2) ═════════
-- One global sequence. It is a safe pull cursor because sync_push takes pg_advisory_xact_lock per store, so
-- within a store server_seq assignment order equals commit order.
CREATE SEQUENCE pos.server_seq AS bigint;
GRANT USAGE ON SEQUENCE pos.server_seq TO pos_sync;

CREATE FUNCTION pos.stamp_server_seq() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  NEW.server_seq := nextval('pos.server_seq');
  NEW.server_received_at := now();
  RETURN NEW;
END $$;

CREATE FUNCTION pos.reject_change() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION '% are append-only', TG_TABLE_NAME USING ERRCODE = 'check_violation';
END $$;

-- ═════════ tenancy ═════════
CREATE TABLE pos.stores (
  id uuid PRIMARY KEY,
  name text NOT NULL,
  owner_user_id uuid NOT NULL UNIQUE,                -- the owner's cloud account (auth.users.id); one store each
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE pos.devices (
  id uuid PRIMARY KEY,                               -- the terminal's own device_id, adopted at registration
  store_id uuid NOT NULL REFERENCES pos.stores(id),
  code text NOT NULL CHECK (code ~ '^T0[1-9]$'),
  name text NOT NULL,
  auth_user_id uuid UNIQUE,                          -- the device identity (auth.users.id)
  app_version text,
  registered_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz,
  last_audit_seq bigint NOT NULL DEFAULT 0,
  last_audit_hash text NOT NULL DEFAULT '0000000000000000000000000000000000000000000000000000000000000000',
  chain_broken_at timestamptz,                       -- set when a pushed audit row does not extend the chain
  chain_broken_seq bigint,
  UNIQUE (store_id, code)
);

-- Idempotency ledger (doc §4.4): a replayed op returns its first result.
CREATE TABLE pos.processed_ops (
  op_id uuid PRIMARY KEY,
  store_id uuid NOT NULL REFERENCES pos.stores(id),
  device_id uuid NOT NULL,
  table_name text NOT NULL,
  result text NOT NULL CHECK (result IN ('ok','rejected')),
  reason text,
  processed_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_processed_ops_device ON pos.processed_ops(device_id, processed_at);

-- ═════════ settings ═════════
CREATE TABLE pos.store_settings (
  key text NOT NULL,
  store_id uuid NOT NULL REFERENCES pos.stores(id),
  value text NOT NULL CHECK (pos.is_json(value)),
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (store_id, key)
);

-- ═════════ RBAC ═════════
CREATE TABLE pos.roles (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id),
  name text NOT NULL,
  permissions bigint NOT NULL DEFAULT 0,
  max_discount_bp bigint NOT NULL DEFAULT 0 CHECK (max_discount_bp BETWEEN 0 AND 10000),
  max_override_bp bigint NOT NULL DEFAULT 0 CHECK (max_override_bp BETWEEN 0 AND 10000),
  is_system boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX ux_roles_name ON pos.roles(store_id, name) WHERE deleted_at IS NULL;

CREATE TABLE pos.staff (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id),
  display_name text NOT NULL,
  role_id uuid NOT NULL REFERENCES pos.roles(id),
  pin_hash text NOT NULL,                            -- device identities only; never a dashboard role (§4.4)
  grant_mask bigint NOT NULL DEFAULT 0,
  deny_mask bigint NOT NULL DEFAULT 0,
  max_discount_bp bigint CHECK (max_discount_bp IS NULL OR max_discount_bp BETWEEN 0 AND 10000),
  ui_language text NOT NULL DEFAULT 'en' CHECK (ui_language IN ('en','si')),
  is_active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_staff_role ON pos.staff(role_id);

-- ═════════ catalog ═════════
CREATE TABLE pos.categories (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id),
  name text NOT NULL, parent_id uuid REFERENCES pos.categories(id),
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE pos.products (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id),
  kind text NOT NULL DEFAULT 'standard' CHECK (kind IN ('standard','generic')),
  name text NOT NULL,
  name_si text,
  sku text, barcode text,
  category_id uuid REFERENCES pos.categories(id),
  default_cost bigint NOT NULL DEFAULT 0 CHECK (default_cost >= 0),
  price bigint NOT NULL CHECK (price >= 0),
  min_price bigint,
  track_stock boolean NOT NULL DEFAULT true,
  allow_negative_stock boolean NOT NULL DEFAULT true,
  tax_rate_bp bigint NOT NULL DEFAULT 0,
  attributes text NOT NULL DEFAULT '{}' CHECK (pos.is_json(attributes)),
  is_active boolean NOT NULL DEFAULT true,
  barcode_conflict boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now()
);
-- Live, active barcodes are unique; sync_push parks a colliding product as inactive + barcode_conflict (§6.3).
CREATE UNIQUE INDEX ux_products_barcode ON pos.products(store_id, barcode)
  WHERE barcode IS NOT NULL AND deleted_at IS NULL AND is_active;
CREATE UNIQUE INDEX ux_products_sku ON pos.products(store_id, sku)
  WHERE sku IS NOT NULL AND deleted_at IS NULL;

-- ═════════ batches (cost layers) ═════════
CREATE TABLE pos.batches (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id),
  code text NOT NULL, name text NOT NULL,
  supplier_name text, supplier_ref text,
  purchased_at timestamptz NOT NULL,
  received_at timestamptz, received_business_date date,
  total_cost bigint NOT NULL DEFAULT 0 CHECK (total_cost >= 0),
  extra_costs bigint NOT NULL DEFAULT 0 CHECK (extra_costs >= 0),
  allocation_method text NOT NULL CHECK (allocation_method IN ('manual','even_per_unit','by_retail_value')),
  allocation_variance bigint NOT NULL DEFAULT 0,
  status text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','received','closed')),
  is_system boolean NOT NULL DEFAULT false,
  received_by uuid REFERENCES pos.staff(id),
  notes text,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  CHECK (status = 'draft' OR (received_at IS NOT NULL AND received_business_date IS NOT NULL))
);
CREATE UNIQUE INDEX ux_batches_code ON pos.batches(store_id, code) WHERE deleted_at IS NULL;

CREATE TABLE pos.batch_items (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id),
  batch_id uuid NOT NULL REFERENCES pos.batches(id),
  product_id uuid NOT NULL REFERENCES pos.products(id),
  qty_received bigint NOT NULL CHECK (qty_received >= 0),
  unit_cost bigint NOT NULL DEFAULT 0 CHECK (unit_cost >= 0),
  retail_price_at_intake bigint NOT NULL DEFAULT 0,
  manual_unit_cost bigint,
  added_after_receipt boolean NOT NULL DEFAULT false,
  note text,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (batch_id, product_id)
);
CREATE INDEX ix_batch_items_product ON pos.batch_items(product_id);

-- ═════════ cash ═════════
CREATE TABLE pos.cash_sessions (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), device_id uuid NOT NULL,
  business_date date NOT NULL,
  status text NOT NULL DEFAULT 'open' CHECK (status IN ('open','closed')),
  opened_by uuid NOT NULL REFERENCES pos.staff(id), opened_at timestamptz NOT NULL,
  opening_float bigint NOT NULL DEFAULT 0 CHECK (opening_float >= 0),
  closed_by uuid REFERENCES pos.staff(id), closed_at timestamptz,
  expected_cash bigint, counted_cash bigint, variance bigint,
  denominations text CHECK (denominations IS NULL OR pos.is_json(denominations)), close_note text,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX ux_one_open_session ON pos.cash_sessions(device_id) WHERE status = 'open';

CREATE TABLE pos.cash_movements (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), device_id uuid NOT NULL,
  cash_session_id uuid NOT NULL REFERENCES pos.cash_sessions(id),
  type text NOT NULL CHECK (type IN ('pay_in','pay_out','no_sale')),
  amount bigint NOT NULL CHECK (amount >= 0),
  reason text NOT NULL,
  staff_id uuid NOT NULL, approved_by uuid,
  business_date date NOT NULL,
  created_at timestamptz NOT NULL,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_cashmov_session ON pos.cash_movements(cash_session_id);

-- ═════════ customers (id = UUIDv5(store_id, phone_e164)) ═════════
CREATE TABLE pos.customers (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id),
  phone_e164 text CHECK (phone_e164 IS NULL OR phone_e164 ~ '^\+94[0-9]{9}$'),
  display_name text, note text,
  first_seen_at timestamptz NOT NULL,
  erased_at timestamptz,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  CHECK (erased_at IS NULL OR (phone_e164 IS NULL AND display_name IS NULL AND note IS NULL))
);
CREATE UNIQUE INDEX ux_customers_phone ON pos.customers(store_id, phone_e164)
  WHERE phone_e164 IS NOT NULL AND deleted_at IS NULL;

-- ═════════ orders: only sealed orders reach the cloud; the one later change is completed → voided ═════════
CREATE TABLE pos.orders (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), device_id uuid NOT NULL,
  cash_session_id uuid NOT NULL REFERENCES pos.cash_sessions(id),
  receipt_no text NOT NULL,
  checkout_token text NOT NULL,
  type text NOT NULL DEFAULT 'sale' CHECK (type IN ('sale')),
  status text NOT NULL CHECK (status IN ('completed','voided')),
  staff_id uuid NOT NULL REFERENCES pos.staff(id),
  approved_by uuid REFERENCES pos.staff(id),
  customer_id uuid REFERENCES pos.customers(id),
  line_count bigint NOT NULL CHECK (line_count > 0),
  item_count bigint NOT NULL CHECK (item_count > 0),
  gross_sales bigint NOT NULL CHECK (gross_sales >= 0),
  discount_total bigint NOT NULL DEFAULT 0 CHECK (discount_total >= 0),
  net_sales bigint NOT NULL CHECK (net_sales >= 0),
  tax_total bigint NOT NULL DEFAULT 0 CHECK (tax_total >= 0),
  rounding bigint NOT NULL DEFAULT 0,
  grand_total bigint NOT NULL CHECK (grand_total >= 0),
  paid_total bigint NOT NULL CHECK (paid_total >= 0),
  change_due bigint NOT NULL DEFAULT 0 CHECK (change_due >= 0),
  cost_total bigint NOT NULL DEFAULT 0,
  quick_sale_net bigint NOT NULL DEFAULT 0,
  unknown_cost_net bigint NOT NULL DEFAULT 0,
  business_date date NOT NULL,
  completed_at timestamptz NOT NULL,
  voided_at timestamptz, voided_by uuid REFERENCES pos.staff(id), void_approved_by uuid REFERENCES pos.staff(id),
  void_reason text CHECK (void_reason IN ('WRONG_ITEM','WRONG_PRICE','WRONG_QTY','DUPLICATE_SCAN','CUSTOMER_CANCELLED','OTHER')),
  void_note text,
  note text,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((status = 'voided') = (voided_at IS NOT NULL AND voided_by IS NOT NULL AND void_reason IS NOT NULL)),
  CHECK (void_reason IS DISTINCT FROM 'OTHER' OR length(trim(coalesce(void_note, ''))) > 0),
  CHECK (paid_total = grand_total)
);
CREATE UNIQUE INDEX ux_orders_receipt ON pos.orders(store_id, receipt_no);
CREATE UNIQUE INDEX ux_orders_checkout_token ON pos.orders(checkout_token);
CREATE INDEX ix_orders_bdate ON pos.orders(store_id, business_date, device_id);

-- STRUCTURAL: completed → voided is the only change, made by the device that made the sale, and nothing that
-- was sold, to whom, by whom, for how much or when may change with it.
CREATE FUNCTION pos.orders_state_machine() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF NOT (OLD.status = 'completed' AND NEW.status = 'voided'
      AND NEW.id = OLD.id AND NEW.store_id = OLD.store_id AND NEW.device_id = OLD.device_id
      AND NEW.cash_session_id = OLD.cash_session_id
      AND NEW.receipt_no = OLD.receipt_no AND NEW.checkout_token = OLD.checkout_token
      AND NEW.staff_id = OLD.staff_id AND NEW.approved_by IS NOT DISTINCT FROM OLD.approved_by
      AND NEW.customer_id IS NOT DISTINCT FROM OLD.customer_id
      AND NEW.line_count = OLD.line_count AND NEW.item_count = OLD.item_count
      AND NEW.gross_sales = OLD.gross_sales AND NEW.discount_total = OLD.discount_total
      AND NEW.net_sales = OLD.net_sales AND NEW.tax_total = OLD.tax_total AND NEW.rounding = OLD.rounding
      AND NEW.grand_total = OLD.grand_total AND NEW.paid_total = OLD.paid_total AND NEW.change_due = OLD.change_due
      AND NEW.cost_total = OLD.cost_total AND NEW.quick_sale_net = OLD.quick_sale_net
      AND NEW.unknown_cost_net = OLD.unknown_cost_net
      AND NEW.business_date = OLD.business_date AND NEW.completed_at = OLD.completed_at
      AND NEW.updated_by_device = OLD.device_id) THEN
    RAISE EXCEPTION 'orders: illegal state transition or mutation of a sold order' USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;

CREATE TABLE pos.order_items (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), device_id uuid NOT NULL,
  order_id uuid NOT NULL REFERENCES pos.orders(id),
  line_no bigint NOT NULL,
  product_id uuid REFERENCES pos.products(id),
  is_custom boolean NOT NULL DEFAULT false,
  custom_key text,
  batch_id uuid REFERENCES pos.batches(id),
  name text NOT NULL, barcode text,
  list_price bigint NOT NULL CHECK (list_price >= 0), unit_price bigint NOT NULL CHECK (unit_price >= 0),
  qty bigint NOT NULL CHECK (qty > 0),
  line_gross bigint NOT NULL,
  line_discount bigint NOT NULL DEFAULT 0,
  order_discount_share bigint NOT NULL DEFAULT 0,
  tax_rate_bp bigint NOT NULL DEFAULT 0,
  tax_amount bigint NOT NULL DEFAULT 0,
  line_total bigint NOT NULL,
  line_net bigint NOT NULL,
  cost_total bigint NOT NULL DEFAULT 0 CHECK (cost_total >= 0),
  cost_known boolean NOT NULL DEFAULT true,
  price_overridden boolean NOT NULL DEFAULT false, override_reason text, approved_by uuid REFERENCES pos.staff(id),
  created_at timestamptz NOT NULL,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((is_custom AND product_id IS NULL AND custom_key IS NOT NULL) OR (NOT is_custom AND product_id IS NOT NULL)),
  CHECK (line_gross = unit_price * qty),
  CHECK (line_total = line_gross - line_discount - order_discount_share),
  CHECK (line_discount >= 0 AND order_discount_share >= 0 AND line_total >= 0 AND line_net >= 0),
  CHECK (price_overridden = (unit_price < list_price)),
  UNIQUE (order_id, line_no)
);
CREATE INDEX ix_items_product ON pos.order_items(product_id);

CREATE TABLE pos.order_payments (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), device_id uuid NOT NULL,
  order_id uuid NOT NULL REFERENCES pos.orders(id),
  method text NOT NULL CHECK (method IN ('cash','card','bank_transfer','qr','other')),
  amount bigint NOT NULL CHECK (amount >= 0),
  tendered bigint,
  reference text,
  created_at timestamptz NOT NULL,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  CHECK (tendered IS NULL OR (method = 'cash' AND tendered >= amount))
);
CREATE INDEX ix_payments_order ON pos.order_payments(order_id);

-- ═════════ stock ledger (append-only) ═════════
CREATE TABLE pos.stock_movements (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), device_id uuid NOT NULL,
  product_id uuid NOT NULL REFERENCES pos.products(id),
  batch_item_id uuid NOT NULL REFERENCES pos.batch_items(id),
  qty_delta bigint NOT NULL CHECK (qty_delta <> 0),
  reason text NOT NULL CHECK (reason IN ('batch_intake','batch_reopen','sale','void',
                                         'damage','count_correction','adjustment')),
  ref_type text, ref_id text,
  order_item_id uuid REFERENCES pos.order_items(id),
  unit_cost bigint NOT NULL CHECK (unit_cost >= 0),
  revenue_net bigint,
  business_date date NOT NULL,
  staff_id uuid, approved_by uuid, note text,
  created_at timestamptz NOT NULL,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((reason IN ('sale','void')) = (order_item_id IS NOT NULL)),
  CHECK (reason <> 'sale' OR (qty_delta < 0 AND revenue_net >= 0)),
  CHECK (reason <> 'void' OR (qty_delta > 0 AND revenue_net <= 0))
);
CREATE INDEX ix_mov_product ON pos.stock_movements(product_id);
CREATE INDEX ix_mov_layer ON pos.stock_movements(batch_item_id);
CREATE INDEX ix_mov_order_item ON pos.stock_movements(order_item_id) WHERE order_item_id IS NOT NULL;

-- STRUCTURAL: a received batch's layer costs and quantities are locked (doc §3.1).
CREATE FUNCTION pos.batch_item_cost_lock() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF (NEW.unit_cost <> OLD.unit_cost OR NEW.qty_received <> OLD.qty_received)
     AND (SELECT status FROM pos.batches WHERE id = OLD.batch_id) <> 'draft' THEN
    RAISE EXCEPTION 'batch item cost/qty locked after receive' USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;

-- STRUCTURAL: a closed cash session never reopens.
CREATE FUNCTION pos.cash_session_no_reopen() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF OLD.status = 'closed' AND NEW.status <> 'closed' THEN
    RAISE EXCEPTION 'a closed cash session cannot be reopened' USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END $$;

-- ═════════ expenses ═════════
CREATE TABLE pos.expense_categories (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), name text NOT NULL,
  is_active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE pos.expenses (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id),
  category_id uuid NOT NULL REFERENCES pos.expense_categories(id),
  amount bigint NOT NULL CHECK (amount > 0),
  business_date date NOT NULL,
  payment_method text NOT NULL CHECK (payment_method IN ('cash','card','bank_transfer','qr','other')),
  paid_from_drawer boolean NOT NULL DEFAULT false,
  cash_session_id uuid REFERENCES pos.cash_sessions(id),
  payee text, note text,
  recorded_by uuid NOT NULL REFERENCES pos.staff(id),
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now()
);

-- ═════════ quick sale promotion (id = UUIDv5(store_id, custom_key)) ═════════
CREATE TABLE pos.quick_sale_links (
  id uuid PRIMARY KEY,
  store_id uuid NOT NULL REFERENCES pos.stores(id),
  custom_key text NOT NULL, display_name text NOT NULL,
  action text NOT NULL CHECK (action IN ('promoted','ignored')),
  product_id uuid REFERENCES pos.products(id), batch_id uuid REFERENCES pos.batches(id),
  decided_by uuid NOT NULL, decided_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, updated_by_device uuid NOT NULL,
  deleted_at timestamptz, version bigint NOT NULL DEFAULT 1,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  CHECK (action = 'ignored' OR product_id IS NOT NULL)
);
CREATE UNIQUE INDEX ux_qsl_key ON pos.quick_sale_links(store_id, custom_key) WHERE deleted_at IS NULL;

-- ═════════ audit (append-only, per-device hash chain; hash inputs stay text, byte for byte) ═════════
CREATE TABLE pos.audit_logs (
  id uuid PRIMARY KEY, store_id uuid NOT NULL REFERENCES pos.stores(id), device_id uuid NOT NULL,
  device_seq bigint NOT NULL CHECK (device_seq >= 1),
  staff_id uuid, approved_by uuid,
  action text NOT NULL,
  entity_type text, entity_id text,
  metadata text NOT NULL DEFAULT '{}' CHECK (pos.is_json(metadata)),
  business_date date NOT NULL,
  prev_hash text NOT NULL, hash text NOT NULL,
  created_at timestamptz NOT NULL,
  server_seq bigint NOT NULL, server_received_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (device_id, device_seq)
);

-- ═════════ the synced-table catalogue (sync_push / sync_pull read it) ═════════
-- kind: 'insert_only' (same id ⇒ no-op), 'order' (insert, then only the void transition), 'lww' (whole-row
-- last-write-wins by (updated_at, updated_by_device)). apply_order is parents-first.
CREATE TABLE pos.synced_tables (
  name text PRIMARY KEY,
  kind text NOT NULL CHECK (kind IN ('insert_only','order','lww')),
  pk text[] NOT NULL,
  apply_order int NOT NULL UNIQUE
);
INSERT INTO pos.synced_tables (name, kind, pk, apply_order) VALUES
  ('store_settings',     'lww',         '{store_id,key}', 1),
  ('roles',              'lww',         '{id}',  2),
  ('staff',              'lww',         '{id}',  3),
  ('categories',         'lww',         '{id}',  4),
  ('products',           'lww',         '{id}',  5),
  ('batches',            'lww',         '{id}',  6),
  ('batch_items',        'lww',         '{id}',  7),
  ('cash_sessions',      'lww',         '{id}',  8),
  ('cash_movements',     'insert_only', '{id}',  9),
  ('customers',          'lww',         '{id}', 10),
  ('orders',             'order',       '{id}', 11),
  ('order_items',        'insert_only', '{id}', 12),
  ('order_payments',     'insert_only', '{id}', 13),
  ('stock_movements',    'insert_only', '{id}', 14),
  ('expense_categories', 'lww',         '{id}', 15),
  ('expenses',           'lww',         '{id}', 16),
  ('quick_sale_links',   'lww',         '{id}', 17),
  ('audit_logs',         'insert_only', '{id}', 18);

-- ═════════ per-table plumbing: cursor stamp and index, structural triggers, RLS, privileges ═════════
DO $$
DECLARE t record;
BEGIN
  FOR t IN SELECT name, kind FROM pos.synced_tables LOOP
    EXECUTE format('CREATE TRIGGER trg_%1$s_server_seq BEFORE INSERT OR UPDATE ON pos.%1$I
                    FOR EACH ROW EXECUTE FUNCTION pos.stamp_server_seq()', t.name);
    EXECUTE format('CREATE INDEX ix_%1$s_cursor ON pos.%1$I (store_id, server_seq)', t.name);
    EXECUTE format('ALTER TABLE pos.%I ENABLE ROW LEVEL SECURITY', t.name);
    EXECUTE format('CREATE POLICY %1$s_select ON pos.%1$I FOR SELECT TO pos_sync
                    USING (store_id = pos.claim_store_id())', t.name);
    EXECUTE format('CREATE POLICY %1$s_insert ON pos.%1$I FOR INSERT TO pos_sync
                    WITH CHECK (store_id = pos.claim_store_id())', t.name);
    IF t.kind = 'insert_only' THEN
      -- No UPDATE or DELETE policy, no UPDATE or DELETE grant, and a trigger for any other writer (doc §4.4).
      EXECUTE format('CREATE TRIGGER trg_%1$s_append_only BEFORE UPDATE OR DELETE ON pos.%1$I
                      FOR EACH ROW EXECUTE FUNCTION pos.reject_change()', t.name);
      EXECUTE format('GRANT SELECT, INSERT ON pos.%I TO pos_sync', t.name);
    ELSE
      EXECUTE format('CREATE POLICY %1$s_update ON pos.%1$I FOR UPDATE TO pos_sync
                      USING (store_id = pos.claim_store_id()) WITH CHECK (store_id = pos.claim_store_id())', t.name);
      EXECUTE format('GRANT SELECT, INSERT, UPDATE ON pos.%I TO pos_sync', t.name);
    END IF;
  END LOOP;
END $$;

-- Orders: UPDATE only for the void transition; never DELETE.
CREATE TRIGGER trg_orders_state_machine BEFORE UPDATE ON pos.orders
  FOR EACH ROW EXECUTE FUNCTION pos.orders_state_machine();
CREATE TRIGGER trg_orders_no_delete BEFORE DELETE ON pos.orders
  FOR EACH ROW EXECUTE FUNCTION pos.reject_change();
CREATE TRIGGER trg_batch_item_cost_lock BEFORE UPDATE OF unit_cost, qty_received ON pos.batch_items
  FOR EACH ROW EXECUTE FUNCTION pos.batch_item_cost_lock();
CREATE TRIGGER trg_cash_session_no_reopen BEFORE UPDATE OF status ON pos.cash_sessions
  FOR EACH ROW EXECUTE FUNCTION pos.cash_session_no_reopen();

-- Tenancy tables: a device sees its own store; the RPCs update its own device row (last seen, audit chain).
ALTER TABLE pos.stores ENABLE ROW LEVEL SECURITY;
ALTER TABLE pos.devices ENABLE ROW LEVEL SECURITY;
ALTER TABLE pos.processed_ops ENABLE ROW LEVEL SECURITY;
CREATE POLICY stores_select ON pos.stores FOR SELECT TO pos_sync USING (id = pos.claim_store_id());
CREATE POLICY devices_select ON pos.devices FOR SELECT TO pos_sync USING (store_id = pos.claim_store_id());
CREATE POLICY devices_update ON pos.devices FOR UPDATE TO pos_sync
  USING (id = pos.claim_device_id() AND store_id = pos.claim_store_id())
  WITH CHECK (id = pos.claim_device_id() AND store_id = pos.claim_store_id());
CREATE POLICY processed_ops_select ON pos.processed_ops FOR SELECT TO pos_sync
  USING (store_id = pos.claim_store_id());
CREATE POLICY processed_ops_insert ON pos.processed_ops FOR INSERT TO pos_sync
  WITH CHECK (store_id = pos.claim_store_id() AND device_id = pos.claim_device_id());
ALTER TABLE pos.synced_tables ENABLE ROW LEVEL SECURITY;
CREATE POLICY synced_tables_select ON pos.synced_tables FOR SELECT TO pos_sync USING (true);
GRANT SELECT ON pos.stores, pos.synced_tables TO pos_sync;
GRANT SELECT, UPDATE (last_seen_at, app_version, last_audit_seq, last_audit_hash, chain_broken_at, chain_broken_seq)
  ON pos.devices TO pos_sync;
GRANT SELECT, INSERT ON pos.processed_ops TO pos_sync;

-- ═════════ derived read models: views, never stored (doc §4.4) ═════════
-- security_invoker: a view reads with the caller's rights, so RLS on the base tables still applies.
CREATE VIEW pos.stock_levels WITH (security_invoker = true) AS
SELECT store_id, product_id, SUM(qty_delta)::bigint AS on_hand
FROM pos.stock_movements GROUP BY store_id, product_id;

CREATE VIEW pos.batch_item_stock WITH (security_invoker = true) AS
SELECT bi.store_id, bi.id AS batch_item_id, bi.batch_id, bi.product_id,
  COALESCE(SUM(m.qty_delta) FILTER (WHERE m.reason IN ('batch_intake','batch_reopen')), 0)::bigint AS received_qty,
  COALESCE(SUM(m.qty_delta), 0)::bigint AS on_hand,
  COALESCE(SUM(-m.qty_delta) FILTER (WHERE m.reason IN ('sale','void')), 0)::bigint AS sold_qty,
  COALESCE(SUM(m.revenue_net) FILTER (WHERE m.reason IN ('sale','void')), 0)::bigint AS sold_revenue,
  COALESCE(SUM(-m.qty_delta * m.unit_cost) FILTER (WHERE m.reason IN ('sale','void')), 0)::bigint AS sold_cost,
  COALESCE(SUM(-m.qty_delta) FILTER (WHERE m.reason IN ('damage','count_correction','adjustment')), 0)::bigint
    AS written_off_qty,
  COALESCE(SUM(-m.qty_delta * m.unit_cost) FILTER (WHERE m.reason IN ('damage','count_correction','adjustment')), 0)::bigint
    AS written_off_cost
FROM pos.batch_items bi JOIN pos.stock_movements m ON m.batch_item_id = bi.id
GROUP BY bi.store_id, bi.id, bi.batch_id, bi.product_id;

CREATE VIEW pos.daily_sales_summary WITH (security_invoker = true) AS
SELECT store_id, business_date, device_id,
  COUNT(*) FILTER (WHERE status = 'completed')::bigint AS sales_count,
  COUNT(*) FILTER (WHERE status = 'voided')::bigint AS void_count,
  COALESCE(SUM(grand_total) FILTER (WHERE status = 'voided'), 0)::bigint AS void_value,
  COALESCE(SUM(gross_sales) FILTER (WHERE status = 'completed'), 0)::bigint AS gross_sales,
  COALESCE(SUM(discount_total) FILTER (WHERE status = 'completed'), 0)::bigint AS discounts,
  COALESCE(SUM(net_sales) FILTER (WHERE status = 'completed'), 0)::bigint AS net_sales,
  COALESCE(SUM(tax_total) FILTER (WHERE status = 'completed'), 0)::bigint AS tax_total,
  COALESCE(SUM(cost_total) FILTER (WHERE status = 'completed'), 0)::bigint AS cogs,
  COALESCE(SUM(quick_sale_net) FILTER (WHERE status = 'completed'), 0)::bigint AS quick_sale_net,
  COALESCE(SUM(unknown_cost_net) FILTER (WHERE status = 'completed'), 0)::bigint AS unknown_cost_net,
  COUNT(*) FILTER (WHERE status = 'completed' AND customer_id IS NOT NULL)::bigint AS customer_orders
FROM pos.orders GROUP BY store_id, business_date, device_id;

CREATE VIEW pos.daily_payment_summary WITH (security_invoker = true) AS
SELECT o.store_id, o.business_date, o.device_id, p.method,
  SUM(p.amount)::bigint AS amount, COUNT(*)::bigint AS txn_count
FROM pos.order_payments p JOIN pos.orders o ON o.id = p.order_id AND o.status = 'completed'
GROUP BY o.store_id, o.business_date, o.device_id, p.method;

CREATE VIEW pos.customer_stats WITH (security_invoker = true) AS
SELECT o.store_id, o.customer_id, COUNT(*)::bigint AS visit_count,
  SUM(o.grand_total)::bigint AS total_spend, SUM(o.net_sales)::bigint AS total_net,
  MIN(o.business_date) AS first_visit_date, MAX(o.business_date) AS last_visit_date
FROM pos.orders o WHERE o.status = 'completed' AND o.customer_id IS NOT NULL
GROUP BY o.store_id, o.customer_id;

GRANT SELECT ON pos.stock_levels, pos.batch_item_stock, pos.daily_sales_summary, pos.daily_payment_summary,
  pos.customer_stats TO pos_sync;

-- ═════════ API roles: nothing in pos is reachable directly ═════════
-- Devices only get EXECUTE on the sync RPCs in public (Block 3.2). Supabase's default grants on new objects
-- are revoked here and for the future.
REVOKE ALL ON ALL TABLES IN SCHEMA pos FROM anon, authenticated, PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA pos FROM anon, authenticated, PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA pos FROM anon, authenticated, PUBLIC;
GRANT EXECUTE ON FUNCTION pos.claims(), pos.claim_store_id(), pos.claim_device_id(), pos.system_device(),
  pos.ts(timestamptz), pos.is_json(text) TO pos_sync;
ALTER DEFAULT PRIVILEGES IN SCHEMA pos REVOKE ALL ON TABLES FROM anon, authenticated, PUBLIC;
ALTER DEFAULT PRIVILEGES IN SCHEMA pos REVOKE ALL ON SEQUENCES FROM anon, authenticated, PUBLIC;
ALTER DEFAULT PRIVILEGES IN SCHEMA pos REVOKE ALL ON FUNCTIONS FROM anon, authenticated, PUBLIC;
