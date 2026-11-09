// GayanShop POS: register-device (Block 3.3, doc §1.3, decisions #18 and #19).
//
// The counter signs the owner in itself (GoTrue password grant) and calls this function with the owner's
// access token. The function:
//   1. verifies that token, and refuses device identities;
//   2. claims the store for the owner's account (adopting the counter's offline store_id) or finds the store
//      the account owns (a counter joining), and registers the counter's own device_id with a code;
//   3. creates, or re-keys, the counter's device identity: an auth user whose app_metadata carries
//      {role: 'device', store_id, device_id}. Every RLS policy reads those claims;
//   4. signs that identity in and returns its tokens. The password is random and never leaves this function.
//
// It speaks plain HTTP to GoTrue (/auth/v1) and PostgREST (/rest/v1), with no client library, so a
// self-hosted GoTrue + PostgREST serves it unchanged. The service role key exists only in this function's
// environment, never in the desktop binary.

const SUPABASE_URL = Deno.env.get("SUPABASE_URL") ?? "";
const ANON_KEY = Deno.env.get("SUPABASE_ANON_KEY") ?? "";
const SERVICE_KEY = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";
const DEVICE_EMAIL_DOMAIN = "devices.gayanshop.invalid";

interface RegisterRequest {
  /** Set when the counter already holds a store (first counter, adopting its offline ids). */
  store_id?: string | null;
  store_name?: string | null;
  device_id: string;
  /** Set when adopting; a joining counter gets the next free code. */
  device_code?: string | null;
  device_name?: string | null;
  app_version?: string | null;
}

interface Claim {
  store_id: string;
  store_name: string;
  device_id: string;
  device_code: string;
  auth_user_id: string | null;
}

class Refusal extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(code);
  }
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

function randomSecret(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return btoa(String.fromCharCode(...bytes))
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replaceAll("=", "");
}

async function call(path: string, init: RequestInit & { key: string; bearer?: string }): Promise<Response> {
  const headers = new Headers(init.headers);
  headers.set("apikey", init.key);
  headers.set("authorization", `Bearer ${init.bearer ?? init.key}`);
  if (init.body) headers.set("content-type", "application/json");
  return await fetch(`${SUPABASE_URL}${path}`, { ...init, headers });
}

/** The owner's account, from their access token. */
async function owner(token: string): Promise<{ id: string }> {
  const res = await call("/auth/v1/user", { method: "GET", key: ANON_KEY, bearer: token });
  if (!res.ok) throw new Refusal(401, "OWNER_NOT_SIGNED_IN");
  const user = await res.json();
  if (user?.app_metadata?.role === "device") throw new Refusal(403, "DEVICE_CANNOT_REGISTER");
  if (typeof user?.id !== "string") throw new Refusal(401, "OWNER_NOT_SIGNED_IN");
  return { id: user.id };
}

async function rpc<T>(name: string, args: Record<string, unknown>): Promise<T> {
  const res = await call(`/rest/v1/rpc/${name}`, {
    method: "POST",
    key: SERVICE_KEY,
    body: JSON.stringify(args),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    const code = typeof err?.message === "string" ? err.message : "SERVER_ERROR";
    const status =
      err?.code === "42501" ? 403 : err?.code === "P0002" ? 404 : err?.code === "23505" ? 409 : 400;
    throw new Refusal(status, code);
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : null) as T;
}

/** Create the device identity, or give the existing one a new password (re-registration). */
async function upsertIdentity(claim: Claim, password: string): Promise<string> {
  const email = `${claim.device_id}@${DEVICE_EMAIL_DOMAIN}`;
  const app_metadata = {
    role: "device",
    store_id: claim.store_id,
    device_id: claim.device_id,
    device_code: claim.device_code,
  };
  if (claim.auth_user_id) {
    const res = await call(`/auth/v1/admin/users/${claim.auth_user_id}`, {
      method: "PUT",
      key: SERVICE_KEY,
      body: JSON.stringify({ password, app_metadata }),
    });
    if (res.ok) return claim.auth_user_id;
    if (res.status !== 404) throw new Refusal(502, "IDENTITY_UPDATE_FAILED");
  }
  const res = await call("/auth/v1/admin/users", {
    method: "POST",
    key: SERVICE_KEY,
    body: JSON.stringify({ email, password, email_confirm: true, app_metadata }),
  });
  if (!res.ok) throw new Refusal(502, "IDENTITY_CREATE_FAILED");
  const user = await res.json();
  return user.id as string;
}

async function signIn(deviceId: string, password: string) {
  const res = await call("/auth/v1/token?grant_type=password", {
    method: "POST",
    key: ANON_KEY,
    body: JSON.stringify({ email: `${deviceId}@${DEVICE_EMAIL_DOMAIN}`, password }),
  });
  if (!res.ok) throw new Refusal(502, "IDENTITY_SIGN_IN_FAILED");
  const t = await res.json();
  return { access_token: t.access_token, refresh_token: t.refresh_token, expires_in: t.expires_in };
}

function parse(body: unknown): RegisterRequest {
  const b = body as Partial<RegisterRequest> | null;
  if (!b || typeof b.device_id !== "string" || !UUID.test(b.device_id)) throw new Refusal(400, "BAD_REQUEST");
  if (b.store_id != null && (typeof b.store_id !== "string" || !UUID.test(b.store_id))) {
    throw new Refusal(400, "BAD_REQUEST");
  }
  if (b.device_code != null && !/^T0[1-9]$/.test(String(b.device_code)))
    throw new Refusal(400, "BAD_REQUEST");
  return b as RegisterRequest;
}

Deno.serve(async (req: Request) => {
  try {
    if (req.method !== "POST") throw new Refusal(405, "METHOD_NOT_ALLOWED");
    if (!SUPABASE_URL || !ANON_KEY || !SERVICE_KEY) throw new Refusal(500, "FUNCTION_NOT_CONFIGURED");
    const token = (req.headers.get("authorization") ?? "").replace(/^Bearer\s+/i, "");
    if (!token) throw new Refusal(401, "OWNER_NOT_SIGNED_IN");
    const input = parse(await req.json().catch(() => null));
    const account = await owner(token);
    const claim = await rpc<Claim>("admin_claim_device", {
      p_owner: account.id,
      p_store_id: input.store_id ?? null,
      p_store_name: input.store_name ?? null,
      p_device_id: input.device_id,
      p_device_code: input.device_code ?? null,
      p_device_name: input.device_name ?? null,
      p_app_version: input.app_version ?? null,
    });
    const password = randomSecret();
    const authUserId = await upsertIdentity(claim, password);
    await rpc<null>("admin_bind_device", { p_device_id: claim.device_id, p_auth_user: authUserId });
    const tokens = await signIn(claim.device_id, password);
    return json(200, {
      store_id: claim.store_id,
      store_name: claim.store_name,
      device_id: claim.device_id,
      device_code: claim.device_code,
      ...tokens,
    });
  } catch (e) {
    if (e instanceof Refusal) return json(e.status, { error: e.code });
    return json(500, { error: "SERVER_ERROR" });
  }
});
