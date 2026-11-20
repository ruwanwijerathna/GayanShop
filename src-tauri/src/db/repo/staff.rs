//! Staff management (doc §3.5; `USER_MANAGE`, Owner only by default): create staff with a PIN, assign a role,
//! reset a PIN, set the UI language, per-person grant / deny overrides and discount limit, enable / disable.
//! Every change is audited, and the **owner invariant** holds after each one: at least one active staff
//! member keeps the Owner system role. `pin_hash` never leaves this module.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::auth::pin::{PinHasher, validate_pin};
use crate::auth::staff::ensure_owner_invariant;
use crate::clock::new_id;
use crate::db::{Db, outbox};
use crate::domain::permissions::{OWNER_ROLE, Perm, PermMask, effective, min_pin_len};
use crate::error::{AppError, AppResult};

const NAME_MAX: usize = 40;

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub is_system: bool,
    pub permissions: Vec<Perm>,
    #[specta(type = specta_typescript::Number)]
    pub max_discount_bp: i64,
    #[specta(type = specta_typescript::Number)]
    pub max_override_bp: i64,
    /// Minimum PIN length for this role (6 for Manager and Owner, else 4).
    #[specta(type = specta_typescript::Number)]
    pub min_pin_len: i64,
}

/// A staff member as the back office sees them (never the PIN hash).
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct StaffMember {
    pub id: String,
    pub display_name: String,
    pub role_id: String,
    pub role_name: String,
    pub is_owner: bool,
    pub is_active: bool,
    pub ui_language: String,
    pub grants: Vec<Perm>,
    pub denies: Vec<Perm>,
    /// Personal discount limit; `None` = the role's.
    #[specta(type = Option<specta_typescript::Number>)]
    pub max_discount_bp: Option<i64>,
    /// What they can actually do: `(role | grants) & ~denies`, or everything for an Owner.
    pub effective: Vec<Perm>,
    /// The only active Owner: can't be disabled or moved to another role (owner invariant).
    pub last_owner: bool,
    /// The small number (1, 2, 3 …) the cashier types at the till to credit a sale; never reused.
    #[specta(type = Option<specta_typescript::Number>)]
    pub staff_no: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct StaffInput {
    pub display_name: String,
    pub role_id: String,
    /// `en` or `si`.
    pub ui_language: String,
    pub grants: Vec<Perm>,
    pub denies: Vec<Perm>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub max_discount_bp: Option<i64>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct NewStaff {
    pub staff: StaffInput,
    pub pin: String,
}

#[derive(FromRow)]
struct RoleRow {
    id: String,
    name: String,
    is_system: bool,
    permissions: i64,
    max_discount_bp: i64,
    max_override_bp: i64,
}

#[derive(FromRow)]
struct StaffRow {
    id: String,
    display_name: String,
    role_id: String,
    role_name: String,
    role_is_system: bool,
    role_permissions: i64,
    is_active: bool,
    ui_language: String,
    grant_mask: i64,
    deny_mask: i64,
    max_discount_bp: Option<i64>,
    staff_no: Option<i64>,
}

const STAFF_SELECT: &str = "SELECT s.id, s.display_name, s.role_id, r.name AS role_name, r.is_system AS role_is_system,
        r.permissions AS role_permissions, s.is_active, s.ui_language, s.grant_mask, s.deny_mask, s.max_discount_bp,
        s.staff_no
     FROM staff s JOIN roles r ON r.id = s.role_id WHERE s.deleted_at IS NULL";

fn is_owner_role(is_system: bool, name: &str) -> bool {
    is_system && name == OWNER_ROLE
}

fn to_member(r: StaffRow, active_owners: i64) -> StaffMember {
    let owner = is_owner_role(r.role_is_system, &r.role_name);
    let eff = effective(PermMask(r.role_permissions), PermMask(r.grant_mask), PermMask(r.deny_mask), owner);
    StaffMember {
        last_owner: owner && r.is_active && active_owners <= 1,
        id: r.id,
        display_name: r.display_name,
        role_id: r.role_id,
        role_name: r.role_name,
        is_owner: owner,
        is_active: r.is_active,
        ui_language: r.ui_language,
        grants: PermMask(r.grant_mask).list(),
        denies: PermMask(r.deny_mask).list(),
        max_discount_bp: r.max_discount_bp,
        effective: eff.list(),
        staff_no: r.staff_no,
    }
}

async fn active_owners(conn: &mut SqliteConnection) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM staff s JOIN roles r ON r.id = s.role_id
         WHERE s.is_active = 1 AND s.deleted_at IS NULL AND r.is_system = 1 AND r.name = ?1",
    )
    .bind(OWNER_ROLE)
    .fetch_one(&mut *conn)
    .await?)
}

