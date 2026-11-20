-- Block 3.2: sync_push and sync_pull (doc §6). Called the way PostgREST calls them: as `authenticated`, with
-- the device's JWT claims in request.jwt.claims. Rows are written in the terminal's wire format (SQLite
-- json_object: numbers, 0/1 flags, `…T…Z` millisecond timestamps).
BEGIN;
CREATE EXTENSION IF NOT EXISTS pgtap WITH SCHEMA extensions;
SET search_path = public, extensions;
SELECT plan(67);

-- ─── tenancy fixtures: store A with counters T01, T02, T03; store B with T01 ───
INSERT INTO pos.stores (id, name, owner_user_id) VALUES
  ('a0000000-0000-0000-0000-000000000001', 'Store A', 'a0000000-0000-0000-0000-0000000000aa'),
  ('b0000000-0000-0000-0000-000000000001', 'Store B', 'b0000000-0000-0000-0000-0000000000bb');
INSERT INTO pos.devices (id, store_id, code, name) VALUES
  ('a0000000-0000-0000-0000-0000000000d1', 'a0000000-0000-0000-0000-000000000001', 'T01', 'Counter 1'),
  ('a0000000-0000-0000-0000-0000000000d2', 'a0000000-0000-0000-0000-000000000001', 'T02', 'Counter 2'),
  ('a0000000-0000-0000-0000-0000000000d3', 'a0000000-0000-0000-0000-000000000001', 'T03', 'Counter 3'),
  ('b0000000-0000-0000-0000-0000000000d1', 'b0000000-0000-0000-0000-000000000001', 'T01', 'Counter 1');

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

