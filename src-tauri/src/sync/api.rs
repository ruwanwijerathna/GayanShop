//! The sync server's HTTP API (ARCHITECTURE §1.3, §6): plain HTTP + JSON against
//!   * `POST /rest/v1/rpc/{sync_push|sync_pull|sync_health}`: PostgREST in front of the Postgres RPCs;
//!   * `POST /auth/v1/token`: GoTrue, for the owner's one-time sign-in and the device token refresh;
//!   * `POST /functions/v1/register-device`: the registration function.
//!
//! Nothing here is specific to supabase.co: pointing [`ServerConfig`] at a self-hosted PostgREST + GoTrue is
//! the whole migration. The worker talks to the server only through [`SyncApi`], so tests drive it with an
//! in-process fake.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::config::ServerConfig;
use super::credentials::{SecretStore, refresh_token_account};
use super::wire::{Health, OpResult, PullPage, PushOp, PushResponse, RegisterRequest, RegisterResponse};
use crate::error::AppError;

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why a call to the server failed. Only `Offline` is retried with backoff; the rest need a person.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    /// No answer, a timeout, or a 5xx: the network or the server is down. Try again later.
    #[error("server unreachable: {0}")]
    Offline(String),
    /// The device identity is missing, revoked or expired beyond refresh: re-register the counter.
    #[error("not authorised: {0}")]
    Unauthorized(String),
    /// The server understood and refused (a 4xx with a reason code).
    #[error("refused ({status}): {code}")]
    Refused { status: u16, code: String },
    /// The answer wasn't what the protocol promises.
    #[error("unexpected response: {0}")]
    Protocol(String),
}

impl SyncError {
    /// The stable code shown in Settings → Sync and stored in `sync_logs.error_code`.
    pub fn code(&self) -> String {
        match self {
            SyncError::Offline(_) => "OFFLINE".to_owned(),
            SyncError::Unauthorized(_) => "UNAUTHORIZED".to_owned(),
            SyncError::Refused { code, .. } => code.clone(),
            SyncError::Protocol(_) => "PROTOCOL".to_owned(),
        }
    }
}

impl From<SyncError> for AppError {
    fn from(e: SyncError) -> Self {
        let code = match &e {
            SyncError::Offline(_) => "SYNC_OFFLINE".to_owned(),
            SyncError::Unauthorized(_) => "SYNC_UNAUTHORIZED".to_owned(),
            SyncError::Refused { code, .. } => format!("SYNC_{code}"),
            SyncError::Protocol(_) => "SYNC_PROTOCOL".to_owned(),
        };
        AppError::validation(&code, e.to_string())
    }
}

/// What the sync worker needs from the server.
pub trait SyncApi: Send + Sync {
    fn health(&self) -> BoxFut<'_, Result<Health, SyncError>>;
    fn push<'a>(&'a self, device_id: &'a str, ops: &'a [PushOp]) -> BoxFut<'a, Result<Vec<OpResult>, SyncError>>;
    fn pull(&self, cursor: i64, max_rows: i64, include_own: bool) -> BoxFut<'_, Result<PullPage, SyncError>>;
}

pub fn http_client() -> Result<reqwest::Client, SyncError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("GayanShop-POS/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| SyncError::Protocol(format!("http client: {e}")))
}

#[derive(Deserialize)]
struct TokenGrant {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

/// The reason code in an error body: PostgREST's `message`, the function's `error`, GoTrue's `error_code`.
fn reason_of(body: &Value) -> String {
    ["message", "error", "error_code", "msg"]
        .iter()
        .find_map(|k| body.get(*k).and_then(Value::as_str))
        .map(|s| s.chars().take(200).collect())
        .unwrap_or_else(|| "UNKNOWN".to_owned())
}

async fn send(req: reqwest::RequestBuilder) -> Result<(u16, Value), SyncError> {
    let res = req.send().await.map_err(|e| SyncError::Offline(e.without_url().to_string()))?;
    let status = res.status().as_u16();
    let text = res.text().await.map_err(|e| SyncError::Offline(e.without_url().to_string()))?;
    let body = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|_| json!({ "message": text.chars().take(200).collect::<String>() }))
    };
    if status >= 500 {
        return Err(SyncError::Offline(format!("{status}: {}", reason_of(&body))));
    }
    Ok((status, body))
}

fn decode<T: DeserializeOwned>(body: Value) -> Result<T, SyncError> {
    serde_json::from_value(body).map_err(|e| SyncError::Protocol(e.to_string()))
}

/// The owner signs in once, on this counter, to authorise its registration. The password goes straight to
/// GoTrue and is never stored; the short-lived token is used for the one registration call.
pub async fn sign_in_owner(
    http: &reqwest::Client,
    cfg: &ServerConfig,
    email: &str,
    password: &str,
) -> Result<String, SyncError> {
    let req = http
        .post(format!("{}/auth/v1/token?grant_type=password", cfg.url))
        .header("apikey", &cfg.anon_key)
        .json(&json!({ "email": email, "password": password }));
    match send(req).await? {
        (200, body) => Ok(decode::<TokenGrant>(body)?.access_token),
        (400 | 401 | 403 | 422, _) => Err(SyncError::Refused { status: 401, code: "INVALID_CREDENTIALS".to_owned() }),
        (status, body) => Err(SyncError::Refused { status, code: reason_of(&body) }),
    }
}

