-- Block 3.1: the cloud schema (doc §4.4). Tables, RLS store isolation, the no-UPDATE/DELETE rules for the
-- append-only tables, the cursor stamp, the structural triggers, the API roles' (lack of) privileges, and the
-- derived views. Every test runs in one transaction that is rolled back.
BEGIN;
CREATE EXTENSION IF NOT EXISTS pgtap WITH SCHEMA extensions;
SET search_path = public, extensions;
SELECT plan(42);

-- ─── fixtures (as the migration owner, which bypasses RLS) ───
-- Store A: one device, an Owner, a received batch with a layer, an open cash session, a completed sale.
INSERT INTO pos.stores (id, name, owner_user_id) VALUES
  ('a0000000-0000-0000-0000-000000000001', 'Store A', 'a0000000-0000-0000-0000-0000000000aa'),
  ('b0000000-0000-0000-0000-000000000001', 'Store B', 'b0000000-0000-0000-0000-0000000000bb');
INSERT INTO pos.devices (id, store_id, code, name) VALUES
  ('a0000000-0000-0000-0000-0000000000d1', 'a0000000-0000-0000-0000-000000000001', 'T01', 'Counter 1'),
  ('b0000000-0000-0000-0000-0000000000d1', 'b0000000-0000-0000-0000-000000000001', 'T01', 'Counter 1');

CREATE TEMP TABLE fx AS SELECT
  'a0000000-0000-0000-0000-000000000001'::uuid AS store_a,
  'b0000000-0000-0000-0000-000000000001'::uuid AS store_b,
  'a0000000-0000-0000-0000-0000000000d1'::uuid AS dev_a,
  'b0000000-0000-0000-0000-0000000000d1'::uuid AS dev_b,
  '2026-11-08T03:30:00.000Z'::timestamptz AS t0;

INSERT INTO pos.roles (id, store_id, name, permissions, is_system, created_at, updated_at, updated_by_device)
SELECT 'a0000000-0000-0000-0000-00000000000e', store_a, 'Owner', 9007199254740991, true, t0, t0, dev_a FROM fx;
INSERT INTO pos.staff (id, store_id, display_name, role_id, pin_hash, created_at, updated_at, updated_by_device)
SELECT 'a0000000-0000-0000-0000-00000000005f', store_a, 'Gayan', 'a0000000-0000-0000-0000-00000000000e',
       'argon2id$x', t0, t0, dev_a FROM fx;
INSERT INTO pos.products (id, store_id, name, barcode, price, created_at, updated_at, updated_by_device)
SELECT 'a0000000-0000-0000-0000-0000000000a1'::uuid, store_a, 'Blue bucket', '2000000000017', 45000, t0, t0, dev_a FROM fx
UNION ALL
SELECT 'b0000000-0000-0000-0000-0000000000a1', store_b, 'Red bucket', '2000000000017', 50000, t0, t0, dev_b FROM fx;
INSERT INTO pos.batches (id, store_id, code, name, purchased_at, received_at, received_business_date,
                         allocation_method, status, created_at, updated_at, updated_by_device)
SELECT 'a0000000-0000-0000-0000-0000000000b1'::uuid, store_a, 'B-001', 'Shoes', t0, t0, '2026-11-08'::date, 'manual', 'received',
       t0, t0, dev_a FROM fx
UNION ALL
SELECT 'a0000000-0000-0000-0000-0000000000b2', store_a, 'B-002', 'Draft', t0, NULL, NULL, 'manual', 'draft',
       t0, t0, dev_a FROM fx;
INSERT INTO pos.batch_items (id, store_id, batch_id, product_id, qty_received, unit_cost, created_at, updated_at,
                             updated_by_device)
SELECT 'a0000000-0000-0000-0000-0000000000c1'::uuid, store_a, 'a0000000-0000-0000-0000-0000000000b1'::uuid,
       'a0000000-0000-0000-0000-0000000000a1'::uuid, 10, 20000, t0, t0, dev_a FROM fx
UNION ALL
SELECT 'a0000000-0000-0000-0000-0000000000c2', store_a, 'a0000000-0000-0000-0000-0000000000b2',
       'a0000000-0000-0000-0000-0000000000a1', 5, 21000, t0, t0, dev_a FROM fx;
INSERT INTO pos.cash_sessions (id, store_id, device_id, business_date, opened_by, opened_at, created_at, updated_at,
                               updated_by_device)