-- The rows T01 writes at first run and during a first sale, exactly as its outbox carries them.
CREATE TEMP TABLE w (k text PRIMARY KEY, tbl text, r jsonb);
INSERT INTO w VALUES
 ('owner_role', 'roles', '{"id":"a0000000-0000-0000-0000-00000000000e","store_id":"a0000000-0000-0000-0000-000000000001","name":"Owner","permissions":9007199254740991,"max_discount_bp":10000,"max_override_bp":10000,"is_system":1,"created_at":"2026-11-08T03:30:00.000Z","updated_at":"2026-11-08T03:30:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":1}'),
 ('cashier_role', 'roles', '{"id":"a0000000-0000-0000-0000-00000000000c","store_id":"a0000000-0000-0000-0000-000000000001","name":"Cashier","permissions":3,"max_discount_bp":500,"max_override_bp":0,"is_system":1,"created_at":"2026-11-08T03:30:00.000Z","updated_at":"2026-11-08T03:30:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":1}'),
 ('owner', 'staff', '{"id":"a0000000-0000-0000-0000-00000000005f","store_id":"a0000000-0000-0000-0000-000000000001","display_name":"Gayan","role_id":"a0000000-0000-0000-0000-00000000000e","pin_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","grant_mask":0,"deny_mask":0,"max_discount_bp":null,"ui_language":"si","is_active":1,"created_at":"2026-11-08T03:30:00.000Z","updated_at":"2026-11-08T03:30:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":1}'),
 ('setting', 'store_settings', '{"key":"receipt.footer","store_id":"a0000000-0000-0000-0000-000000000001","value":"\"Exchange within 7 days. Item for item only; no cash refunds. Thank you, come again!\"","created_at":"2026-11-08T03:30:00.000Z","updated_at":"2026-11-08T03:30:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":1}'),
 ('product', 'products', '{"id":"a0000000-0000-0000-0000-0000000000a1","store_id":"a0000000-0000-0000-0000-000000000001","kind":"standard","name":"Blue bucket","name_si":"නිල් බාල්දිය","sku":"100001","barcode":"2000000000017","category_id":null,"default_cost":20000,"price":45000,"min_price":null,"track_stock":1,"allow_negative_stock":1,"tax_rate_bp":0,"attributes":"{\"size\":\"10L\",  \"colour\":\"blue\"}","is_active":1,"barcode_conflict":0,"created_at":"2026-11-08T03:31:00.000Z","updated_at":"2026-11-08T03:31:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":1}'),
 ('batch', 'batches', '{"id":"a0000000-0000-0000-0000-0000000000b1","store_id":"a0000000-0000-0000-0000-000000000001","code":"B-001","name":"Buckets","supplier_name":null,"supplier_ref":null,"purchased_at":"2026-11-08T03:32:00.000Z","received_at":"2026-11-08T03:33:00.000Z","received_business_date":"2026-11-07","total_cost":200000,"extra_costs":0,"allocation_method":"manual","allocation_variance":0,"status":"received","is_system":0,"received_by":"a0000000-0000-0000-0000-00000000005f","notes":null,"created_at":"2026-11-08T03:32:00.000Z","updated_at":"2026-11-08T03:33:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":2}'),
 ('layer', 'batch_items', '{"id":"a0000000-0000-0000-0000-0000000000c1","store_id":"a0000000-0000-0000-0000-000000000001","batch_id":"a0000000-0000-0000-0000-0000000000b1","product_id":"a0000000-0000-0000-0000-0000000000a1","qty_received":10,"unit_cost":20000,"retail_price_at_intake":45000,"manual_unit_cost":20000,"added_after_receipt":0,"note":null,"created_at":"2026-11-08T03:32:00.000Z","updated_at":"2026-11-08T03:32:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":1}'),
 ('intake', 'stock_movements', '{"id":"a0000000-0000-0000-0000-000000000031","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","product_id":"a0000000-0000-0000-0000-0000000000a1","batch_item_id":"a0000000-0000-0000-0000-0000000000c1","qty_delta":10,"reason":"batch_intake","ref_type":"batch","ref_id":"a0000000-0000-0000-0000-0000000000b1","order_item_id":null,"unit_cost":20000,"revenue_net":null,"business_date":"2026-11-07","staff_id":"a0000000-0000-0000-0000-00000000005f","approved_by":null,"note":null,"created_at":"2026-11-08T03:33:00.000Z"}'),
 ('session', 'cash_sessions', '{"id":"a0000000-0000-0000-0000-0000000000e1","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","business_date":"2026-11-08","status":"open","opened_by":"a0000000-0000-0000-0000-00000000005f","opened_at":"2026-11-08T03:34:00.000Z","opening_float":500000,"closed_by":null,"closed_at":null,"expected_cash":null,"counted_cash":null,"variance":null,"denominations":null,"close_note":null,"created_at":"2026-11-08T03:34:00.000Z","updated_at":"2026-11-08T03:34:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":1}'),
 ('order', 'orders', '{"id":"a0000000-0000-0000-0000-0000000000f1","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","cash_session_id":"a0000000-0000-0000-0000-0000000000e1","receipt_no":"T01-20261108-0001","checkout_token":"0196f0c2-7d1a-7c00-8000-000000000001","type":"sale","status":"completed","staff_id":"a0000000-0000-0000-0000-00000000005f","approved_by":null,"customer_id":null,"line_count":1,"item_count":2,"gross_sales":90000,"discount_total":0,"net_sales":90000,"tax_total":0,"rounding":0,"grand_total":90000,"paid_total":90000,"change_due":10000,"cost_total":40000,"quick_sale_net":0,"unknown_cost_net":0,"business_date":"2026-11-08","completed_at":"2026-11-08T04:10:00.123Z","voided_at":null,"voided_by":null,"void_approved_by":null,"void_reason":null,"void_note":null,"note":null,"created_at":"2026-11-08T04:10:00.123Z","updated_at":"2026-11-08T04:10:00.123Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","version":1}'),
 ('line', 'order_items', '{"id":"a0000000-0000-0000-0000-000000000011","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","order_id":"a0000000-0000-0000-0000-0000000000f1","line_no":1,"product_id":"a0000000-0000-0000-0000-0000000000a1","is_custom":0,"custom_key":null,"batch_id":"a0000000-0000-0000-0000-0000000000b1","name":"Blue bucket","barcode":"2000000000017","list_price":45000,"unit_price":45000,"qty":2,"line_gross":90000,"line_discount":0,"order_discount_share":0,"tax_rate_bp":0,"tax_amount":0,"line_total":90000,"line_net":90000,"cost_total":40000,"cost_known":1,"price_overridden":0,"override_reason":null,"approved_by":null,"created_at":"2026-11-08T04:10:00.123Z"}'),
 ('payment', 'order_payments', '{"id":"a0000000-0000-0000-0000-000000000021","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","order_id":"a0000000-0000-0000-0000-0000000000f1","method":"cash","amount":90000,"tendered":100000,"reference":null,"created_at":"2026-11-08T04:10:00.123Z"}'),
 ('slice', 'stock_movements', '{"id":"a0000000-0000-0000-0000-000000000032","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","product_id":"a0000000-0000-0000-0000-0000000000a1","batch_item_id":"a0000000-0000-0000-0000-0000000000c1","qty_delta":-2,"reason":"sale","ref_type":"order","ref_id":"a0000000-0000-0000-0000-0000000000f1","order_item_id":"a0000000-0000-0000-0000-000000000011","unit_cost":20000,"revenue_net":90000,"business_date":"2026-11-08","staff_id":"a0000000-0000-0000-0000-00000000005f","approved_by":null,"note":null,"created_at":"2026-11-08T04:10:00.123Z"}'),
 ('audit1', 'audit_logs', '{"id":"a0000000-0000-0000-0000-000000000041","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","device_seq":1,"staff_id":"a0000000-0000-0000-0000-00000000005f","approved_by":null,"action":"STORE_INITIALIZED","entity_type":"store","entity_id":"a0000000-0000-0000-0000-000000000001","metadata":"{\"device_code\":\"T01\",\"store_name\":\"Store A\"}","business_date":"2026-11-07","prev_hash":"0000000000000000000000000000000000000000000000000000000000000000","hash":"1111111111111111111111111111111111111111111111111111111111111111","created_at":"2026-11-08T03:30:00.000Z"}'),
 ('audit2', 'audit_logs', '{"id":"a0000000-0000-0000-0000-000000000042","store_id":"a0000000-0000-0000-0000-000000000001","device_id":"a0000000-0000-0000-0000-0000000000d1","device_seq":2,"staff_id":"a0000000-0000-0000-0000-00000000005f","approved_by":null,"action":"ORDER_COMPLETED","entity_type":"order","entity_id":"a0000000-0000-0000-0000-0000000000f1","metadata":"{\"receipt_no\":\"T01-20261108-0001\"}","business_date":"2026-11-08","prev_hash":"1111111111111111111111111111111111111111111111111111111111111111","hash":"2222222222222222222222222222222222222222222222222222222222222222","created_at":"2026-11-08T04:10:00.123Z"}');

