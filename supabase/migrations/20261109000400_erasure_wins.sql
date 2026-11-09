-- GayanShop POS — fix for Block 3.2 (found by the Block 3.4 client tests): an erasure wins over any concurrent
-- edit, whichever is newer (doc §6.3). sync_push already refused to re-populate an erased customer, but an
-- erasure older than the cloud's copy was dropped as a stale write, so the customer's phone and name stayed
-- in the cloud and on other counters. A pushed erasure now always applies.

-- The whole-row upsert without the last-write-wins guard (an erasure only).
CREATE FUNCTION pos.upsert_force(tbl text, pk text[], r jsonb) RETURNS void LANGUAGE plpgsql AS $$
DECLARE cols text[] := pos.cols(tbl);
BEGIN
  EXECUTE format(
    'INSERT INTO pos.%1$I (%2$s) SELECT %2$s FROM jsonb_populate_record(NULL::pos.%1$I, $1)
     ON CONFLICT (%3$s) DO UPDATE SET (%2$s) = ROW(%4$s)',
    tbl,
    (SELECT string_agg(quote_ident(c), ', ') FROM unnest(cols) c),
    (SELECT string_agg(quote_ident(c), ', ') FROM unnest(pk) c),
    (SELECT string_agg('EXCLUDED.' || quote_ident(c), ', ') FROM unnest(cols) c))
  USING r;
END $$;
REVOKE ALL ON FUNCTION pos.upsert_force(text, text[], jsonb) FROM PUBLIC, anon, authenticated;
GRANT EXECUTE ON FUNCTION pos.upsert_force(text, text[], jsonb) TO pos_sync;

-- Apply one upsert. Raises P0001 'CODE: detail' to reject it.
CREATE OR REPLACE FUNCTION pos.apply_row(p_store uuid, p_dev uuid, tbl text, r jsonb) RETURNS void LANGUAGE plpgsql AS $$
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
  erasing boolean;
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
  -- An erasure wins over any concurrent edit, whichever is newer (§6.3).
  erasing := tbl = 'customers' AND cur IS NOT NULL AND r ->> 'erased_at' IS NOT NULL AND cur ->> 'erased_at' IS NULL;
  IF NOT erasing AND cur IS NOT NULL AND NOT pos.is_newer(inc_at, inc_dev, (cur ->> 'updated_at')::timestamptz,
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
    IF erasing THEN
      PERFORM pos.upsert_force(tbl, meta.pk, r);
    ELSE
      PERFORM pos.upsert_lww(tbl, meta.pk, r);
    END IF;
  EXCEPTION WHEN check_violation THEN
    PERFORM pos.reject(CASE WHEN tbl = 'batch_items' THEN 'COST_LOCKED'
                            WHEN tbl = 'cash_sessions' THEN 'SESSION_CLOSED' ELSE 'CHECK' END, SQLERRM);
  END;
  IF had_owner AND NOT pos.has_active_owner(p_store) THEN
    PERFORM pos.reject('OWNER_INVARIANT', 'the store must keep an active Owner');
  END IF;
END $$;
