-- Exchanges (decision #26) through the sync RPCs: the exchange order, its payments, its returns and the return
-- stock movements sync like any sale; a terminal that predates exchanges keeps syncing; the rows stay immutable;
-- other counters pull the returns with their parents; sales are reported net of returns.
BEGIN;
CREATE EXTENSION IF NOT EXISTS pgtap WITH SCHEMA extensions;
SET search_path = public, extensions;
SELECT plan(13);

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

INSERT INTO pos.devices (id, store_id, code, name) VALUES
  ('a0000000-0000-0000-0000-0000000000d2', 'a0000000-0000-0000-0000-000000000001', 'T02', 'Counter 2');

CREATE FUNCTION pg_temp.as_device(store text, dev text) RETURNS void LANGUAGE sql AS $$
  SELECT set_config('request.jwt.claims', jsonb_build_object('role', 'authenticated',
    'app_metadata', jsonb_build_object('role', 'device', 'store_id', store, 'device_id', dev))::text, true);
$$;
CREATE FUNCTION pg_temp.op(tbl text, r jsonb) RETURNS jsonb LANGUAGE sql AS $$
  SELECT jsonb_build_object('op_id', gen_random_uuid(), 'table', tbl, 'op', 'upsert', 'row', r);
$$;
CREATE FUNCTION pg_temp.statuses(res jsonb) RETURNS text LANGUAGE sql AS $$
  SELECT string_agg(e ->> 'status', ',' ORDER BY ord) FROM jsonb_array_elements(res -> 'results') WITH ORDINALITY x(e, ord);
$$;
CREATE FUNCTION pg_temp.reason(res jsonb) RETURNS text LANGUAGE sql AS $$
  SELECT res -> 'results' -> 0 ->> 'reason';
$$;

-- Wire rows, as a terminal's outbox carries them (the stored sale is the template).
CREATE TEMP TABLE w (k text PRIMARY KEY, tbl text, r jsonb);
INSERT INTO w
SELECT 'order', 'orders', to_jsonb(o) - 'server_seq' - 'server_received_at' FROM pos.orders o
  WHERE o.id = 'a0000000-0000-0000-0000-0000000000f1'
UNION ALL
SELECT 'line', 'order_items', to_jsonb(i) - 'server_seq' - 'server_received_at' FROM pos.order_items i
  WHERE i.id = 'a0000000-0000-0000-0000-000000000011'
UNION ALL
SELECT 'payment', 'order_payments', to_jsonb(p) - 'server_seq' - 'server_received_at' FROM pos.order_payments p
  WHERE p.id = 'a0000000-0000-0000-0000-000000000021';
GRANT ALL ON w TO authenticated;

-- The exchange: 45,000 comes back (one of two buckets), a 90,000 replacement is sold, 45,000 is paid.
CREATE TEMP TABLE ex (k text PRIMARY KEY, tbl text, r jsonb);
INSERT INTO ex VALUES
 ('order', 'orders', (SELECT r FROM w WHERE k = 'order') || '{"id":"a0000000-0000-0000-0000-0000000000f2","receipt_no":"T01-20261110-0001","checkout_token":"tok-ex","business_date":"2026-11-10","exchange_credit":45000,"returned_items":1,"returned_gross":45000,"returned_discount":0,"returned_net":45000,"returned_tax":0,"returned_cost":20000,"paid_total":90000,"late_approved_by":null}'),
 ('line', 'order_items', (SELECT r FROM w WHERE k = 'line') || '{"id":"a0000000-0000-0000-0000-000000000012","order_id":"a0000000-0000-0000-0000-0000000000f2"}'),
 ('payment', 'order_payments', (SELECT r FROM w WHERE k = 'payment') || '{"id":"a0000000-0000-0000-0000-000000000022","order_id":"a0000000-0000-0000-0000-0000000000f2","amount":45000,"tendered":50000}'),
 ('return', 'order_returns', '{"id":"a0000000-0000-0000-0000-000000000051","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","order_id":"a0000000-0000-0000-0000-0000000000f2","source_item_id":"a0000000-0000-0000-0000-000000000011","qty":1,"line_gross":45000,"line_discount":0,"line_total":45000,"line_net":45000,"tax_amount":0,"cost_total":20000,"created_at":"2026-11-10T04:00:00.000Z"}'),
 ('restock', 'stock_movements', '{"id":"a0000000-0000-0000-0000-000000000052","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","product_id":"a0000000-0000-0000-0000-0000000000a1","batch_item_id":"a0000000-0000-0000-0000-0000000000c1","qty_delta":1,"reason":"return","ref_type":"order","ref_id":"a0000000-0000-0000-0000-0000000000f2","order_item_id":"a0000000-0000-0000-0000-000000000011","unit_cost":20000,"revenue_net":-45000,"business_date":"2026-11-10","created_at":"2026-11-10T04:00:00.000Z"}');
GRANT ALL ON ex TO authenticated;

-- ─── T01 pushes the exchange ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1',
    (SELECT jsonb_agg(pg_temp.op(tbl, r) ORDER BY array_position(ARRAY['order','line','payment','return','restock'], k)) FROM ex))),
  'ok,ok,ok,ok,ok', 'an exchange order, its payment, its returns and the restock all sync');