CREATE TEMP TABLE batch1 AS
  SELECT jsonb_agg(pg_temp.op(tbl, r) ORDER BY ord) AS ops
  FROM (SELECT tbl, r, array_position(ARRAY['owner_role','cashier_role','owner','setting','product','batch','layer',
          'intake','session','order','line','payment','slice','audit1','audit2'], k) AS ord FROM w) x;
CREATE TEMP TABLE res (k text PRIMARY KEY, v jsonb);
GRANT ALL ON w, batch1, res TO authenticated;

-- ─── who may call ───
SET LOCAL ROLE authenticated;
SELECT set_config('request.jwt.claims', '{"role":"authenticated","app_metadata":{"store_id":"a0000000-0000-0000-0000-000000000001"}}', true);
SELECT throws_ok($$SELECT sync_push('a0000000-0000-0000-0000-0000000000d1', '[]')$$, '42501', 'NOT_A_DEVICE',
  'a non-device identity cannot push');
SELECT throws_ok($$SELECT sync_pull(0)$$, '42501', 'NOT_A_DEVICE', 'or pull');
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT throws_ok($$SELECT sync_push('a0000000-0000-0000-0000-0000000000d2', '[]')$$, '42501', 'DEVICE_MISMATCH',
  'a device pushes only as itself');
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d9');
SELECT throws_ok($$SELECT sync_health()$$, '42501', 'DEVICE_NOT_REGISTERED', 'an unregistered device id is refused');

-- ─── T01 pushes its first run and first sale ───
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
INSERT INTO res SELECT 'first', sync_push('a0000000-0000-0000-0000-0000000000d1', (SELECT ops FROM batch1), '0.3.0');
SELECT is(pg_temp.statuses((SELECT v FROM res WHERE k = 'first')), 'ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok',
  'every op of the first batch is applied');
SELECT is((sync_health() ->> 'device_code'), 'T01', 'sync_health answers for the calling device');
SELECT is((sync_health() ->> 'last_audit_seq')::bigint, 2::bigint,
  'and reports its audit chain head as the cloud knows it (restore recovery cross-checks it)');
RESET ROLE;
SELECT ok(EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'advisory' AND pid = pg_backend_pid() AND granted),
  'sync_push holds the per-store advisory lock until commit');
SELECT is((SELECT count(*)::int FROM pos.processed_ops
           WHERE result = 'ok' AND store_id = 'a0000000-0000-0000-0000-000000000001'), 15, 'each op is recorded once');