SELECT 'a0000000-0000-0000-0000-0000000000e1', store_a, dev_a, '2026-11-08', 'a0000000-0000-0000-0000-00000000005f',
       t0, t0, t0, dev_a FROM fx;
INSERT INTO pos.orders (id, store_id, device_id, cash_session_id, receipt_no, checkout_token, status, staff_id,
                        line_count, item_count, gross_sales, net_sales, grand_total, paid_total, cost_total,
                        business_date, completed_at, created_at, updated_at, updated_by_device)
SELECT 'a0000000-0000-0000-0000-0000000000f1', store_a, dev_a, 'a0000000-0000-0000-0000-0000000000e1',
       'T01-20261108-0001', 'tok-1', 'completed', 'a0000000-0000-0000-0000-00000000005f',
       1, 2, 90000, 90000, 90000, 90000, 40000, '2026-11-08', t0, t0, t0, dev_a FROM fx;
INSERT INTO pos.order_items (id, store_id, device_id, order_id, line_no, product_id, name, list_price, unit_price, qty,
                             line_gross, line_total, line_net, cost_total, created_at)
SELECT 'a0000000-0000-0000-0000-000000000011', store_a, dev_a, 'a0000000-0000-0000-0000-0000000000f1', 1,
       'a0000000-0000-0000-0000-0000000000a1', 'Blue bucket', 45000, 45000, 2, 90000, 90000, 90000, 40000, t0 FROM fx;
INSERT INTO pos.order_payments (id, store_id, device_id, order_id, method, amount, tendered, created_at)
SELECT 'a0000000-0000-0000-0000-000000000021', store_a, dev_a, 'a0000000-0000-0000-0000-0000000000f1', 'cash',
       90000, 100000, t0 FROM fx;
INSERT INTO pos.stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, unit_cost,
                                 business_date, created_at)
SELECT 'a0000000-0000-0000-0000-000000000031', store_a, dev_a, 'a0000000-0000-0000-0000-0000000000a1',
       'a0000000-0000-0000-0000-0000000000c1', 10, 'batch_intake', 20000, '2026-11-08', t0 FROM fx;
INSERT INTO pos.stock_movements (id, store_id, device_id, product_id, batch_item_id, qty_delta, reason, order_item_id,
                                 unit_cost, revenue_net, business_date, created_at)
SELECT 'a0000000-0000-0000-0000-000000000032', store_a, dev_a, 'a0000000-0000-0000-0000-0000000000a1',
       'a0000000-0000-0000-0000-0000000000c1', -2, 'sale', 'a0000000-0000-0000-0000-000000000011', 20000, 90000,
       '2026-11-08', t0 FROM fx;

-- ─── structure ───
SELECT has_schema('pos', 'the business schema exists');
SELECT is((SELECT count(*)::int FROM pos.synced_tables), 18, 'eighteen synced tables, as on the terminal');
SELECT is(
  (SELECT count(*)::int FROM pos.synced_tables st
   JOIN information_schema.tables it ON it.table_schema = 'pos' AND it.table_name = st.name),
  18, 'every synced table exists in pos');
SELECT is(
  (SELECT count(*)::int FROM pos.synced_tables st
   JOIN information_schema.columns c ON c.table_schema = 'pos' AND c.table_name = st.name
     AND c.column_name = 'server_seq' AND c.is_nullable = 'NO'),
  18, 'every synced table carries server_seq NOT NULL');
SELECT is(
  (SELECT count(*)::int FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
   WHERE n.nspname = 'pos' AND c.relkind = 'r' AND NOT c.relrowsecurity),
  0, 'RLS is enabled on every pos table');
SELECT has_table('pos', 'processed_ops', 'the idempotency ledger exists');
SELECT has_table('pos', 'devices', 'the devices table exists');
SELECT col_is_pk('pos', 'processed_ops', 'op_id', 'processed_ops is keyed by op_id');
SELECT col_type_is('pos', 'audit_logs', 'metadata', 'text', 'audit metadata stays text: it feeds the hash chain');

-- ─── policies ───
SELECT is(
  (SELECT count(*)::int FROM pg_policies
   WHERE schemaname = 'pos'
     AND tablename IN ('audit_logs','stock_movements','order_items','order_payments','cash_movements')
     AND cmd IN ('UPDATE','DELETE','ALL')),
  0, 'append-only tables have no UPDATE or DELETE policy');
