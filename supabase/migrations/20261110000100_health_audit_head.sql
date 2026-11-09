-- GayanShop POS — Block 4.1: sync_health also reports the calling device's audit chain head as the cloud
-- knows it (devices.last_audit_seq / last_audit_hash). A counter restoring a backup online pulls its own
-- post-backup rows back, then checks that its chain ends exactly where the cloud's does before it writes
-- another audit row (doc §8 Phase 4, restore recovery step 3).
CREATE OR REPLACE FUNCTION public.sync_health() RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pos, pg_temp AS $$
DECLARE me record; d pos.devices;
BEGIN
  SELECT * INTO me FROM pos.require_device();
  SELECT * INTO d FROM pos.devices WHERE id = me.device_id;
  RETURN jsonb_build_object('ok', true, 'server_time', pos.ts(now()), 'store_id', me.store_id,
    'device_id', me.device_id, 'device_code', d.code,
    'last_audit_seq', d.last_audit_seq, 'last_audit_hash', d.last_audit_hash,
    'chain_broken', COALESCE((SELECT jsonb_agg(jsonb_build_object('device_code', x.code, 'at', pos.ts(x.chain_broken_at),
                                                                   'seq', x.chain_broken_seq) ORDER BY x.code)
                              FROM pos.devices x WHERE x.store_id = me.store_id AND x.chain_broken_at IS NOT NULL),
                             '[]'::jsonb));
END $$;