SELECT is((SELECT app_version FROM pos.devices WHERE code = 'T01' AND store_id = 'a0000000-0000-0000-0000-000000000001'),
  '0.3.0', 'the device''s last seen version is kept');
SELECT is((SELECT last_audit_seq FROM pos.devices WHERE id = 'a0000000-0000-0000-0000-0000000000d1'), 2::bigint,
  'the audit chain advances');
SELECT ok((SELECT chain_broken_at IS NULL FROM pos.devices WHERE id = 'a0000000-0000-0000-0000-0000000000d1'),
  'an unbroken chain is not flagged');
CREATE TEMP TABLE seq_order AS SELECT server_seq FROM pos.orders WHERE id = 'a0000000-0000-0000-0000-0000000000f1';

-- ─── idempotency ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', (SELECT ops FROM batch1))),
  'ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok,ok', 'a replayed batch returns the first results');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1',
            jsonb_build_array(pg_temp.op('order_payments', (SELECT r FROM w WHERE k = 'payment') || '{"amount":1}'))
          )), 'ok', 'an insert-only row sent again under a new op id is a no-op');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1',
            jsonb_build_array(pg_temp.op('orders', (SELECT r FROM w WHERE k = 'order') || '{"grand_total":1,"paid_total":1}'))
          )), 'ok', 'a completed order sent again is a no-op');
RESET ROLE;
SELECT is((SELECT server_seq FROM pos.orders WHERE id = 'a0000000-0000-0000-0000-0000000000f1'),
  (SELECT server_seq FROM seq_order), 'replays do not touch the stored order');
SELECT is((SELECT amount FROM pos.order_payments WHERE id = 'a0000000-0000-0000-0000-000000000021'), 90000::bigint,
  'nor the stored payment');
SELECT is((SELECT grand_total FROM pos.orders WHERE id = 'a0000000-0000-0000-0000-0000000000f1'), 90000::bigint,
  'nor the sold total');

-- ─── orders: sealed only, voided only by the selling device, never re-priced ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('orders',
  (SELECT r FROM w WHERE k = 'order') || '{"id":"a0000000-0000-0000-0000-0000000000f2","receipt_no":"T01-20261108-0002","checkout_token":"t2","status":"pending"}')))),
  'ORDER_NOT_SEALED: pending', 'a pending order is rejected');
SELECT is(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('orders',
  (SELECT r FROM w WHERE k = 'order') || '{"status":"voided","voided_at":"2026-11-08T05:00:00.000Z","voided_by":"a0000000-0000-0000-0000-00000000005f","void_reason":"WRONG_ITEM","updated_at":"2026-11-08T05:00:00.000Z","grand_total":1,"paid_total":1}')))) LIKE 'ORDER_IMMUTABLE%',
  true, 'a void that also changes the sale is rejected');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('orders',
  (SELECT r FROM w WHERE k = 'order') || '{"status":"voided","voided_at":"2026-11-08T05:00:00.000Z","voided_by":"a0000000-0000-0000-0000-00000000005f","void_reason":"WRONG_ITEM","updated_at":"2026-11-08T05:00:00.000Z","version":2}')))),
  'ok', 'the void transition is applied');
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
SELECT is(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(pg_temp.op('order_payments',
  (SELECT r FROM w WHERE k = 'payment') || '{"id":"a0000000-0000-0000-0000-000000000029"}')))),
  'DEVICE_MISMATCH', 'a device cannot push another device''s ledger rows');
SELECT is(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(pg_temp.op('categories',
  '{"id":"a0000000-0000-0000-0000-0000000000c9","store_id":"b0000000-0000-0000-0000-000000000001","name":"x","parent_id":null,"created_at":"2026-11-08T05:00:00.000Z","updated_at":"2026-11-08T05:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2","deleted_at":null,"version":1}')))),
  'STORE_MISMATCH', 'or rows of another store');
RESET ROLE;
SELECT is((SELECT status FROM pos.orders WHERE id = 'a0000000-0000-0000-0000-0000000000f1'), 'voided', 'the order is voided');

-- ─── last-write-wins ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(
  pg_temp.op('products', (SELECT r FROM w WHERE k = 'product') || '{"price":47000,"updated_at":"2026-11-08T06:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2","version":2}'),
  pg_temp.op('products', (SELECT r FROM w WHERE k = 'product') || '{"price":1,"updated_at":"2026-11-08T05:59:59.999Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2","version":2}'))
  )), 'ok,ok', 'a newer and a stale edit are both accepted');
