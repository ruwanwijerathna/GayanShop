//! First-run setup: device identity + every owner-confirmed seed in ONE transaction (ARCHITECTURE §4.3
//! "Seeds", §11). Phase 3 registration adopts these ids; they are never reassigned.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use sqlx::SqlitePool;

use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::pin::{PinHasher, validate_pin};
use crate::clock::{Clock, iso, new_id};
use crate::db::settings::{self, keys};
use crate::db::{Db, outbox};
use crate::domain::business_date::{StoreCalendar, format_date};
use crate::domain::permissions::{OWNER_ROLE, default_roles, min_pin_len};
use crate::domain::receipt_no::device_digit;
use crate::error::{AppError, AppResult};

/// Receipt footer under the 7-day exchange policy (§11, decision #26). The wording is proposed; the owner confirms it.
pub const RECEIPT_FOOTER: &str = "Exchange within 7 days. Item for item only; no cash refunds. Thank you, come again!";

/// The footer stores were seeded with before exchanges existed; the upgrade replaces it only while it is untouched.
pub const LEGACY_RECEIPT_FOOTER: &str = "All sales are final. No refunds or exchanges. Thank you, come again!";

/// `sales.policy` value under the exchange policy, and the setting for the exchange window.
pub const SALES_POLICY_EXCHANGE: &str = "exchange_7d";
pub const EXCHANGE_WINDOW_KEY: &str = "exchange.window_days";

pub const EXPENSE_CATEGORIES: [&str; 8] =
    ["Rent", "Electricity", "Water", "Wages", "Transport (non-batch)", "Tea & sundries", "Repairs", "Other"];

#[derive(Debug, Clone, Deserialize, Type)]
pub struct SetupInput {
    pub store_name: String,
    pub owner_name: String,
    pub owner_pin: String,
    /// `T01` for the first counter.
    pub device_code: String,
    /// `en` or `si`.
    pub ui_language: String,
}

/// This terminal's identity, loaded at startup once setup has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub store_id: String,
    pub device_id: String,
    pub device_code: String,
    pub store_name: String,
    pub calendar: StoreCalendar,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct SetupResult {
    pub store_id: String,
    pub device_id: String,
    pub device_code: String,
    pub owner_staff_id: String,
}

/// Store settings seeded at first run (§11), in insertion order.
pub fn store_setting_seeds(store_name: &str) -> Vec<(&'static str, Value)> {
    vec![
        (keys::STORE_NAME, json!(store_name)),
        ("store.currency", json!("LKR")),
        ("store.currency_exponent", json!(2)),
        (keys::TIMEZONE, json!("Asia/Colombo")),
        (keys::DAY_CUTOFF, json!("04:00")),
        (keys::WEEK_START, json!("monday")),
        ("tax.inclusive", json!(true)),
        ("tax.default_rate_bp", json!(0)),
        ("discount.cashier_max_bp", json!(500)),
        ("sales.policy", json!(SALES_POLICY_EXCHANGE)),
        (EXCHANGE_WINDOW_KEY, json!(crate::domain::exchange::DEFAULT_WINDOW_DAYS)),
        ("void.max_minutes", Value::Null),
        ("quick_sale.auto_tag_batch", json!(false)),
        ("receipt.footer", json!(RECEIPT_FOOTER)),
        ("receipt.mask_customer_phone", json!(true)),
        ("customer.capture", json!("optional")),
        ("backup.reminder_days", json!(7)),
    ]
}