pub async fn roles(pool: &SqlitePool) -> AppResult<Vec<Role>> {
    let rows: Vec<RoleRow> = sqlx::query_as(
        "SELECT id, name, is_system, permissions, max_discount_bp, max_override_bp FROM roles
         WHERE deleted_at IS NULL ORDER BY is_system DESC, name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Role {
            permissions: if is_owner_role(r.is_system, &r.name) { PermMask::all() } else { PermMask(r.permissions) }
                .list(),
            min_pin_len: i64::try_from(min_pin_len(&r.name)).unwrap_or(4),
            id: r.id,
            name: r.name,
            is_system: r.is_system,
            max_discount_bp: r.max_discount_bp,
            max_override_bp: r.max_override_bp,
        })
        .collect())
}

pub async fn list(pool: &SqlitePool) -> AppResult<Vec<StaffMember>> {
    let mut conn = pool.acquire().await?;
    let owners = active_owners(&mut conn).await?;
    let rows: Vec<StaffRow> =
        sqlx::query_as(&format!("{STAFF_SELECT} ORDER BY s.is_active DESC, s.display_name COLLATE NOCASE"))
            .fetch_all(&mut *conn)
            .await?;
    Ok(rows.into_iter().map(|r| to_member(r, owners)).collect())
}

async fn member(conn: &mut SqliteConnection, id: &str) -> AppResult<StaffMember> {
    let owners = active_owners(conn).await?;
    let row: StaffRow = sqlx::query_as(&format!("{STAFF_SELECT} AND s.id = ?1"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::not_found("staff"))?;
    Ok(to_member(row, owners))
}

/// Checked input: name, language, the role (and who may hand out the Owner role), overrides, limit.
struct Checked {
    name: String,
    role: RoleRow,
    grant: PermMask,
    deny: PermMask,
}

async fn check(conn: &mut SqliteConnection, actor_is_owner: bool, input: &StaffInput) -> AppResult<Checked> {
    let name = input.display_name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(AppError::validation("STAFF_NAME_INVALID", format!("1–{NAME_MAX} characters")));
    }
    if !matches!(input.ui_language.as_str(), "en" | "si") {
        return Err(AppError::validation("LANGUAGE_INVALID", input.ui_language.clone()));
    }
    let role: RoleRow = sqlx::query_as(
        "SELECT id, name, is_system, permissions, max_discount_bp, max_override_bp FROM roles
         WHERE id = ?1 AND deleted_at IS NULL",
    )
    .bind(&input.role_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| AppError::not_found("role"))?;
    if is_owner_role(role.is_system, &role.name) && !actor_is_owner {
        return Err(AppError::validation("OWNER_ROLE_OWNER_ONLY", "only an Owner can make someone an Owner"));
    }
    let grant = PermMask::of(&input.grants);
    let deny = PermMask::of(&input.denies);
    if grant.0 & deny.0 != 0 {
        return Err(AppError::validation("OVERRIDE_CONFLICT", "a permission can't be both granted and denied"));
    }
    if (grant.has(Perm::USER_MANAGE) || deny.has(Perm::USER_MANAGE)) && !actor_is_owner {
        return Err(AppError::validation("OWNER_ROLE_OWNER_ONLY", "only an Owner can change USER_MANAGE"));
    }
    if input.max_discount_bp.is_some_and(|bp| !(0..=10_000).contains(&bp)) {
        return Err(AppError::validation("DISCOUNT_LIMIT_INVALID", "0–100%"));
    }
    Ok(Checked { name, role, grant, deny })
}

async fn actor_is_owner(conn: &mut SqliteConnection, actor: &Actor) -> AppResult<bool> {
    let row: Option<(bool, String)> = sqlx::query_as(
        "SELECT r.is_system, r.name FROM staff s JOIN roles r ON r.id = s.role_id
         WHERE s.id = ?1 AND s.is_active = 1 AND s.deleted_at IS NULL",
    )
    .bind(&actor.staff_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.is_some_and(|(sys, name)| is_owner_role(sys, &name)))
}

fn audit_ctx(ctx: &WriteCtx) -> AuditCtx<'_> {
    AuditCtx { store_id: &ctx.store_id, device_id: &ctx.device_id, business_date: &ctx.business_date, now: &ctx.now }
}

/// The audit picture of a member (no PIN material, ever).
fn audit_view(m: &StaffMember) -> serde_json::Value {
    json!({ "display_name": m.display_name, "role": m.role_name, "is_active": m.is_active,
            "ui_language": m.ui_language, "grants": m.grants, "denies": m.denies,
            "max_discount_bp": m.max_discount_bp, "staff_no": m.staff_no })
}

