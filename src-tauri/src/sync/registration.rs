//! Registering a counter with the cloud (Block 3.3; ARCHITECTURE §1.3, decisions #18–#19).
//!
//! The owner signs in once on the counter (their password goes to the auth server only and is never
//! stored), the register-device function creates the counter's own device identity, and that identity's
//! refresh token goes into the Windows Credential Manager. A counter's ids are adopted, never reassigned:
//!   * [`register_this_counter`]: a counter that already has a store (set up offline at first run) brings
//!     its store_id, device_id and code;
//!   * [`join_store`]: a new counter joins the store the owner's account owns, with a fresh device_id and the
//!     next free code; its history then arrives by bootstrap (Block 3.5).

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;

use super::api;
use super::config::{self, ServerConfig};
use super::credentials::{SecretStore, refresh_token_account};
use super::wire::{RegisterRequest, RegisterResponse};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::clock::{Clock, iso, new_id};
use crate::db::Db;
use crate::db::settings::{self, keys};
use crate::domain::business_date::{StoreCalendar, format_date};
use crate::error::{AppError, AppResult};
use crate::setup::{self, Identity};

/// Set once a joined counter has pulled the store's history (Block 3.5 clears it).
pub const BOOTSTRAP_PENDING_KEY: &str = "sync.bootstrap_pending";

#[derive(Debug, Clone, Deserialize, Type)]
pub struct RegisterInput {
    /// The server's base URL, e.g. `https://<project>.supabase.co`.
    pub server_url: String,
    /// The project's public (anon) key.
    pub anon_key: String,
    pub owner_email: String,
    pub owner_password: String,
    /// Shown in the cloud's device list; defaults to "Counter T0n".
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Registered {
    pub store_id: String,
    pub store_name: String,
    pub device_id: String,
    pub device_code: String,
}

fn clean_email(raw: &str) -> AppResult<String> {
    let email = raw.trim().to_owned();
    if email.len() > 254 || !email.contains('@') || email.contains(char::is_whitespace) {
        return Err(AppError::validation("SYNC_EMAIL_INVALID", email));
    }
    Ok(email)
}

fn clean_device_name(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim).filter(|s| !s.is_empty()).map(|s| s.chars().take(40).collect())
}

/// Sign the owner in and call the register-device function.
async fn call_register(input: &RegisterInput, req: &RegisterRequest) -> AppResult<(ServerConfig, RegisterResponse)> {
    let cfg = ServerConfig::parse(&input.server_url, &input.anon_key)?;
    let email = clean_email(&input.owner_email)?;
    if input.owner_password.is_empty() {
        return Err(AppError::validation("SYNC_PASSWORD_REQUIRED", "enter the owner's cloud password"));
    }
    let http = api::http_client()?;
    let owner_token = api::sign_in_owner(&http, &cfg, &email, &input.owner_password).await?;
    let res = api::register(&http, &cfg, &owner_token, req).await?;
    Ok((cfg, res))
}

