-- GayanShop POS — sync RPCs (Block 3.2, doc §6). Plain Postgres functions with JSON in and out, served by
-- PostgREST at POST /rest/v1/rpc/sync_push | sync_pull | sync_health. They run SECURITY DEFINER as pos_sync,
-- which has no BYPASSRLS: every read and write still passes the store-isolation policies of Block 3.1.
--
-- Wire format: a row is the terminal's SQLite row as JSON (json_object over every column): integers as
-- numbers, flags as 0/1, timestamps as `YYYY-MM-DDTHH:MM:SS.mmmZ`, dates as `YYYY-MM-DD`, NULL as null.
-- sync_pull renders rows back exactly that way, so a row pushed by T01 reaches T02 byte for byte.

-- ═════════ catalogue helpers ═════════
-- Business columns of a synced table, in table order (the server's own cursor columns excluded).
CREATE FUNCTION pos.cols(tbl text) RETURNS text[] LANGUAGE sql STABLE AS $$
  SELECT array_agg(a.attname::text ORDER BY a.attnum)
  FROM pg_attribute a
  WHERE a.attrelid = format('pos.%I', tbl)::regclass AND a.attnum > 0 AND NOT a.attisdropped
    AND a.attname NOT IN ('server_seq', 'server_received_at')
$$;

-- The SQL expression that renders row `x` of a synced table in the terminal's wire format.
CREATE FUNCTION pos.wire_expr(tbl text) RETURNS text LANGUAGE sql STABLE AS $$
  SELECT format('(to_jsonb(x) - ''server_seq'' - ''server_received_at'')%s',
    COALESCE(string_agg(
      CASE format_type(a.atttypid, a.atttypmod)
        WHEN 'boolean' THEN format(' || jsonb_build_object(%L, x.%I::int)', a.attname, a.attname)
        WHEN 'timestamp with time zone' THEN format(' || jsonb_build_object(%L, pos.ts(x.%I))', a.attname, a.attname)
      END, '' ORDER BY a.attnum), ''))
  FROM pg_attribute a
  WHERE a.attrelid = format('pos.%I', tbl)::regclass AND a.attnum > 0 AND NOT a.attisdropped
    AND a.attname NOT IN ('server_seq', 'server_received_at')
    AND format_type(a.atttypid, a.atttypmod) IN ('boolean', 'timestamp with time zone')
$$;

-- "Not this device's own write" (doc §6.2): mutable tables and orders by updated_by_device, insert-only
-- tables by device_id. Rows the server changed carry the SYSTEM device, so they always come back.
CREATE FUNCTION pos.not_own_expr(tbl text, kind text) RETURNS text LANGUAGE sql IMMUTABLE AS $$
  SELECT CASE WHEN kind = 'insert_only' THEN 'x.device_id <> $4' ELSE 'x.updated_by_device <> $4' END
$$;

-- A rejection: the op is recorded as rejected and its reason returned to the terminal (which marks it dead).
CREATE FUNCTION pos.reject(code text, detail text DEFAULT NULL) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION '%', code || COALESCE(': ' || detail, '') USING ERRCODE = 'P0001';
END $$;

CREATE FUNCTION pos.require_device() RETURNS TABLE (store_id uuid, device_id uuid) LANGUAGE plpgsql STABLE AS $$
BEGIN
  store_id := pos.claim_store_id();
  device_id := pos.claim_device_id();
  IF store_id IS NULL OR device_id IS NULL THEN
    RAISE EXCEPTION 'NOT_A_DEVICE' USING ERRCODE = '42501';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pos.devices d WHERE d.id = require_device.device_id
                 AND d.store_id = require_device.store_id) THEN
    RAISE EXCEPTION 'DEVICE_NOT_REGISTERED' USING ERRCODE = '42501';
  END IF;
  RETURN NEXT;
END $$;

-- ═════════ merge rules (doc §6.3) ═════════
CREATE FUNCTION pos.is_newer(new_at timestamptz, new_dev uuid, old_at timestamptz, old_dev uuid)
RETURNS boolean LANGUAGE sql IMMUTABLE AS $$
  SELECT new_at > old_at OR (new_at = old_at AND new_dev > old_dev)