/// Call the register-device function with the owner's token.
pub async fn register(
    http: &reqwest::Client,
    cfg: &ServerConfig,
    owner_token: &str,
    req: &RegisterRequest,
) -> Result<RegisterResponse, SyncError> {
    let call = http
        .post(format!("{}/functions/v1/register-device", cfg.url))
        .header("apikey", &cfg.anon_key)
        .bearer_auth(owner_token)
        .json(req);
    match send(call).await? {
        (200, body) => decode(body),
        (401, _) => Err(SyncError::Refused { status: 401, code: "INVALID_CREDENTIALS".to_owned() }),
        (status, body) => Err(SyncError::Refused { status, code: reason_of(&body) }),
    }
}

/// One refresh at a time in this process. GoTrue rotates refresh tokens and treats a reused one as theft, so
/// two connections (the worker, a bootstrap) must never refresh from the same token: the second waits here
/// and then reads the already rotated token from the keyring.
static REFRESH_LOCK: Mutex<()> = Mutex::const_new(());

/// The device identity's access token, refreshed from the keyring's refresh token when it nears expiry.
/// GoTrue rotates refresh tokens: the new one is written to the keyring before the access token is used.
struct Tokens {
    secrets: Arc<dyn SecretStore>,
    account: String,
    cached: Mutex<Option<(String, Instant)>>,
}

impl Tokens {
    async fn access(&self, http: &reqwest::Client, cfg: &ServerConfig) -> Result<String, SyncError> {
        let mut cached = self.cached.lock().await;
        if let Some((token, until)) = cached.as_ref()
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let _one_at_a_time = REFRESH_LOCK.lock().await;
        let refresh = self
            .secrets
            .get(&self.account)
            .map_err(|e| SyncError::Protocol(e.to_string()))?
            .ok_or_else(|| SyncError::Unauthorized("NOT_REGISTERED".to_owned()))?;
        let req = http
            .post(format!("{}/auth/v1/token?grant_type=refresh_token", cfg.url))
            .header("apikey", &cfg.anon_key)
            .json(&json!({ "refresh_token": refresh }));
        let grant: TokenGrant = match send(req).await? {
            (200, body) => decode(body)?,
            (400 | 401 | 403, body) => return Err(SyncError::Unauthorized(reason_of(&body))),
            (status, body) => return Err(SyncError::Refused { status, code: reason_of(&body) }),
        };
        if let Some(next) = grant.refresh_token.as_deref() {
            self.secrets.set(&self.account, next).map_err(|e| SyncError::Protocol(e.to_string()))?;
        }
        // Refresh a minute early, so a token never expires mid-request.
        let life = grant.expires_in.unwrap_or(3600).clamp(60, 86_400);
        let until = Instant::now() + Duration::from_secs(u64::try_from(life - 60).unwrap_or(0));
        *cached = Some((grant.access_token.clone(), until));
        Ok(grant.access_token)
    }

    async fn forget(&self) {
        *self.cached.lock().await = None;
    }
}

/// The real server.
pub struct HttpApi {
    http: reqwest::Client,
    cfg: ServerConfig,
    tokens: Tokens,
}

impl HttpApi {
    pub fn new(cfg: ServerConfig, device_id: &str, secrets: Arc<dyn SecretStore>) -> Result<Self, SyncError> {
        Ok(Self {
            http: http_client()?,
            cfg,
            tokens: Tokens { secrets, account: refresh_token_account(device_id), cached: Mutex::new(None) },
        })
    }

    pub fn config(&self) -> &ServerConfig {
        &self.cfg
    }

    /// Call an RPC; an expired access token is refreshed and the call sent once more.
    async fn rpc<T: DeserializeOwned>(&self, name: &str, args: &Value) -> Result<T, SyncError> {
        for attempt in 0..2 {
            let token = self.tokens.access(&self.http, &self.cfg).await?;
            let req = self
                .http
                .post(format!("{}/rest/v1/rpc/{name}", self.cfg.url))
                .header("apikey", &self.cfg.anon_key)
                .bearer_auth(token)
                .json(args);
            match send(req).await? {
                (200, body) => return decode(body),
                (401, _) if attempt == 0 => self.tokens.forget().await,
                (401 | 403, body) => return Err(SyncError::Unauthorized(reason_of(&body))),
                (status, body) => return Err(SyncError::Refused { status, code: reason_of(&body) }),
            }
        }
        Err(SyncError::Unauthorized("TOKEN_REJECTED".to_owned()))
    }
}

impl SyncApi for HttpApi {
    fn health(&self) -> BoxFut<'_, Result<Health, SyncError>> {
        Box::pin(async move { self.rpc("sync_health", &json!({})).await })
    }

    fn push<'a>(&'a self, device_id: &'a str, ops: &'a [PushOp]) -> BoxFut<'a, Result<Vec<OpResult>, SyncError>> {
        Box::pin(async move {
            let res: PushResponse = self
                .rpc(
                    "sync_push",
                    &json!({ "device_id": device_id, "ops": ops, "app_version": env!("CARGO_PKG_VERSION") }),
                )
                .await?;
            if res.results.len() != ops.len() {
                return Err(SyncError::Protocol(format!("{} results for {} ops", res.results.len(), ops.len())));
            }
            Ok(res.results)
        })
    }

    fn pull(&self, cursor: i64, max_rows: i64, include_own: bool) -> BoxFut<'_, Result<PullPage, SyncError>> {
        Box::pin(async move {
            self.rpc("sync_pull", &json!({ "cursor": cursor, "max_rows": max_rows, "include_own": include_own })).await
        })
    }
}