SELECT is(
  (SELECT count(*)::int FROM pg_policies WHERE schemaname = 'pos' AND cmd IN ('DELETE','ALL')),
  0, 'no table has a DELETE policy');
SELECT ok(
  NOT has_table_privilege('pos_sync', 'pos.order_items', 'UPDATE')
  AND NOT has_table_privilege('pos_sync', 'pos.audit_logs', 'UPDATE')
  AND NOT has_table_privilege('pos_sync', 'pos.stock_movements', 'DELETE'),
  'the sync role cannot update or delete append-only rows');

-- ─── API roles see nothing directly ───
SELECT ok(NOT has_schema_privilege('authenticated', 'pos', 'USAGE'), 'authenticated has no usage on pos');
SELECT ok(NOT has_schema_privilege('anon', 'pos', 'USAGE'), 'anon has no usage on pos');
SELECT is(
  (SELECT count(*)::int FROM information_schema.role_table_grants
   WHERE table_schema = 'pos' AND grantee IN ('anon','authenticated','PUBLIC')),
  0, 'no table privileges for the API roles');

-- ─── cursor stamp ───
SELECT ok((SELECT server_seq FROM pos.order_items WHERE id = 'a0000000-0000-0000-0000-000000000011')
        > (SELECT server_seq FROM pos.orders WHERE id = 'a0000000-0000-0000-0000-0000000000f1'),
  'server_seq increases with insert order');
CREATE TEMP TABLE seq_before AS
  SELECT server_seq FROM pos.products WHERE id = 'a0000000-0000-0000-0000-0000000000a1';
UPDATE pos.products SET price = 46000 WHERE id = 'a0000000-0000-0000-0000-0000000000a1';
SELECT ok((SELECT server_seq FROM pos.products WHERE id = 'a0000000-0000-0000-0000-0000000000a1')
        > (SELECT server_seq FROM seq_before), 'an update takes a new server_seq');
SELECT ok((SELECT server_received_at IS NOT NULL FROM pos.products WHERE id = 'a0000000-0000-0000-0000-0000000000a1'),
  'server_received_at is stamped');

-- ─── wire format ───
SELECT is(pos.ts('2026-11-08T09:30:00.123Z'), '2026-11-08T09:30:00.123Z', 'ts() renders the terminal format');
SELECT is(pos.ts('2026-11-08T15:00:00+05:30'), '2026-11-08T09:30:00.000Z', 'ts() renders UTC with milliseconds');

-- ─── structural triggers (they hold for every writer, the owner included) ───
SELECT throws_ok($$UPDATE pos.order_items SET name = 'x' WHERE id = 'a0000000-0000-0000-0000-000000000011'$$,
  '23514', 'order_items are append-only', 'order_items cannot be updated');
SELECT throws_ok($$DELETE FROM pos.stock_movements WHERE id = 'a0000000-0000-0000-0000-000000000031'$$,
  '23514', 'stock_movements are append-only', 'stock_movements cannot be deleted');
SELECT throws_ok($$DELETE FROM pos.orders WHERE id = 'a0000000-0000-0000-0000-0000000000f1'$$,
  '23514', 'orders are append-only', 'orders cannot be deleted');
SELECT throws_ok($$UPDATE pos.orders SET grand_total = 1, paid_total = 1 WHERE id = 'a0000000-0000-0000-0000-0000000000f1'$$,
  '23514', 'orders: illegal state transition or mutation of a sold order', 'a sold order cannot be re-priced');
SELECT throws_ok(
  $$UPDATE pos.orders SET status = 'voided', voided_at = now(), voided_by = 'a0000000-0000-0000-0000-00000000005f',
      void_reason = 'WRONG_ITEM', updated_by_device = 'b0000000-0000-0000-0000-0000000000d1'
    WHERE id = 'a0000000-0000-0000-0000-0000000000f1'$$,
  '23514', 'orders: illegal state transition or mutation of a sold order', 'only the selling device can void');
SELECT lives_ok(
  $$UPDATE pos.orders SET status = 'voided', voided_at = now(), voided_by = 'a0000000-0000-0000-0000-00000000005f',
      void_reason = 'WRONG_ITEM', updated_by_device = 'a0000000-0000-0000-0000-0000000000d1'
    WHERE id = 'a0000000-0000-0000-0000-0000000000f1'$$,
  'completed → voided by the selling device is allowed');
