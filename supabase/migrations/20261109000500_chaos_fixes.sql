-- GayanShop POS — Block 3.6: fixes the chaos suite found against the real stack.
--
-- 1. A row written twice by one device within the same millisecond (an ad-hoc purchase creates its batch and
--    receives it in one transaction) reached sync_push as two snapshots with the same (updated_at,
--    updated_by_device); last-write-wins kept the first and dropped the second as stale. One device's changes
--    always arrive in order, so on a timestamp tie the later snapshot from the same device wins.
CREATE OR REPLACE FUNCTION pos.is_newer(new_at timestamptz, new_dev uuid, old_at timestamptz, old_dev uuid)
RETURNS boolean LANGUAGE sql IMMUTABLE AS $$
  SELECT new_at > old_at OR (new_at = old_at AND new_dev >= old_dev)
$$;

-- 2. A checkout token dedupes retries of one cart on one counter; the cloud made it unique across every
--    store, so two stores that happened to share a token could not both keep their order. Scoped per store.
DROP INDEX pos.ux_orders_checkout_token;
CREATE UNIQUE INDEX ux_orders_checkout_token ON pos.orders(store_id, checkout_token);

-- 3. Found while scripting the concurrent-owner-demotion test: a refused change
-- must not leave the counter that made it disagreeing with the cloud. When sync_push rejects a change to a
-- mutable row (an owner demotion that would leave no Owner, a cost change on a received layer, …), the cloud's
-- current row is re-stamped with a new server_seq, so every counter, the refusing one included, pulls the
-- authoritative version; the terminal lets it replace the local change the cloud refused (its op is dead and
-- shown in Settings → Sync). Doc §6.3.

CREATE FUNCTION pos.resend_current(p_store uuid, tbl text, r jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE kind text;
BEGIN
  SELECT st.kind INTO kind FROM pos.synced_tables st WHERE st.name = tbl;
  IF kind IS DISTINCT FROM 'lww' OR jsonb_typeof(r) IS DISTINCT FROM 'object' THEN
    RETURN;
  END IF;
  IF tbl = 'store_settings' THEN
    UPDATE pos.store_settings SET version = version WHERE store_id = p_store AND key = r ->> 'key';
  ELSE
    EXECUTE format('UPDATE pos.%I SET version = version WHERE store_id = $1 AND id = ($2 ->> ''id'')::uuid', tbl)
    USING p_store, r;
  END IF;
EXCEPTION WHEN others THEN
  -- Re-sending is a courtesy; it never turns a rejection into an error.
  RETURN;
END $$;
REVOKE ALL ON FUNCTION pos.resend_current(uuid, text, jsonb) FROM PUBLIC, anon, authenticated;
GRANT EXECUTE ON FUNCTION pos.resend_current(uuid, text, jsonb) TO pos_sync;

CREATE OR REPLACE FUNCTION public.sync_push(device_id uuid, ops jsonb, app_version text DEFAULT NULL)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pos, pg_temp AS $$
#variable_conflict use_column
DECLARE
  me record;
  op jsonb;
  v_op_id uuid;
  prior pos.processed_ops;
  results jsonb := '[]'::jsonb;
  reason text;
BEGIN
  SELECT * INTO me FROM pos.require_device();
  IF sync_push.device_id IS DISTINCT FROM me.device_id THEN
    RAISE EXCEPTION 'DEVICE_MISMATCH' USING ERRCODE = '42501';
  END IF;
  IF jsonb_typeof(ops) IS DISTINCT FROM 'array' OR jsonb_array_length(ops) > 500 THEN
    RAISE EXCEPTION 'BAD_BATCH' USING ERRCODE = '22023';
  END IF;
  PERFORM pg_advisory_xact_lock(hashtext(me.store_id::text));

  FOR op IN SELECT value FROM jsonb_array_elements(ops) LOOP
    BEGIN
      v_op_id := (op ->> 'op_id')::uuid;
    EXCEPTION WHEN others THEN
      results := results || jsonb_build_array(jsonb_build_object('op_id', op -> 'op_id', 'status', 'rejected',
                                                                 'reason', 'BAD_OP_ID'));
      CONTINUE;
    END;
    SELECT * INTO prior FROM pos.processed_ops WHERE op_id = v_op_id;
    IF FOUND THEN
      results := results || jsonb_build_array(jsonb_build_object('op_id', v_op_id, 'status', prior.result,
                                                                 'reason', prior.reason));
      CONTINUE;
    END IF;
    BEGIN
      IF op ->> 'op' IS DISTINCT FROM 'upsert' THEN PERFORM pos.reject('UNSUPPORTED_OP', op ->> 'op'); END IF;
      PERFORM pos.apply_row(me.store_id, me.device_id, op ->> 'table', op -> 'row');
      INSERT INTO pos.processed_ops (op_id, store_id, device_id, table_name, result)
      VALUES (v_op_id, me.store_id, me.device_id, COALESCE(op ->> 'table', '?'), 'ok');
      results := results || jsonb_build_array(jsonb_build_object('op_id', v_op_id, 'status', 'ok'));
    EXCEPTION
      WHEN foreign_key_violation THEN
        results := results || jsonb_build_array(jsonb_build_object('op_id', v_op_id, 'status', 'retry',
                                                                   'reason', 'PARENT_MISSING: ' || SQLERRM));
      WHEN others THEN
        reason := left(CASE WHEN SQLSTATE = 'P0001' THEN SQLERRM ELSE SQLSTATE || ': ' || SQLERRM END, 500);
        INSERT INTO pos.processed_ops (op_id, store_id, device_id, table_name, result, reason)
        VALUES (v_op_id, me.store_id, me.device_id, COALESCE(op ->> 'table', '?'), 'rejected', reason);
        PERFORM pos.resend_current(me.store_id, op ->> 'table', op -> 'row');
        results := results || jsonb_build_array(jsonb_build_object('op_id', v_op_id, 'status', 'rejected',
                                                                   'reason', reason));
    END;
  END LOOP;

  UPDATE pos.devices d SET last_seen_at = now(), app_version = COALESCE(sync_push.app_version, d.app_version)
  WHERE d.id = me.device_id;
  RETURN jsonb_build_object('results', results);
END $$;