/// The two built-in label rolls (§7.4): 203 dpi, 3 mm die-cut gap, column gap/margin calibrated later.
pub fn builtin_label_templates() -> Value {
    json!([
        {
            "id": "two_up_50x25", "name": "2-up 50 x 25 mm", "columns": 2,
            "label_w_mm": 50.0, "label_h_mm": 25.0, "col_gap_mm": 2.0, "row_gap_mm": 3.0, "left_margin_mm": 1.5,
            "dpi": 203, "language": "tspl", "symbology": "auto",
            "elements": [
                { "kind": "store_name", "x_mm": 2.0, "y_mm": 3.5,  "w_mm": 46.0, "h_mm": 2.5,  "font_size": 2, "bold": true, "align": "center" },
                { "kind": "barcode",    "x_mm": 3.0, "y_mm": 8.0,  "w_mm": 44.0, "h_mm": 6.5,  "font_size": 1 },
                { "kind": "price",      "x_mm": 2.0, "y_mm": 17.0, "w_mm": 46.0, "h_mm": 4.0,  "font_size": 4, "bold": true, "align": "center" }
            ]
        },
        {
            "id": "three_up_30x16", "name": "3-up 30 x 16 mm", "columns": 3,
            "label_w_mm": 30.0, "label_h_mm": 16.0, "col_gap_mm": 2.0, "row_gap_mm": 3.0, "left_margin_mm": 1.5,
            "dpi": 203, "language": "tspl", "symbology": "code128",
            "elements": [
                { "kind": "store_name", "x_mm": 1.0, "y_mm": 1.5,  "w_mm": 28.0, "h_mm": 1.5, "font_size": 1, "bold": true, "align": "center" },
                { "kind": "barcode",    "x_mm": 1.0, "y_mm": 4.0,  "w_mm": 28.0, "h_mm": 4.5, "font_size": 1 },
                { "kind": "price",      "x_mm": 1.0, "y_mm": 10.5, "w_mm": 28.0, "h_mm": 4.0, "font_size": 4, "bold": true, "align": "center" }
            ]
        }
    ])
}

fn clean_name(raw: &str, code: &str, max: usize) -> AppResult<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > max {
        return Err(AppError::validation(code, format!("must be 1–{max} characters")));
    }
    Ok(name)
}

pub async fn is_initialized(pool: &SqlitePool) -> AppResult<bool> {
    Ok(settings::device_get::<String>(pool, keys::DEVICE_ID).await?.is_some())
}

/// Load this terminal's identity (None before first-run setup).
pub async fn load_identity(pool: &SqlitePool) -> AppResult<Option<Identity>> {
    let (Some(device_id), Some(device_code), Some(store_id)) = (
        settings::device_get::<String>(pool, keys::DEVICE_ID).await?,
        settings::device_get::<String>(pool, keys::DEVICE_CODE).await?,
        settings::device_get::<String>(pool, keys::STORE_ID).await?,
    ) else {
        return Ok(None);
    };
    let tz: String = settings::store_get(pool, keys::TIMEZONE).await?.unwrap_or_else(|| "Asia/Colombo".to_owned());
    let cutoff: String = settings::store_get(pool, keys::DAY_CUTOFF).await?.unwrap_or_else(|| "04:00".to_owned());
    let week: String = settings::store_get(pool, keys::WEEK_START).await?.unwrap_or_else(|| "monday".to_owned());
    let calendar = StoreCalendar::parse(&tz, &cutoff, &week)
        .ok_or_else(|| AppError::invariant("BAD_CALENDAR_SETTINGS", format!("{tz} / {cutoff} / {week}")))?;
    let store_name: String = settings::store_get(pool, keys::STORE_NAME).await?.unwrap_or_default();
    Ok(Some(Identity { store_id, device_id, device_code, store_name, calendar }))
}