pub async fn create(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    hasher: &PinHasher,
    input: &NewStaff,
) -> AppResult<StaffMember> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let owner = actor_is_owner(conn, actor).await?;
    let c = check(conn, owner, &input.staff).await?;
    validate_pin(&input.pin, min_pin_len(&c.role.name))?;
    let hash = hasher.hash(&input.pin)?;
    let id = new_id();
    // The next free number; deleted rows still count, so a number is never handed out twice.
    let staff_no: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(staff_no), 0) + 1 FROM staff WHERE store_id = ?1")
        .bind(&ctx.store_id)
        .fetch_one(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO staff (id, store_id, display_name, role_id, pin_hash, grant_mask, deny_mask, max_discount_bp,
                            ui_language, is_active, created_at, updated_at, updated_by_device, staff_no)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12, ?13)",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(&c.name)
    .bind(&c.role.id)
    .bind(&hash)
    .bind(c.grant.0)
    .bind(c.deny.0)
    .bind(input.staff.max_discount_bp)
    .bind(&input.staff.ui_language)
    .bind(input.staff.is_active)
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(staff_no)
    .execute(&mut *conn)
    .await?;
    let m = member(conn, &id).await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::USER_CREATE,
            staff_id: Some(&actor.staff_id),
            approved_by: None,
            entity_type: Some("staff"),
            entity_id: Some(&id),
            metadata: json!({ "after": audit_view(&m) }),
        },
    )
    .await?;
    outbox::enqueue_upsert(conn, "staff", &id, &ctx.now).await?;
    tx.commit().await?;
    Ok(m)
}

pub async fn update(db: &Db, ctx: &WriteCtx, actor: &Actor, id: &str, input: &StaffInput) -> AppResult<StaffMember> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let before = member(conn, id).await?;
    let owner = actor_is_owner(conn, actor).await?;
    if before.is_owner && !owner {
        return Err(AppError::validation("OWNER_ROLE_OWNER_ONLY", "only an Owner can change an Owner"));
    }
    let c = check(conn, owner, input).await?;
    sqlx::query(
        "UPDATE staff SET display_name = ?1, role_id = ?2, grant_mask = ?3, deny_mask = ?4, max_discount_bp = ?5,
                ui_language = ?6, is_active = ?7, updated_at = ?8, updated_by_device = ?9, version = version + 1
         WHERE id = ?10",
    )
    .bind(&c.name)
    .bind(&c.role.id)
    .bind(c.grant.0)
    .bind(c.deny.0)
    .bind(input.max_discount_bp)
    .bind(&input.ui_language)
    .bind(input.is_active)
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    // A disable or demote must leave an active Owner (rolled back with the update otherwise).
    ensure_owner_invariant(conn).await?;
    let after = member(conn, id).await?;
    let action =
        if before.is_active && !after.is_active { AuditAction::USER_DISABLE } else { AuditAction::USER_UPDATE };
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action,
            staff_id: Some(&actor.staff_id),
            approved_by: None,
            entity_type: Some("staff"),
            entity_id: Some(id),
            metadata: json!({ "before": audit_view(&before), "after": audit_view(&after) }),
        },
    )
    .await?;
    outbox::enqueue_upsert(conn, "staff", id, &ctx.now).await?;
    tx.commit().await?;
    Ok(after)
}

/// Set a new PIN (the staff member forgot theirs) and lift any lockout on this counter.
pub async fn reset_pin(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    hasher: &PinHasher,
    id: &str,
    pin: &str,
) -> AppResult<()> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let m = member(conn, id).await?;
    if m.is_owner && !actor_is_owner(conn, actor).await? {
        return Err(AppError::validation("OWNER_ROLE_OWNER_ONLY", "only an Owner can reset an Owner's PIN"));
    }
    validate_pin(pin, min_pin_len(&m.role_name))?;
    let hash = hasher.hash(pin)?;
    sqlx::query(
        "UPDATE staff SET pin_hash = ?1, updated_at = ?2, updated_by_device = ?3, version = version + 1 WHERE id = ?4",
    )
    .bind(&hash)
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM auth_attempts WHERE staff_id = ?1").bind(id).execute(&mut *conn).await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::USER_PIN_RESET,
            staff_id: Some(&actor.staff_id),
            approved_by: None,
            entity_type: Some("staff"),
            entity_id: Some(id),
            metadata: json!({ "display_name": m.display_name }),
        },
    )
    .await?;
    outbox::enqueue_upsert(conn, "staff", id, &ctx.now).await?;
    tx.commit().await
}