SELECT throws_ok($$UPDATE pos.orders SET status = 'completed', voided_at = NULL, voided_by = NULL, void_reason = NULL
                   WHERE id = 'a0000000-0000-0000-0000-0000000000f1'$$,
  '23514', 'orders: illegal state transition or mutation of a sold order', 'a void cannot be undone');
SELECT throws_ok($$UPDATE pos.batch_items SET unit_cost = 1 WHERE id = 'a0000000-0000-0000-0000-0000000000c1'$$,
  '23514', 'batch item cost/qty locked after receive', 'a received layer''s cost is locked');
SELECT lives_ok($$UPDATE pos.batch_items SET unit_cost = 1 WHERE id = 'a0000000-0000-0000-0000-0000000000c2'$$,
  'a draft layer''s cost can change');
UPDATE pos.cash_sessions SET status = 'closed', closed_at = now() WHERE id = 'a0000000-0000-0000-0000-0000000000e1';
SELECT throws_ok($$UPDATE pos.cash_sessions SET status = 'open' WHERE id = 'a0000000-0000-0000-0000-0000000000e1'$$,
  '23514', 'a closed cash session cannot be reopened', 'a closed session never reopens');
SELECT throws_ok($$INSERT INTO pos.store_settings (key, store_id, value, created_at, updated_at, updated_by_device)
                   VALUES ('x', 'a0000000-0000-0000-0000-000000000001', '{not json', now(), now(),
                           'a0000000-0000-0000-0000-0000000000d1')$$,
  '23514', NULL, 'JSON text columns must hold valid JSON');

-- ─── derived views ───
SELECT is((SELECT void_count FROM pos.daily_sales_summary WHERE store_id = (SELECT store_a FROM fx)), 1::bigint,
  'the voided sale counts as a void');
SELECT is((SELECT sales_count FROM pos.daily_sales_summary WHERE store_id = (SELECT store_a FROM fx)), 0::bigint,
  'and not as a sale');
SELECT is((SELECT on_hand FROM pos.stock_levels WHERE product_id = 'a0000000-0000-0000-0000-0000000000a1'), 8::bigint,
  'stock_levels = Σ movements');

-- ─── RLS: a device of store A sees and writes store A only ───
GRANT USAGE ON SCHEMA extensions TO pos_sync;   -- test-only (rolled back): pgTAP's own functions
SET LOCAL ROLE pos_sync;
SELECT set_config('request.jwt.claims',
  '{"role":"authenticated","app_metadata":{"role":"device","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1"}}',
  true);
SELECT is((SELECT count(*)::int FROM pos.products), 1, 'a device sees only its own store''s products');
SELECT is((SELECT name FROM pos.products), 'Blue bucket', 'namely store A''s');
SELECT is((SELECT count(*)::int FROM pos.devices), 1, 'and only its own store''s devices');
SELECT throws_ok(
  $$INSERT INTO pos.categories (id, store_id, name, created_at, updated_at, updated_by_device)
    VALUES ('b0000000-0000-0000-0000-0000000000c9', 'b0000000-0000-0000-0000-000000000001', 'x', now(), now(),
            'a0000000-0000-0000-0000-0000000000d1')$$,
  '42501', NULL, 'it cannot write into another store');
WITH u AS (UPDATE pos.products SET price = 1 WHERE id = 'b0000000-0000-0000-0000-0000000000a1' RETURNING 1)
SELECT is((SELECT count(*)::int FROM u), 0, 'it cannot update another store''s rows');
SELECT throws_ok($$UPDATE pos.order_payments SET amount = 1$$, '42501', NULL,
  'the sync role has no UPDATE privilege on payments');
-- A signed-in user who is not a device identity (e.g. the owner's account) sees nothing.
SELECT set_config('request.jwt.claims',
  '{"role":"authenticated","app_metadata":{"store_id":"a0000000-0000-0000-0000-000000000001"}}', true);
SELECT is((SELECT count(*)::int FROM pos.products), 0, 'a non-device identity sees no rows');
SELECT set_config('request.jwt.claims', '', true);
SELECT is((SELECT count(*)::int FROM pos.orders), 0, 'no claims, no rows');
RESET ROLE;

SELECT * FROM finish();
ROLLBACK;