RESET ROLE;
SELECT is((SELECT price FROM pos.products WHERE id = 'a0000000-0000-0000-0000-0000000000a1'), 47000::bigint,
  'the newer edit wins, the stale one is ignored');
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d3');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d3', jsonb_build_array(
  pg_temp.op('products', (SELECT r FROM w WHERE k = 'product') || '{"price":48000,"updated_at":"2026-11-08T06:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d3","version":2}'))
  )), 'ok', 'an edit at the same instant from another device');
RESET ROLE;
SELECT is((SELECT price FROM pos.products WHERE id = 'a0000000-0000-0000-0000-0000000000a1'), 48000::bigint,
  'a timestamp tie is broken by device id');
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d3');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d3', jsonb_build_array(
  pg_temp.op('products', (SELECT r FROM w WHERE k = 'product') || '{"price":48500,"updated_at":"2026-11-08T06:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d3","version":3}'))
  )), 'ok', 'the same device writes the row again within the same millisecond');
RESET ROLE;
SELECT is((SELECT price FROM pos.products WHERE id = 'a0000000-0000-0000-0000-0000000000a1'), 48500::bigint,
  'its later snapshot wins the tie');

-- ─── invariants ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
SELECT is(split_part(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(pg_temp.op('batch_items',
  (SELECT r FROM w WHERE k = 'layer') || '{"unit_cost":1,"updated_at":"2026-11-08T07:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2"}')))), ':', 1),
  'COST_LOCKED', 'a received layer''s cost cannot change');
SELECT is(split_part(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(pg_temp.op('batches',
  (SELECT r FROM w WHERE k = 'batch') || '{"status":"draft","received_at":null,"received_business_date":null,"updated_at":"2026-11-08T07:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2"}')))), ':', 1),
  'BATCH_STATUS_BACKWARDS', 'a batch that has sold cannot be reopened');
SELECT is(split_part(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(pg_temp.op('staff',
  (SELECT r FROM w WHERE k = 'owner') || '{"role_id":"a0000000-0000-0000-0000-00000000000c","updated_at":"2026-11-08T07:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2"}')))), ':', 1),
  'OWNER_INVARIANT', 'the only Owner cannot be demoted');
RESET ROLE;
SELECT is((SELECT role_id FROM pos.staff WHERE id = 'a0000000-0000-0000-0000-00000000005f'),
  'a0000000-0000-0000-0000-00000000000e'::uuid, 'and the rejected change is not stored');
SELECT ok((SELECT server_seq FROM pos.staff WHERE id = 'a0000000-0000-0000-0000-00000000005f')
          > (SELECT max(server_seq) FROM pos.batch_items),
  'the refused row is re-stamped, so the refusing counter pulls the cloud''s version back');
-- Two Owners, each demoted offline by a different counter: the second demotion to arrive is rejected.
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('staff',
  (SELECT r FROM w WHERE k = 'owner') || '{"id":"a0000000-0000-0000-0000-00000000006f","display_name":"Nimal","updated_at":"2026-11-08T08:00:00.000Z"}')))),
  'ok', 'a second Owner is added');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('staff',
  (SELECT r FROM w WHERE k = 'owner') || '{"role_id":"a0000000-0000-0000-0000-00000000000c","updated_at":"2026-11-08T08:01:00.000Z"}')))),
  'ok', 'T01 demotes the first Owner');
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
SELECT is(split_part(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(pg_temp.op('staff',
  (SELECT r FROM w WHERE k = 'owner') || '{"id":"a0000000-0000-0000-0000-00000000006f","display_name":"Nimal","role_id":"a0000000-0000-0000-0000-00000000000c","updated_at":"2026-11-08T08:02:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2"}')))), ':', 1),
  'OWNER_INVARIANT', 'T02''s demotion of the second Owner is rejected');

-- ─── barcode collision: parked, never rejected ───
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(pg_temp.op('products',
  (SELECT r FROM w WHERE k = 'product') || '{"id":"a0000000-0000-0000-0000-0000000000a2","name":"Bucket (T02)","sku":"200001","created_at":"2026-11-08T06:30:00.000Z","updated_at":"2026-11-08T06:30:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2"}')))),
  'ok', 'a product registered with a barcode already in use is accepted');
