-- GayanShop POS — device registration (Block 3.3, doc §1.3, §8 Phase 3, decisions #18–#19).
-- The register-device edge function calls these with the service role, after it has verified the owner's
-- sign-in. A counter's ids are adopted, never reassigned: the first counter brings the store_id, device_id
-- and device code it generated offline at first run; a joining counter brings its own new device_id and gets
-- the next free code. One owner account owns one store.

-- Returns {store_id, store_name, device_id, device_code, auth_user_id}; auth_user_id is set when this device was
-- registered before (re-registration keeps its row and code and replaces its identity's credentials).
CREATE FUNCTION public.admin_claim_device(
  p_owner uuid, p_store_id uuid, p_store_name text, p_device_id uuid, p_device_code text, p_device_name text,
  p_app_version text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = pos, pg_temp AS $$
DECLARE
  st pos.stores;
  dev pos.devices;
  v_code text;
BEGIN
  IF p_owner IS NULL OR p_device_id IS NULL THEN
    RAISE EXCEPTION 'BAD_REQUEST' USING ERRCODE = '22023';
  END IF;
  IF p_store_id IS NOT NULL THEN
    -- Adopt: the counter already holds a store (created offline at first run).
    SELECT * INTO st FROM pos.stores WHERE id = p_store_id FOR UPDATE;
    IF FOUND AND st.owner_user_id <> p_owner THEN
      RAISE EXCEPTION 'STORE_OWNED_BY_ANOTHER_ACCOUNT' USING ERRCODE = '42501';
    END IF;
    IF NOT FOUND THEN
      IF EXISTS (SELECT 1 FROM pos.stores WHERE owner_user_id = p_owner) THEN
        RAISE EXCEPTION 'ACCOUNT_OWNS_ANOTHER_STORE' USING ERRCODE = '42501';
      END IF;
      IF length(trim(COALESCE(p_store_name, ''))) = 0 THEN
        RAISE EXCEPTION 'BAD_REQUEST' USING ERRCODE = '22023';
      END IF;
      INSERT INTO pos.stores (id, name, owner_user_id) VALUES (p_store_id, trim(p_store_name), p_owner)
      RETURNING * INTO st;
    END IF;
  ELSE
    -- Join: a new counter joins the store this account owns.
    SELECT * INTO st FROM pos.stores WHERE owner_user_id = p_owner FOR UPDATE;
    IF NOT FOUND THEN
      RAISE EXCEPTION 'NO_STORE_FOR_ACCOUNT' USING ERRCODE = 'P0002';
    END IF;
  END IF;

  SELECT * INTO dev FROM pos.devices WHERE id = p_device_id FOR UPDATE;
  IF FOUND THEN
    IF dev.store_id <> st.id THEN
      RAISE EXCEPTION 'DEVICE_IN_ANOTHER_STORE' USING ERRCODE = '42501';
    END IF;
    IF p_device_code IS NOT NULL AND p_device_code <> dev.code THEN
      RAISE EXCEPTION 'DEVICE_CODE_MISMATCH' USING ERRCODE = '23505';
    END IF;
    UPDATE pos.devices SET name = COALESCE(NULLIF(trim(p_device_name), ''), name),
                           app_version = COALESCE(p_app_version, app_version)
    WHERE id = p_device_id RETURNING * INTO dev;
  ELSE
    IF p_device_code IS NOT NULL THEN
      v_code := upper(trim(p_device_code));
      IF EXISTS (SELECT 1 FROM pos.devices d WHERE d.store_id = st.id AND d.code = v_code) THEN
        RAISE EXCEPTION 'DEVICE_CODE_TAKEN' USING ERRCODE = '23505';
      END IF;
    ELSE
      SELECT c INTO v_code FROM unnest(ARRAY['T01','T02','T03','T04','T05','T06','T07','T08','T09']) c
      WHERE c NOT IN (SELECT d.code FROM pos.devices d WHERE d.store_id = st.id) ORDER BY c LIMIT 1;
      IF v_code IS NULL THEN
        RAISE EXCEPTION 'NO_FREE_DEVICE_CODE' USING ERRCODE = '23505';
      END IF;
    END IF;
    INSERT INTO pos.devices (id, store_id, code, name, app_version)
    VALUES (p_device_id, st.id, v_code, COALESCE(NULLIF(trim(p_device_name), ''), 'Counter ' || v_code), p_app_version)
    RETURNING * INTO dev;
  END IF;

  RETURN jsonb_build_object('store_id', st.id, 'store_name', st.name, 'device_id', dev.id, 'device_code', dev.code,
                            'auth_user_id', dev.auth_user_id);
END $$;

-- Bind the device identity (auth.users.id) created by the edge function to the device row.
CREATE FUNCTION public.admin_bind_device(p_device_id uuid, p_auth_user uuid)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path = pos, pg_temp AS $$
BEGIN
  UPDATE pos.devices SET auth_user_id = p_auth_user, registered_at = now() WHERE id = p_device_id;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'DEVICE_NOT_FOUND' USING ERRCODE = 'P0002';
  END IF;
END $$;

REVOKE ALL ON FUNCTION public.admin_claim_device(uuid, uuid, text, uuid, text, text, text),
  public.admin_bind_device(uuid, uuid) FROM PUBLIC, anon, authenticated;
DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'service_role') THEN
    GRANT EXECUTE ON FUNCTION public.admin_claim_device(uuid, uuid, text, uuid, text, text, text),
      public.admin_bind_device(uuid, uuid) TO service_role;
  END IF;
END $$;