$$;

-- Whole-row last-write-wins upsert; a stale write is a no-op.
CREATE FUNCTION pos.upsert_lww(tbl text, pk text[], r jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE cols text[] := pos.cols(tbl);
BEGIN
  EXECUTE format(
    'INSERT INTO pos.%1$I (%2$s) SELECT %2$s FROM jsonb_populate_record(NULL::pos.%1$I, $1)
     ON CONFLICT (%3$s) DO UPDATE SET (%2$s) = ROW(%4$s)
     WHERE pos.is_newer(EXCLUDED.updated_at, EXCLUDED.updated_by_device, %1$I.updated_at, %1$I.updated_by_device)',
    tbl,
    (SELECT string_agg(quote_ident(c), ', ') FROM unnest(cols) c),
    (SELECT string_agg(quote_ident(c), ', ') FROM unnest(pk) c),
    (SELECT string_agg('EXCLUDED.' || quote_ident(c), ', ') FROM unnest(cols) c))
  USING r;
END $$;

CREATE FUNCTION pos.insert_ignore(tbl text, r jsonb) RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE cols text := (SELECT string_agg(quote_ident(c), ', ') FROM unnest(pos.cols(tbl)) c);
        n int;
BEGIN
  EXECUTE format('INSERT INTO pos.%1$I (%2$s) SELECT %2$s FROM jsonb_populate_record(NULL::pos.%1$I, $1)
                  ON CONFLICT (id) DO NOTHING', tbl, cols) USING r;
  GET DIAGNOSTICS n = ROW_COUNT;
  RETURN n = 1;
END $$;

-- Whether the store has an active Owner (doc §3.5).
CREATE FUNCTION pos.has_active_owner(p_store uuid) RETURNS boolean LANGUAGE sql STABLE AS $$
  SELECT EXISTS (
    SELECT 1 FROM pos.staff s JOIN pos.roles r ON r.id = s.role_id
    WHERE s.store_id = p_store AND s.deleted_at IS NULL AND s.is_active
      AND r.is_system AND r.name = 'Owner' AND r.deleted_at IS NULL)
$$;

-- Apply one upsert. Raises P0001 'CODE: detail' to reject it.
CREATE FUNCTION pos.apply_row(p_store uuid, p_dev uuid, tbl text, r jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE
  meta pos.synced_tables;
  cur jsonb;
  inc_at timestamptz;
  inc_dev uuid;
  bad_status_change boolean;
  rank_old int;
  rank_new int;
  is_new_audit boolean;
  had_owner boolean;
  d pos.devices;
BEGIN
  SELECT * INTO meta FROM pos.synced_tables WHERE name = tbl;
  IF NOT FOUND THEN PERFORM pos.reject('UNKNOWN_TABLE', tbl); END IF;
  IF jsonb_typeof(r) IS DISTINCT FROM 'object' THEN PERFORM pos.reject('BAD_ROW'); END IF;
  IF (r ->> 'store_id')::uuid IS DISTINCT FROM p_store THEN PERFORM pos.reject('STORE_MISMATCH'); END IF;

  IF meta.kind = 'insert_only' THEN
    IF (r ->> 'device_id')::uuid IS DISTINCT FROM p_dev THEN PERFORM pos.reject('DEVICE_MISMATCH'); END IF;
    is_new_audit := pos.insert_ignore(tbl, r) AND tbl = 'audit_logs';
    -- The audit chain (§6.3): a row that does not extend this device's chain is still stored — losing audit
    -- rows would be worse — but the device is flagged.
    IF is_new_audit THEN
      SELECT * INTO d FROM pos.devices WHERE id = p_dev FOR UPDATE;
      IF (r ->> 'device_seq')::bigint <> d.last_audit_seq + 1 OR (r ->> 'prev_hash') <> d.last_audit_hash THEN
        UPDATE pos.devices SET chain_broken_at = COALESCE(chain_broken_at, now()),
                               chain_broken_seq = COALESCE(chain_broken_seq, (r ->> 'device_seq')::bigint)
        WHERE id = p_dev;
      END IF;
      IF (r ->> 'device_seq')::bigint > d.last_audit_seq THEN
        UPDATE pos.devices SET last_audit_seq = (r ->> 'device_seq')::bigint, last_audit_hash = r ->> 'hash'
        WHERE id = p_dev;
      END IF;
    END IF;
    RETURN;
  END IF;

  IF (r ->> 'updated_by_device')::uuid IS DISTINCT FROM p_dev THEN PERFORM pos.reject('DEVICE_MISMATCH'); END IF;

  IF meta.kind = 'order' THEN
    -- Orders: sealed rows only; inserted once; the only later change is completed → voided by the selling
    -- device (the state-machine trigger re-checks every immutable field).
    IF (r ->> 'device_id')::uuid IS DISTINCT FROM p_dev THEN PERFORM pos.reject('DEVICE_MISMATCH'); END IF;
    IF r ->> 'status' NOT IN ('completed', 'voided') THEN PERFORM pos.reject('ORDER_NOT_SEALED', r ->> 'status'); END IF;
    SELECT to_jsonb(o) INTO cur FROM pos.orders o WHERE o.id = (r ->> 'id')::uuid;
    IF cur IS NULL THEN
      PERFORM pos.insert_ignore('orders', r);
    ELSIF cur ->> 'status' = 'completed' AND r ->> 'status' = 'voided' THEN
      BEGIN
        EXECUTE format('UPDATE pos.orders o SET (%1$s) = (SELECT %1$s FROM jsonb_populate_record(NULL::pos.orders, $1))
                        WHERE o.id = $2',
          (SELECT string_agg(quote_ident(c), ', ') FROM unnest(pos.cols('orders')) c))
        USING r, (r ->> 'id')::uuid;
      EXCEPTION WHEN check_violation THEN
        PERFORM pos.reject('ORDER_IMMUTABLE', SQLERRM);
      END;
    END IF;
    -- Same status again, or a completed snapshot after the void: nothing to change.
    RETURN;
  END IF;

  -- Last-write-wins tables.
  inc_at := (r ->> 'updated_at')::timestamptz;
  inc_dev := (r ->> 'updated_by_device')::uuid;
  EXECUTE format('SELECT to_jsonb(x) FROM pos.%I x WHERE %s', tbl,
    CASE WHEN tbl = 'store_settings' THEN 'x.store_id = ($1 ->> ''store_id'')::uuid AND x.key = $1 ->> ''key'''
         ELSE 'x.id = ($1 ->> ''id'')::uuid' END)
  INTO cur USING r;
  IF cur IS NOT NULL AND NOT pos.is_newer(inc_at, inc_dev, (cur ->> 'updated_at')::timestamptz,
                                          (cur ->> 'updated_by_device')::uuid) THEN
    RETURN;                                          -- stale: the newer row stays
  END IF;

  IF tbl = 'batches' AND cur IS NOT NULL THEN
    -- Status moves forward only (draft → received → closed), except an audited reopen of a received batch
    -- that has no movement besides its intake (doc §6.3).
    rank_old := array_position(ARRAY['draft','received','closed'], cur ->> 'status');
    rank_new := array_position(ARRAY['draft','received','closed'], r ->> 'status');
    IF rank_new < rank_old THEN
      bad_status_change := NOT (cur ->> 'status' = 'received' AND r ->> 'status' = 'draft')
        OR EXISTS (SELECT 1 FROM pos.stock_movements m JOIN pos.batch_items bi ON bi.id = m.batch_item_id
                   WHERE bi.batch_id = (r ->> 'id')::uuid AND m.reason NOT IN ('batch_intake','batch_reopen'));
      IF bad_status_change THEN
        PERFORM pos.reject('BATCH_STATUS_BACKWARDS', (cur ->> 'status') || ' → ' || (r ->> 'status'));
      END IF;
    END IF;
  ELSIF tbl = 'batch_items' AND cur IS NOT NULL THEN
    IF ((r ->> 'unit_cost')::bigint <> (cur ->> 'unit_cost')::bigint
        OR (r ->> 'qty_received')::bigint <> (cur ->> 'qty_received')::bigint)
       AND (SELECT b.status FROM pos.batches b WHERE b.id = (cur ->> 'batch_id')::uuid) <> 'draft' THEN
      PERFORM pos.reject('COST_LOCKED', 'batch item cost/qty locked after receive');
    END IF;
  ELSIF tbl = 'customers' AND cur IS NOT NULL THEN
    -- An erasure wins over any concurrent edit.
    IF cur ->> 'erased_at' IS NOT NULL AND r ->> 'erased_at' IS NULL THEN
      RETURN;
    END IF;
  ELSIF tbl = 'products' THEN
    -- Two counters registered the same barcode offline: never a rejection (a sale must not fail over it).
    -- Park the incoming product as inactive + barcode_conflict, stamped by SYSTEM so it returns to its
    -- origin too; the owner resolves it in "Barcode conflicts".
    IF (r ->> 'is_active')::int = 1 AND r ->> 'barcode' IS NOT NULL AND r ->> 'deleted_at' IS NULL
       AND EXISTS (SELECT 1 FROM pos.products p
                   WHERE p.store_id = p_store AND p.barcode = r ->> 'barcode' AND p.id <> (r ->> 'id')::uuid
                     AND p.is_active AND p.deleted_at IS NULL) THEN
      r := r || jsonb_build_object(
        'is_active', 0, 'barcode_conflict', 1, 'updated_by_device', pos.system_device(),
        'updated_at', pos.ts(GREATEST(date_trunc('milliseconds', now()),
                                      inc_at + interval '1 millisecond',
                                      COALESCE((cur ->> 'updated_at')::timestamptz, inc_at) + interval '1 millisecond')));
    END IF;
  END IF;

  -- The owner invariant (§6.3): a change that leaves the store without an active Owner is rejected. Only a
  -- change that removes the last one counts; while a store is first being seeded there is none yet.
  had_owner := tbl IN ('staff', 'roles') AND pos.has_active_owner(p_store);
  BEGIN
    PERFORM pos.upsert_lww(tbl, meta.pk, r);
  EXCEPTION WHEN check_violation THEN
    PERFORM pos.reject(CASE WHEN tbl = 'batch_items' THEN 'COST_LOCKED'
                            WHEN tbl = 'cash_sessions' THEN 'SESSION_CLOSED' ELSE 'CHECK' END, SQLERRM);
  END;
  IF had_owner AND NOT pos.has_active_owner(p_store) THEN
    PERFORM pos.reject('OWNER_INVARIANT', 'the store must keep an active Owner');
  END IF;
END $$;

-- ═════════ sync_push (doc §6.1) ═════════
-- ops: [{op_id, table, op: 'upsert', row}], parents before children. One transaction, serialised per store by
-- an advisory lock so server_seq assignment order equals commit order. Each op runs in its own subtransaction:
--   ok        applied (or a replay of an op already processed, or a stale write)
--   rejected  violates a merge rule or invariant; recorded, so a replay is rejected the same way
--   retry     a parent it references is not in the cloud yet; not recorded, the terminal sends it again
CREATE FUNCTION public.sync_push(device_id uuid, ops jsonb, app_version text DEFAULT NULL)
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
        results := results || jsonb_build_array(jsonb_build_object('op_id', v_op_id, 'status', 'rejected',
                                                                   'reason', reason));
    END;
  END LOOP;

  UPDATE pos.devices d SET last_seen_at = now(), app_version = COALESCE(sync_push.app_version, d.app_version)
  WHERE d.id = me.device_id;
  RETURN jsonb_build_object('results', results);
END $$;

-- ═════════ sync_pull (doc §6.2) ═════════
-- Rows of the caller's store with server_seq > cursor, in server_seq order, without this device's own
-- writes (unless include_own, used by bootstrap). Every page also carries each row's ancestors with
-- server_seq > cursor, following the foreign keys, so a terminal can apply any page whatever its limit.
-- Returns {rows: [{seq, table, row}], cursor, has_more, watermark}: cursor is where the next call starts, and
-- watermark the store's newest server_seq when the page was taken (bootstrap shows progress against it).
CREATE FUNCTION public.sync_pull(cursor bigint DEFAULT 0, max_rows int DEFAULT 500, include_own boolean DEFAULT false)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pos, pg_temp AS $$
DECLARE
  me record;
  lim int := LEAST(GREATEST(COALESCE(max_rows, 500), 1), 2000);
  since bigint := GREATEST(COALESCE(cursor, 0), 0);
  watermark bigint := 0;
  m bigint;
  t record;
  fk record;
  n int;
  added int;
  has_more boolean;
  next_cursor bigint;
  out_rows jsonb := '[]'::jsonb;
  page jsonb;
  own_dev uuid;
BEGIN
  SELECT * INTO me FROM pos.require_device();
  -- With include_own no row is "own": compare against a device id no row carries.
  own_dev := CASE WHEN include_own THEN '00000000-0000-0000-0000-000000000001'::uuid ELSE me.device_id END;

  -- The watermark first: every row at or below it is committed (sync_push serialises a store's writers), so
  -- when this page is the last one the cursor can safely jump to it, past this device's own rows.
  FOR t IN SELECT name FROM pos.synced_tables LOOP
    EXECUTE format('SELECT COALESCE(MAX(server_seq), 0) FROM pos.%I WHERE store_id = $1', t.name)
    INTO m USING me.store_id;
    watermark := GREATEST(watermark, m);
  END LOOP;

  CREATE TEMP TABLE IF NOT EXISTS _pull (tbl text, id text, seq bigint, PRIMARY KEY (tbl, id)) ON COMMIT DROP;
  TRUNCATE _pull;
  FOR t IN SELECT name, kind FROM pos.synced_tables LOOP
    EXECUTE format(
      'INSERT INTO _pull (tbl, id, seq)
       SELECT %1$L, %3$s, x.server_seq FROM pos.%1$I x
       WHERE x.store_id = $1 AND x.server_seq > $2 AND x.server_seq <= $5 AND %2$s
       ORDER BY x.server_seq LIMIT $3 + 1',
      t.name, pos.not_own_expr(t.name, t.kind), CASE WHEN t.name = 'store_settings' THEN 'x.key' ELSE 'x.id::text' END)
    USING me.store_id, since, lim, own_dev, watermark;
  END LOOP;
  SELECT count(*) INTO n FROM _pull;
  has_more := n > lim;
  IF has_more THEN
    DELETE FROM _pull WHERE seq > (SELECT seq FROM _pull ORDER BY seq OFFSET lim - 1 LIMIT 1);
    SELECT MAX(seq) INTO next_cursor FROM _pull;
  ELSE
    next_cursor := GREATEST(since, watermark);
  END IF;

  -- Parent closure: add ancestors (server_seq > cursor, not own) until nothing new is found.
  LOOP
    added := 0;
    FOR fk IN
      SELECT ch.relname AS child, pa.relname AS parent, ca.attname AS child_col, pa_att.attname AS parent_col
      FROM pg_constraint c
      JOIN pg_class ch ON ch.oid = c.conrelid
      JOIN pg_class pa ON pa.oid = c.confrelid
      JOIN pg_attribute ca ON ca.attrelid = c.conrelid AND ca.attnum = c.conkey[1]
      JOIN pg_attribute pa_att ON pa_att.attrelid = c.confrelid AND pa_att.attnum = c.confkey[1]
      JOIN pos.synced_tables sc ON sc.name = ch.relname
      JOIN pos.synced_tables sp ON sp.name = pa.relname
      WHERE c.contype = 'f' AND c.connamespace = 'pos'::regnamespace AND array_length(c.conkey, 1) = 1
    LOOP
      EXECUTE format(
        'INSERT INTO _pull (tbl, id, seq)
         SELECT DISTINCT %2$L, x.id::text, x.server_seq
         FROM _pull u JOIN pos.%1$I c ON u.tbl = %1$L AND c.id::text = u.id
         JOIN pos.%2$I x ON x.%4$I = c.%3$I
         WHERE x.store_id = $1 AND x.server_seq > $2 AND %5$s
         ON CONFLICT DO NOTHING',
        fk.child, fk.parent, fk.child_col, fk.parent_col,
        pos.not_own_expr(fk.parent, (SELECT kind FROM pos.synced_tables WHERE name = fk.parent)))
      USING me.store_id, since, lim, own_dev;
      GET DIAGNOSTICS n = ROW_COUNT;
      added := added + n;
    END LOOP;
    EXIT WHEN added = 0;
  END LOOP;

  FOR t IN SELECT DISTINCT tbl FROM _pull LOOP
    EXECUTE format(
      'SELECT COALESCE(jsonb_agg(jsonb_build_object(''seq'', x.server_seq, ''table'', %1$L, ''row'', %2$s)), ''[]'')
       FROM pos.%1$I x JOIN _pull u ON u.tbl = %1$L AND u.id = %3$s
       WHERE x.store_id = $1',
      t.tbl, pos.wire_expr(t.tbl), CASE WHEN t.tbl = 'store_settings' THEN 'x.key' ELSE 'x.id::text' END)
    INTO page USING me.store_id;
    out_rows := out_rows || page;
  END LOOP;
  SELECT COALESCE(jsonb_agg(e ORDER BY (e ->> 'seq')::bigint), '[]') INTO out_rows FROM jsonb_array_elements(out_rows) e;

  RETURN jsonb_build_object('rows', out_rows, 'cursor', next_cursor, 'has_more', has_more, 'watermark', watermark);
END $$;

-- ═════════ sync_health: the worker's reachability probe ═════════
CREATE FUNCTION public.sync_health() RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pos, pg_temp AS $$
DECLARE me record; d pos.devices;
BEGIN
  SELECT * INTO me FROM pos.require_device();
  SELECT * INTO d FROM pos.devices WHERE id = me.device_id;
  RETURN jsonb_build_object('ok', true, 'server_time', pos.ts(now()), 'store_id', me.store_id,
    'device_id', me.device_id, 'device_code', d.code,
    'chain_broken', COALESCE((SELECT jsonb_agg(jsonb_build_object('device_code', x.code, 'at', pos.ts(x.chain_broken_at),
                                                                   'seq', x.chain_broken_seq) ORDER BY x.code)
                              FROM pos.devices x WHERE x.store_id = me.store_id AND x.chain_broken_at IS NOT NULL),
                             '[]'::jsonb));
END $$;

-- ═════════ ownership and privileges ═════════
DO $$
DECLARE f text;
BEGIN
  FOREACH f IN ARRAY ARRAY[
    'pos.cols(text)', 'pos.wire_expr(text)', 'pos.not_own_expr(text,text)', 'pos.reject(text,text)',
    'pos.require_device()', 'pos.is_newer(timestamptz,uuid,timestamptz,uuid)', 'pos.upsert_lww(text,text[],jsonb)',
    'pos.insert_ignore(text,jsonb)', 'pos.has_active_owner(uuid)', 'pos.apply_row(uuid,uuid,text,jsonb)'] LOOP
    EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC, anon, authenticated', f);
    EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO pos_sync', f);
  END LOOP;
END $$;
-- A new owner needs CREATE on the function's schema at the moment of the transfer, and only then.
GRANT CREATE ON SCHEMA public TO pos_sync;
ALTER FUNCTION public.sync_push(uuid, jsonb, text) OWNER TO pos_sync;
ALTER FUNCTION public.sync_pull(bigint, int, boolean) OWNER TO pos_sync;
ALTER FUNCTION public.sync_health() OWNER TO pos_sync;
REVOKE CREATE ON SCHEMA public FROM pos_sync;
REVOKE ALL ON FUNCTION public.sync_push(uuid, jsonb, text), public.sync_pull(bigint, int, boolean),
  public.sync_health() FROM PUBLIC, anon;
GRANT EXECUTE ON FUNCTION public.sync_push(uuid, jsonb, text), public.sync_pull(bigint, int, boolean),
  public.sync_health() TO authenticated;
-- The catalogue lookups in sync_pull's closure read pg_catalog, which every role may read.