RESET ROLE;
SELECT is((SELECT row(is_active, barcode_conflict, updated_by_device)::text FROM pos.products
           WHERE id = 'a0000000-0000-0000-0000-0000000000a2'),
  '(f,t,00000000-0000-0000-0000-000000000000)', 'and parked inactive, flagged, stamped by SYSTEM');

-- ─── customers: an erasure wins over a later edit ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(
  pg_temp.op('customers', '{"id":"a0000000-0000-0000-0000-0000000000cc","store_id":"a0000000-0000-0000-0000-000000000001","phone_e164":"+94771234567","display_name":"Kamal","note":null,"first_seen_at":"2026-11-08T04:00:00.000Z","erased_at":null,"created_at":"2026-11-08T04:00:00.000Z","updated_at":"2026-11-08T04:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":1}'),
  pg_temp.op('customers', '{"id":"a0000000-0000-0000-0000-0000000000cc","store_id":"a0000000-0000-0000-0000-000000000001","phone_e164":null,"display_name":null,"note":null,"first_seen_at":"2026-11-08T04:00:00.000Z","erased_at":"2026-11-08T09:00:00.000Z","created_at":"2026-11-08T04:00:00.000Z","updated_at":"2026-11-08T09:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":2}')))),
  'ok,ok', 'a customer is created and erased');
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(
  pg_temp.op('customers', '{"id":"a0000000-0000-0000-0000-0000000000cc","store_id":"a0000000-0000-0000-0000-000000000001","phone_e164":"+94771234567","display_name":"Kamal P.","note":null,"first_seen_at":"2026-11-08T04:00:00.000Z","erased_at":null,"created_at":"2026-11-08T04:00:00.000Z","updated_at":"2026-11-08T10:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2","deleted_at":null,"version":2}')))),
  'ok', 'a later edit from another counter is accepted');
RESET ROLE;
SELECT ok((SELECT erased_at IS NOT NULL AND phone_e164 IS NULL AND display_name IS NULL FROM pos.customers
           WHERE id = 'a0000000-0000-0000-0000-0000000000cc'), 'but does not re-populate the erased customer');
-- The other order: a newer edit is in the cloud, then an older erasure arrives. The erasure still wins.
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d2', jsonb_build_array(
  pg_temp.op('customers', '{"id":"a0000000-0000-0000-0000-0000000000ce","store_id":"a0000000-0000-0000-0000-000000000001","phone_e164":"+94771234568","display_name":"Sunil","note":null,"first_seen_at":"2026-11-08T04:00:00.000Z","erased_at":null,"created_at":"2026-11-08T04:00:00.000Z","updated_at":"2026-11-08T10:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d2","deleted_at":null,"version":2}')))),
  'ok', 'a customer edited late on T02');
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(
  pg_temp.op('customers', '{"id":"a0000000-0000-0000-0000-0000000000ce","store_id":"a0000000-0000-0000-0000-000000000001","phone_e164":null,"display_name":null,"note":null,"first_seen_at":"2026-11-08T04:00:00.000Z","erased_at":"2026-11-08T09:00:00.000Z","created_at":"2026-11-08T04:00:00.000Z","updated_at":"2026-11-08T09:00:00.000Z","updated_by_device":"a0000000-0000-0000-0000-0000000000d1","deleted_at":null,"version":2}')))),
  'ok', 'and erased earlier on T01, arriving after');
RESET ROLE;
SELECT ok((SELECT erased_at IS NOT NULL AND phone_e164 IS NULL AND display_name IS NULL FROM pos.customers
           WHERE id = 'a0000000-0000-0000-0000-0000000000ce'), 'the older erasure still wins over the newer edit');

-- ─── audit chain: a gap is stored but flagged ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('audit_logs',
  (SELECT r FROM w WHERE k = 'audit2') || '{"id":"a0000000-0000-0000-0000-000000000044","device_seq":4,"prev_hash":"3333333333333333333333333333333333333333333333333333333333333333","hash":"4444444444444444444444444444444444444444444444444444444444444444"}')))),
  'ok', 'an audit row that skips the chain is still stored');
RESET ROLE;
SELECT is((SELECT row(chain_broken_seq, last_audit_seq)::text FROM pos.devices WHERE id = 'a0000000-0000-0000-0000-0000000000d1'),
  '(4,4)', 'and the device is flagged at that row');

