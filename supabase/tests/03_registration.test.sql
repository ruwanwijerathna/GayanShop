-- Block 3.3: the registration functions the register-device edge function calls with the service role.
-- A counter's ids are adopted, never reassigned; one owner account owns one store (decisions #18–#19).
BEGIN;
CREATE EXTENSION IF NOT EXISTS pgtap WITH SCHEMA extensions;
SET search_path = public, extensions;
SELECT plan(16);

-- The first counter adopts the store it created offline.
SELECT is(
  admin_claim_device('a0000000-0000-0000-0000-0000000000aa', 'a0000000-0000-0000-0000-000000000001', 'GayanShop',
                     'a0000000-0000-0000-0000-0000000000d1', 'T01', 'Front counter', '0.3.0') - 'auth_user_id',
  '{"store_id":"a0000000-0000-0000-0000-000000000001","store_name":"GayanShop","device_id":"a0000000-0000-0000-0000-0000000000d1","device_code":"T01"}'::jsonb,
  'the first counter registers its own store, device id and code');
SELECT is((SELECT owner_user_id FROM pos.stores WHERE id = 'a0000000-0000-0000-0000-000000000001'),
  'a0000000-0000-0000-0000-0000000000aa'::uuid, 'and the store belongs to the owner''s account');
SELECT is((SELECT name FROM pos.devices WHERE id = 'a0000000-0000-0000-0000-0000000000d1'), 'Front counter',
  'the device row carries its name');

-- Later counters join with a fresh id and get the next free code.
SELECT is(admin_claim_device('a0000000-0000-0000-0000-0000000000aa', NULL, NULL, 'a0000000-0000-0000-0000-0000000000d2',
                             NULL, NULL, '0.3.0') ->> 'device_code', 'T02', 'a joining counter gets T02');
SELECT is(admin_claim_device('a0000000-0000-0000-0000-0000000000aa', NULL, NULL, 'a0000000-0000-0000-0000-0000000000d3',
                             NULL, '', '0.3.0') ->> 'device_code', 'T03', 'the next one T03');
SELECT is((SELECT name FROM pos.devices WHERE id = 'a0000000-0000-0000-0000-0000000000d3'), 'Counter T03',
  'an unnamed counter is named after its code');
SELECT is(admin_claim_device('a0000000-0000-0000-0000-0000000000aa', NULL, NULL, 'a0000000-0000-0000-0000-0000000000d2',
                             NULL, NULL, '0.3.1') ->> 'device_code', 'T02', 're-registering keeps the counter''s code');
SELECT is((SELECT count(*)::int FROM pos.devices WHERE store_id = 'a0000000-0000-0000-0000-000000000001'), 3,
  'and does not add a device');

-- Refusals.
SELECT throws_ok($$SELECT admin_claim_device('b0000000-0000-0000-0000-0000000000bb', 'a0000000-0000-0000-0000-000000000001',
  'Mine now', 'b0000000-0000-0000-0000-0000000000d1', 'T01', NULL, NULL)$$, '42501', 'STORE_OWNED_BY_ANOTHER_ACCOUNT',
  'another account cannot claim the store');
SELECT throws_ok($$SELECT admin_claim_device('a0000000-0000-0000-0000-0000000000aa', 'c0000000-0000-0000-0000-000000000001',
  'Second shop', 'c0000000-0000-0000-0000-0000000000d1', 'T01', NULL, NULL)$$, '42501', 'ACCOUNT_OWNS_ANOTHER_STORE',
  'one account owns one store');
SELECT throws_ok($$SELECT admin_claim_device('b0000000-0000-0000-0000-0000000000bb', NULL, NULL,
  'b0000000-0000-0000-0000-0000000000d1', NULL, NULL, NULL)$$, 'P0002', 'NO_STORE_FOR_ACCOUNT',
  'an account without a store has nothing to join');
SELECT throws_ok($$SELECT admin_claim_device('a0000000-0000-0000-0000-0000000000aa', 'a0000000-0000-0000-0000-000000000001',
  'GayanShop', 'a0000000-0000-0000-0000-0000000000d9', 'T02', NULL, NULL)$$, '23505', 'DEVICE_CODE_TAKEN',
  'a code already used in the store is refused');
SELECT throws_ok($$SELECT admin_claim_device('a0000000-0000-0000-0000-0000000000aa', 'a0000000-0000-0000-0000-000000000001',
  'GayanShop', 'a0000000-0000-0000-0000-0000000000d1', 'T05', NULL, NULL)$$, '23505', 'DEVICE_CODE_MISMATCH',
  'a registered counter cannot change its code');
INSERT INTO pos.stores (id, name, owner_user_id) VALUES
  ('b0000000-0000-0000-0000-000000000001', 'Other', 'b0000000-0000-0000-0000-0000000000bb');
SELECT throws_ok($$SELECT admin_claim_device('b0000000-0000-0000-0000-0000000000bb', NULL, NULL,
  'a0000000-0000-0000-0000-0000000000d1', NULL, NULL, NULL)$$, '42501', 'DEVICE_IN_ANOTHER_STORE',
  'a device cannot move to another store');

-- Binding the identity, and who may call these.
SELECT lives_ok($$SELECT admin_bind_device('a0000000-0000-0000-0000-0000000000d1', 'a0000000-0000-0000-0000-00000000aaa1')$$,
  'the edge function binds the device identity');
SELECT ok(NOT has_function_privilege('authenticated', 'public.admin_claim_device(uuid,uuid,text,uuid,text,text,text)', 'EXECUTE')
      AND NOT has_function_privilege('anon', 'public.admin_bind_device(uuid,uuid)', 'EXECUTE'),
  'only the service role can call the registration functions');

SELECT * FROM finish();
ROLLBACK;