RESET ROLE;
SELECT is((SELECT exchange_credit FROM pos.orders WHERE id = 'a0000000-0000-0000-0000-0000000000f2'), 45000::bigint,
  'the exchange credit is stored on the order');
SELECT is((SELECT qty FROM pos.order_returns WHERE order_id = 'a0000000-0000-0000-0000-0000000000f2'), 1::bigint,
  'and the returned units in order_returns');

-- ─── idempotency and immutability ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1',
    jsonb_build_array(pg_temp.op('order_returns', (SELECT r FROM ex WHERE k = 'return') || '{"qty":9,"line_gross":1,"line_total":1}')))),
  'ok', 'a returns row sent again is a no-op');
RESET ROLE;
SELECT is((SELECT qty FROM pos.order_returns WHERE id = 'a0000000-0000-0000-0000-000000000051'), 1::bigint,
  'and the stored return is untouched');
SELECT throws_ok($$UPDATE pos.order_returns SET qty = 2$$, '23514', 'order_returns are append-only',
  'order_returns cannot be edited');
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('orders',
  (SELECT r FROM ex WHERE k = 'order') || '{"status":"voided","voided_at":"2026-11-10T05:00:00.000Z","voided_by":"a0000000-0000-0000-0000-00000000005f","void_reason":"WRONG_ITEM","updated_at":"2026-11-10T05:00:00.000Z","exchange_credit":0,"version":2}')))) LIKE 'ORDER_IMMUTABLE%',
  true, 'a void cannot erase an exchange''s credit');
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
SELECT is(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d2',
    jsonb_build_array(pg_temp.op('order_returns', (SELECT r FROM ex WHERE k = 'return') || '{"id":"a0000000-0000-0000-0000-000000000059"}')))),
  'DEVICE_MISMATCH', 'a counter cannot push another counter''s returns');

-- ─── a counter that predates exchanges keeps syncing ───
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('orders',
  (SELECT r FROM w WHERE k = 'order') - 'exchange_credit' - 'returned_items' - 'returned_gross' - 'returned_discount'
    - 'returned_net' - 'returned_tax' - 'returned_cost' - 'late_approved_by'
    || '{"id":"a0000000-0000-0000-0000-0000000000f3","receipt_no":"T01-20261110-0002","checkout_token":"tok-old","business_date":"2026-11-10"}')))),
  'ok', 'an order pushed without the exchange columns is accepted');
RESET ROLE;
SELECT is((SELECT exchange_credit FROM pos.orders WHERE id = 'a0000000-0000-0000-0000-0000000000f3'), 0::bigint,
  'and carries no exchange credit');

-- ─── another counter pulls the returns together with their parents ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
CREATE TEMP TABLE pulled AS SELECT sync_pull(0, 500) AS p;
GRANT ALL ON pulled TO authenticated;
SELECT ok(EXISTS (SELECT 1 FROM pulled, jsonb_array_elements(p -> 'rows') e
                  WHERE e ->> 'table' = 'order_returns' AND e -> 'row' ->> 'qty' = '1'),
  'sync_pull ships the returns row');
SELECT ok(EXISTS (SELECT 1 FROM pulled, jsonb_array_elements(p -> 'rows') e
                  WHERE e ->> 'table' = 'orders' AND (e -> 'row' ->> 'exchange_credit')::bigint = 45000),
  'with the exchange order carrying its credit');
RESET ROLE;

-- ─── reports are net of returns ───
SELECT is((SELECT net_sales FROM pos.daily_sales_summary WHERE business_date = '2026-11-10' AND device_id = 'a0000000-0000-0000-0000-0000000000d1'
           AND sales_count = 2), 135000::bigint,
  'sales on the exchange day are net of the returned goods (90,000 + 90,000 − 45,000)');

SELECT * FROM finish();
ROLLBACK;