-- ─── retry, rejections, replays of rejections ───
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
INSERT INTO res SELECT 'orphan', sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('order_payments',
  (SELECT r FROM w WHERE k = 'payment') || '{"id":"a0000000-0000-0000-0000-000000000028","order_id":"a0000000-0000-0000-0000-0000000000ff"}')));
SELECT is(pg_temp.statuses((SELECT v FROM res WHERE k = 'orphan')), 'retry', 'a row whose parent is not in the cloud yet is retried');
RESET ROLE;
SELECT is((SELECT count(*)::int FROM pos.processed_ops WHERE op_id = ((SELECT v FROM res WHERE k = 'orphan') -> 'results' -> 0 ->> 'op_id')::uuid),
  0, 'and not recorded, so it can succeed later');
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
INSERT INTO res SELECT 'delete', sync_push('a0000000-0000-0000-0000-0000000000d1',
  jsonb_build_array(pg_temp.op('products', (SELECT r FROM w WHERE k = 'product')) || '{"op":"delete"}'));
SELECT is(pg_temp.reason((SELECT v FROM res WHERE k = 'delete')), 'UNSUPPORTED_OP: delete', 'deletes are not a sync op');
SELECT is(pg_temp.reason(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(pg_temp.op('sync_outbox', '{}')))),
  'UNKNOWN_TABLE: sync_outbox', 'only synced tables are accepted');
SELECT is(pg_temp.statuses(sync_push('a0000000-0000-0000-0000-0000000000d1', jsonb_build_array(
  (SELECT v FROM res WHERE k = 'delete') -> 'results' -> 0 || jsonb_build_object('table', 'products', 'op', 'upsert',
    'row', (SELECT r FROM w WHERE k = 'product'))))),
  'rejected', 'a replayed op that was rejected is rejected again');

-- ─── pull ───
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d3');
INSERT INTO res SELECT 'full_t03', sync_pull(0, 1000);
SELECT ok(NOT ((SELECT v FROM res WHERE k = 'full_t03') ->> 'has_more')::boolean, 'one page holds everything');
SELECT is((SELECT count(*)::int FROM jsonb_array_elements((SELECT v FROM res WHERE k = 'full_t03') -> 'rows') e
           WHERE e -> 'row' ->> 'updated_by_device' = 'a0000000-0000-0000-0000-0000000000d3'), 0,
  'a device never receives its own writes');
SELECT ok((SELECT bool_and(a.s < b.s) FROM (
             SELECT (e ->> 'seq')::bigint AS s, row_number() OVER () AS n
             FROM jsonb_array_elements((SELECT v FROM res WHERE k = 'full_t03') -> 'rows') e) a
           JOIN (SELECT (e ->> 'seq')::bigint AS s, row_number() OVER () AS n
                 FROM jsonb_array_elements((SELECT v FROM res WHERE k = 'full_t03') -> 'rows') e) b ON b.n = a.n + 1),
  'rows come in server_seq order');
-- Byte-for-byte: what T01 pushed is what T03 receives, for every table T01 wrote and nobody changed since.
SELECT is(
  (SELECT count(*)::int FROM w JOIN LATERAL (
     SELECT e -> 'row' AS pulled FROM jsonb_array_elements((SELECT v FROM res WHERE k = 'full_t03') -> 'rows') e
     WHERE e ->> 'table' = w.tbl AND (e -> 'row' ->> 'id' = w.r ->> 'id' OR e -> 'row' ->> 'key' = w.r ->> 'key')) p ON true
   WHERE w.k IN ('owner_role','cashier_role','setting','batch','layer','intake','session','line','payment','slice','audit1','audit2')
     AND p.pulled = w.r),
  12, 'pushed rows come back identical: numbers, 0/1 flags, millisecond timestamps, JSON text untouched');
SELECT is((SELECT e -> 'row' ->> 'metadata' FROM jsonb_array_elements((SELECT v FROM res WHERE k = 'full_t03') -> 'rows') e
           WHERE e -> 'row' ->> 'id' = 'a0000000-0000-0000-0000-000000000041'),
  '{"device_code":"T01","store_name":"Store A"}', 'audit metadata keeps its exact bytes');
SELECT is((SELECT e -> 'row' ->> 'updated_by_device' FROM jsonb_array_elements((SELECT v FROM res WHERE k = 'full_t03') -> 'rows') e
           WHERE e -> 'row' ->> 'id' = 'a0000000-0000-0000-0000-0000000000a2'),
  '00000000-0000-0000-0000-000000000000', 'server-changed rows carry the SYSTEM device');
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
SELECT ok(EXISTS (SELECT 1 FROM jsonb_array_elements(sync_pull(0, 1000) -> 'rows') e
                  WHERE e -> 'row' ->> 'id' = 'a0000000-0000-0000-0000-0000000000a2'),
  'so the parked product returns to the counter that registered it');

