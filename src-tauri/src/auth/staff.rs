//! Staff + role lookups used by authentication, and the owner invariant (ARCHITECTURE §3.5).

use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::domain::permissions::{OWNER_ROLE, PermMask, effective};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, FromRow)]
pub struct StaffAuthRow {
    pub id: String,
    pub display_name: String,
    pub pin_hash: String,
    pub is_active: bool,
    pub grant_mask: i64,
    pub deny_mask: i64,
    pub staff_max_discount_bp: Option<i64>,
    pub ui_language: String,
    pub role_name: String,
    pub role_permissions: i64,
    pub role_is_system: bool,
    pub role_max_discount_bp: i64,
    pub role_max_override_bp: i64,
}

impl StaffAuthRow {
    pub fn is_owner_role(&self) -> bool {
        self.role_is_system && self.role_name == OWNER_ROLE
    }

    pub fn effective_perms(&self) -> PermMask {
        effective(
            PermMask(self.role_permissions),
            PermMask(self.grant_mask),
            PermMask(self.deny_mask),
            self.is_owner_role(),
        )
    }

    pub fn max_discount_bp(&self) -> i64 {
        self.staff_max_discount_bp.unwrap_or(self.role_max_discount_bp)
    }
}

pub async fn load(pool: &SqlitePool, staff_id: &str) -> AppResult<StaffAuthRow> {
    sqlx::query_as(
        "SELECT s.id, s.display_name, s.pin_hash, s.is_active, s.grant_mask, s.deny_mask,
                s.max_discount_bp AS staff_max_discount_bp, s.ui_language,
                r.name AS role_name, r.permissions AS role_permissions, r.is_system AS role_is_system,
                r.max_discount_bp AS role_max_discount_bp, r.max_override_bp AS role_max_override_bp
         FROM staff s JOIN roles r ON r.id = s.role_id
         WHERE s.id = ?1 AND s.deleted_at IS NULL",
    )
    .bind(staff_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("staff"))
}

/// Call inside any transaction that disables, deletes or re-roles staff, *after* the change and before
/// commit: there must always be at least one active staff member holding the Owner system role.
pub async fn ensure_owner_invariant(conn: &mut SqliteConnection) -> AppResult<()> {
    let owners: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM staff s JOIN roles r ON r.id = s.role_id
         WHERE s.is_active = 1 AND s.deleted_at IS NULL AND r.is_system = 1 AND r.name = ?1 AND r.deleted_at IS NULL",
    )
    .bind(OWNER_ROLE)
    .fetch_one(&mut *conn)
    .await?;
    if owners == 0 {
        return Err(AppError::invariant("LAST_OWNER", "at least one active Owner must remain"));
    }
    Ok(())
}