/// Bring a store created before exchanges (schema v5) onto the exchange policy, once. Idempotent: it changes
/// nothing when the policy is already set. The footer is replaced only while it still holds the old verbatim
/// text, so an owner's own wording is kept. Roles gain EXCHANGE_PROCESS / EXCHANGE_LATE where the defaults grant
/// them (bits are only ever added). Settings and roles go to the outbox; one audit row records the upgrade.
pub async fn upgrade_exchange_policy(db: &Db, clock: &dyn Clock, identity: &Identity) -> AppResult<bool> {
    let policy: Option<String> = settings::store_get(&db.pool, "sales.policy").await?;
    if policy.as_deref() == Some(SALES_POLICY_EXCHANGE) {
        return Ok(false);
    }
    let footer: Option<String> = settings::store_get(&db.pool, "receipt.footer").await?;
    let window: Option<i64> = settings::store_get(&db.pool, EXCHANGE_WINDOW_KEY).await?;

    let now_dt = clock.now();
    let now = iso(now_dt);
    let bd = format_date(identity.calendar.business_date(now_dt));
    let mut tx = db.write().await?;
    let c = tx.conn();
    let (store, device) = (identity.store_id.as_str(), identity.device_id.as_str());

    settings::store_set(c, store, device, &now, "sales.policy", &json!(SALES_POLICY_EXCHANGE)).await?;
    if window.is_none() {
        let days = crate::domain::exchange::DEFAULT_WINDOW_DAYS;
        settings::store_set(c, store, device, &now, EXCHANGE_WINDOW_KEY, &json!(days)).await?;
    }
    let footer_replaced = footer.as_deref().is_none_or(|f| f == LEGACY_RECEIPT_FOOTER);
    if footer_replaced {
        settings::store_set(c, store, device, &now, "receipt.footer", &json!(RECEIPT_FOOTER)).await?;
    }

    let mut roles_changed: Vec<&str> = Vec::new();
    for seed in default_roles() {
        let bits = seed.perms.0 & EXCHANGE_MASK;
        if bits == 0 {
            continue;
        }
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM roles WHERE store_id = ?1 AND name = ?2 AND is_system = 1 AND deleted_at IS NULL
               AND (permissions & ?3) != ?3",
        )
        .bind(store)
        .bind(seed.name)
        .bind(bits)
        .fetch_optional(&mut *c)
        .await?;
        let Some(id) = id else { continue };
        sqlx::query(
            "UPDATE roles SET permissions = permissions | ?2, updated_at = ?3, updated_by_device = ?4,
               version = version + 1 WHERE id = ?1",
        )
        .bind(&id)
        .bind(bits)
        .bind(&now)
        .bind(device)
        .execute(&mut *c)
        .await?;
        outbox::enqueue_upsert(c, "roles", &id, &now).await?;
        roles_changed.push(seed.name);
    }

    audit::record(
        c,
        &AuditCtx { store_id: store, device_id: device, business_date: &bd, now: &now },
        AuditEntry {
            action: AuditAction::POLICY_UPGRADED,
            staff_id: None,
            approved_by: None,
            entity_type: Some("store"),
            entity_id: Some(store),
            metadata: json!({ "policy": SALES_POLICY_EXCHANGE, "footer_replaced": footer_replaced, "roles": roles_changed }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// EXCHANGE_PROCESS (bit 6) | EXCHANGE_LATE (bit 7).
const EXCHANGE_MASK: i64 = (1 << 6) | (1 << 7);

const META_COLS: &str = "created_at, updated_at, updated_by_device";

/// This terminal's local preferences at setup: language, theme, idle lock and the label templates (203 dpi,
/// 2 mm gap). Shared by first run and by a counter joining an existing store (Block 3.3).
pub async fn write_device_defaults(c: &mut sqlx::SqliteConnection, ui_language: &str) -> AppResult<()> {
    settings::device_set(c, "ui.default_language", &ui_language).await?;
    settings::device_set(c, "ui.theme", &"light").await?;
    settings::device_set(c, keys::IDLE_LOCK_MINUTES, &5).await?;
    settings::device_set(c, "label.templates", &builtin_label_templates()).await
}

/// Run first-run setup. Fails with `AlreadyInitialized` if this terminal already has an identity.
pub async fn first_run(db: &Db, hasher: &PinHasher, clock: &dyn Clock, input: &SetupInput) -> AppResult<SetupResult> {
    let store_name = clean_name(&input.store_name, "STORE_NAME_INVALID", 80)?;
    let owner_name = clean_name(&input.owner_name, "OWNER_NAME_INVALID", 60)?;
    let device_code = input.device_code.trim().to_ascii_uppercase();
    device_digit(&device_code).map_err(|_| AppError::validation("DEVICE_CODE_INVALID", "use T01–T09"))?;
    if !matches!(input.ui_language.as_str(), "en" | "si") {
        return Err(AppError::validation("LANGUAGE_INVALID", "en or si"));
    }
    validate_pin(&input.owner_pin, min_pin_len(OWNER_ROLE))?;
    let pin_hash = hasher.hash(&input.owner_pin)?;

    let now_dt = clock.now();
    let now = iso(now_dt);
    let calendar = StoreCalendar::colombo_default();
    let bd = format_date(calendar.business_date(now_dt));
    let store_id = new_id();
    let device_id = new_id();

    let mut tx = db.write().await?;
    if settings::device_get::<String>(&db.pool, keys::DEVICE_ID).await?.is_some() {
        return Err(AppError::AlreadyInitialized);
    }
    let c = tx.conn();

    // Device-local identity and preferences.
    settings::device_set(c, keys::DEVICE_ID, &device_id).await?;
    settings::device_set(c, keys::DEVICE_CODE, &device_code).await?;
    settings::device_set(c, keys::STORE_ID, &store_id).await?;
    write_device_defaults(c, &input.ui_language).await?;

    // Synced store settings.
    for (key, value) in store_setting_seeds(&store_name) {
        settings::store_set(c, &store_id, &device_id, &now, key, &value).await?;
    }

    // Roles.
    let mut owner_role_id = String::new();
    for r in default_roles() {
        let id = new_id();
        sqlx::query(&format!(
            "INSERT INTO roles (id, store_id, name, permissions, max_discount_bp, max_override_bp, is_system, {META_COLS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7, ?8)"
        ))
        .bind(&id)
        .bind(&store_id)
        .bind(r.name)
        .bind(r.perms.0)
        .bind(r.max_discount_bp)
        .bind(r.max_override_bp)
        .bind(&now)
        .bind(&device_id)
        .execute(&mut *c)
        .await?;
        outbox::enqueue_upsert(c, "roles", &id, &now).await?;
        if r.name == OWNER_ROLE {
            owner_role_id.clone_from(&id);
        }
    }

    // The Owner.
    let owner_id = new_id();
    sqlx::query(&format!(
        "INSERT INTO staff (id, store_id, display_name, role_id, pin_hash, ui_language, staff_no, {META_COLS})
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7, ?8)"
    ))
    .bind(&owner_id)
    .bind(&store_id)
    .bind(&owner_name)
    .bind(&owner_role_id)
    .bind(&pin_hash)
    .bind(&input.ui_language)
    .bind(&now)
    .bind(&device_id)
    .execute(&mut *c)
    .await?;
    outbox::enqueue_upsert(c, "staff", &owner_id, &now).await?;

    // Expense categories.
    for name in EXPENSE_CATEGORIES {
        let id = new_id();
        sqlx::query(&format!(
            "INSERT INTO expense_categories (id, store_id, name, {META_COLS}) VALUES (?1, ?2, ?3, ?4, ?4, ?5)"
        ))
        .bind(&id)
        .bind(&store_id)
        .bind(name)
        .bind(&now)
        .bind(&device_id)
        .execute(&mut *c)
        .await?;
        outbox::enqueue_upsert(c, "expense_categories", &id, &now).await?;
    }

    // System batches: OPENING-STOCK (draft until go-live) and ADJUSTMENTS (received, takes layers).
    for (code, name, received) in
        [("OPENING-STOCK", "Opening stock", false), ("ADJUSTMENTS", "Stock adjustments", true)]
    {
        let id = new_id();
        sqlx::query(&format!(
            "INSERT INTO batches (id, store_id, code, name, purchased_at, received_at, received_business_date,
               allocation_method, status, is_system, received_by, {META_COLS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'manual', ?8, 1, ?9, ?5, ?5, ?10)"
        ))
        .bind(&id)
        .bind(&store_id)
        .bind(code)
        .bind(name)
        .bind(&now)
        .bind(received.then_some(&now))
        .bind(received.then_some(&bd))
        .bind(if received { "received" } else { "draft" })
        .bind(received.then_some(&owner_id))
        .bind(&device_id)
        .execute(&mut *c)
        .await?;
        outbox::enqueue_upsert(c, "batches", &id, &now).await?;
    }

    audit::record(
        c,
        &AuditCtx { store_id: &store_id, device_id: &device_id, business_date: &bd, now: &now },
        AuditEntry {
            action: AuditAction::STORE_INITIALIZED,
            staff_id: Some(&owner_id),
            approved_by: None,
            entity_type: Some("store"),
            entity_id: Some(&store_id),
            metadata: json!({ "store_name": store_name, "device_code": device_code }),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(SetupResult { store_id, device_id, device_code, owner_staff_id: owner_id })
}