-- Paging: small pages, followed by cursor, deliver exactly the full set.
CREATE TEMP TABLE paged (seq bigint, tbl text, id text);
GRANT ALL ON paged TO authenticated;
DO $$
DECLARE c bigint := 0; p jsonb; pages int := 0;
BEGIN
  PERFORM pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d3');
  LOOP
    p := sync_pull(c, 4);
    INSERT INTO paged SELECT (e ->> 'seq')::bigint, e ->> 'table', COALESCE(e -> 'row' ->> 'id', e -> 'row' ->> 'key')
      FROM jsonb_array_elements(p -> 'rows') e;
    c := (p ->> 'cursor')::bigint;
    pages := pages + 1;
    EXIT WHEN NOT (p ->> 'has_more')::boolean OR pages > 50;
  END LOOP;
  INSERT INTO paged VALUES (-pages, 'pages', NULL);
END $$;
SELECT ok((SELECT -min(seq) FROM paged WHERE tbl = 'pages') > 3, 'a small page size takes several pages');
SELECT is(
  (SELECT count(DISTINCT (tbl, id))::int FROM paged WHERE tbl <> 'pages'),
  (SELECT jsonb_array_length((SELECT v FROM res WHERE k = 'full_t03') -> 'rows')),
  'and pages together deliver exactly the rows of one big pull');

-- Parent closure: the product was edited (by T02 and T03) after T01's sale slice referenced it, so it sorts
-- after the slice; a page that cuts right after the slice must still carry the product.
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d2');
RESET ROLE;
CREATE TEMP TABLE slice_seq AS SELECT server_seq - 1 AS c FROM pos.stock_movements WHERE id = 'a0000000-0000-0000-0000-000000000032';
GRANT ALL ON slice_seq TO authenticated;
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('a0000000-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-0000000000d1');
INSERT INTO res SELECT 'closure', sync_pull((SELECT c FROM slice_seq), 1, true);
SELECT ok(EXISTS (SELECT 1 FROM jsonb_array_elements((SELECT v FROM res WHERE k = 'closure') -> 'rows') e
                  WHERE e ->> 'table' = 'stock_movements' AND e -> 'row' ->> 'id' = 'a0000000-0000-0000-0000-000000000032')
      AND EXISTS (SELECT 1 FROM jsonb_array_elements((SELECT v FROM res WHERE k = 'closure') -> 'rows') e
                  WHERE e ->> 'table' = 'products' AND e -> 'row' ->> 'id' = 'a0000000-0000-0000-0000-0000000000a1'),
  'a page carries the ancestors edited after its rows');
SELECT is(((SELECT v FROM res WHERE k = 'closure') ->> 'cursor')::bigint, (SELECT c FROM slice_seq) + 1,
  'and its cursor stops at its own last row, not at the ancestors');

-- The watermark: a device whose own writes are newest still reaches the end.
INSERT INTO res SELECT 'own_tail', sync_pull((SELECT max(seq) FROM paged), 1000);
RESET ROLE;
SELECT is(((SELECT v FROM res WHERE k = 'own_tail') ->> 'watermark')::bigint,
  ((SELECT v FROM res WHERE k = 'own_tail') ->> 'cursor')::bigint, 'the watermark is reported with every page');
SELECT is(((SELECT v FROM res WHERE k = 'own_tail') ->> 'cursor')::bigint,
  (SELECT max(server_seq) FROM (SELECT server_seq FROM pos.audit_logs UNION ALL SELECT server_seq FROM pos.customers
                                UNION ALL SELECT server_seq FROM pos.products UNION ALL SELECT server_seq FROM pos.staff) s),
  'the last page''s cursor is the store watermark');

-- Store isolation.
SET LOCAL ROLE authenticated;
SELECT pg_temp.as_device('b0000000-0000-0000-0000-000000000001', 'b0000000-0000-0000-0000-0000000000d1');
SELECT is(jsonb_array_length(sync_pull(0, 1000) -> 'rows'), 0, 'another store''s device receives nothing');
RESET ROLE;

SELECT * FROM finish();
ROLLBACK;