/// Register this counter, adopting the store, device id and code it already has. Also re-registers a
/// counter whose identity was lost or revoked (`SYNC_MANAGE` → "Re-register device").
pub async fn register_this_counter(
    db: &Db,
    clock: &dyn Clock,
    secrets: &dyn SecretStore,
    identity: &Identity,
    staff_id: &str,
    input: &RegisterInput,
) -> AppResult<Registered> {
    let req = RegisterRequest {
        store_id: Some(identity.store_id.clone()),
        store_name: Some(identity.store_name.clone()),
        device_id: identity.device_id.clone(),
        device_code: Some(identity.device_code.clone()),
        device_name: clean_device_name(input.device_name.as_deref()),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let (cfg, res) = call_register(input, &req).await?;
    if res.store_id != identity.store_id
        || res.device_id != identity.device_id
        || res.device_code != identity.device_code
    {
        return Err(AppError::invariant(
            "SYNC_IDENTITY_MISMATCH",
            format!("the server answered for {}/{} ({})", res.store_id, res.device_id, res.device_code),
        ));
    }
    secrets.set(&refresh_token_account(&identity.device_id), &res.refresh_token)?;

    let now_dt = clock.now();
    let now = iso(now_dt);
    let bd = format_date(identity.calendar.business_date(now_dt));
    let mut tx = db.write().await?;
    let c = tx.conn();
    config::save(c, &cfg, &now).await?;
    audit::record(
        c,
        &AuditCtx { store_id: &identity.store_id, device_id: &identity.device_id, business_date: &bd, now: &now },
        AuditEntry {
            action: AuditAction::DEVICE_REGISTER,
            staff_id: Some(staff_id),
            approved_by: None,
            entity_type: Some("device"),
            entity_id: Some(&identity.device_id),
            metadata: json!({ "mode": "adopt", "device_code": res.device_code, "server": cfg.url }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Registered {
        store_id: res.store_id,
        store_name: res.store_name,
        device_id: res.device_id,
        device_code: res.device_code,
    })
}

/// First-run "Join existing store" (decision #18): this terminal has no identity yet. It takes a new device
/// id, registers it into the owner's store, and records the identity the server confirms. The store's data,
/// staff included, arrives by bootstrap before anyone can log in.
pub async fn join_store(
    db: &Db,
    clock: &dyn Clock,
    secrets: &dyn SecretStore,
    input: &RegisterInput,
    ui_language: &str,
) -> AppResult<Registered> {
    if setup::is_initialized(&db.pool).await? {
        return Err(AppError::AlreadyInitialized);
    }
    if !matches!(ui_language, "en" | "si") {
        return Err(AppError::validation("LANGUAGE_INVALID", "en or si"));
    }
    let device_id = new_id();
    let req = RegisterRequest {
        store_id: None,
        store_name: None,
        device_id: device_id.clone(),
        device_code: None,
        device_name: clean_device_name(input.device_name.as_deref()),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let (cfg, res) = call_register(input, &req).await?;
    if res.device_id != device_id {
        return Err(AppError::invariant(
            "SYNC_IDENTITY_MISMATCH",
            format!("the server answered for {}", res.device_id),
        ));
    }
    crate::domain::receipt_no::device_digit(&res.device_code)
        .map_err(|_| AppError::invariant("SYNC_IDENTITY_MISMATCH", format!("device code {}", res.device_code)))?;
    secrets.set(&refresh_token_account(&device_id), &res.refresh_token)?;

    let now_dt = clock.now();
    let now = iso(now_dt);
    let bd = format_date(StoreCalendar::colombo_default().business_date(now_dt));
    let mut tx = db.write().await?;
    if setup::is_initialized(&db.pool).await? {
        return Err(AppError::AlreadyInitialized);
    }
    let c = tx.conn();
    settings::device_set(c, keys::DEVICE_ID, &device_id).await?;
    settings::device_set(c, keys::DEVICE_CODE, &res.device_code).await?;
    settings::device_set(c, keys::STORE_ID, &res.store_id).await?;
    setup::write_device_defaults(c, ui_language).await?;
    settings::device_set(c, BOOTSTRAP_PENDING_KEY, &true).await?;
    config::save(c, &cfg, &now).await?;
    audit::record(
        c,
        &AuditCtx { store_id: &res.store_id, device_id: &device_id, business_date: &bd, now: &now },
        AuditEntry {
            action: AuditAction::DEVICE_REGISTER,
            staff_id: None,
            approved_by: None,
            entity_type: Some("device"),
            entity_id: Some(&device_id),
            metadata: json!({ "mode": "join", "device_code": res.device_code, "server": cfg.url }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Registered {
        store_id: res.store_id,
        store_name: res.store_name,
        device_id: res.device_id,
        device_code: res.device_code,
    })
}

/// Whether this counter has a server and a device identity to sync with.
pub async fn is_registered(db: &Db, secrets: &dyn SecretStore) -> AppResult<bool> {
    let Some(device_id) = settings::device_get::<String>(&db.pool, keys::DEVICE_ID).await? else {
        return Ok(false);
    };
    Ok(config::load(&db.pool).await?.is_some() && secrets.get(&refresh_token_account(&device_id))?.is_some())
}
