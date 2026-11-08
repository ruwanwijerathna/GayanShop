//! Authentication: PIN login with lockout, supervisor approvals, sessions (ARCHITECTURE §3.5).
//!
//! PINs are verified **before** taking the writer lock (argon2 is deliberately slow); the attempt
//! bookkeeping and audit rows are then written in one transaction.

pub mod approvals;
pub mod guard;
pub mod pin;
pub mod session;
pub mod staff;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::json;
use specta::Type;

use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::clock::{Clock, iso, parse_iso};
use crate::db::Db;
use crate::domain::business_date::{StoreCalendar, format_date};
use crate::domain::permissions::Perm;
use crate::error::{AppError, AppResult};
use approvals::ApprovalStore;
use pin::{LOCKOUT_MINUTES, MAX_FAILED_ATTEMPTS, PinHasher};
use session::{DEFAULT_IDLE_LOCK_MINUTES, Session, SessionInfo, SessionStore};
use staff::StaffAuthRow;

/// The store/device a login or approval happens on (used for audit rows).
#[derive(Debug, Clone)]
pub struct DeviceCtx {
    pub store_id: String,
    pub device_id: String,
    pub calendar: StoreCalendar,
}

/// In-memory authentication state for this terminal.
pub struct Auth {
    pub sessions: SessionStore,
    pub approvals: ApprovalStore,
    pub hasher: PinHasher,
    pub idle_lock: Duration,
}

impl Auth {
    pub fn new(hasher: PinHasher) -> Self {
        Self {
            sessions: SessionStore::default(),
            approvals: ApprovalStore::default(),
            hasher,
            idle_lock: Duration::minutes(DEFAULT_IDLE_LOCK_MINUTES),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ApprovalGrant {
    pub approval_id: String,
    pub expires_at: String,
}

async fn write_audit(db: &Db, dev: &DeviceCtx, now: DateTime<Utc>, entries: Vec<AuditEntry<'_>>) -> AppResult<()> {
    let now_s = iso(now);
    let bd = format_date(dev.calendar.business_date(now));
    let ctx = AuditCtx { store_id: &dev.store_id, device_id: &dev.device_id, business_date: &bd, now: &now_s };
    let mut tx = db.write().await?;
    for e in entries {
        audit::record(tx.conn(), &ctx, e).await?;
    }
    tx.commit().await
}

/// Verify `pin` for `staff` with the lockout policy (5 failures ⇒ locked for 5 minutes on this device).
/// Failures and lockouts are audited; a success clears the counter.
async fn check_pin(
    db: &Db,
    auth: &Auth,
    dev: &DeviceCtx,
    now: DateTime<Utc>,
    staff: &StaffAuthRow,
    pin: &str,
) -> AppResult<()> {
    let attempts: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT failed_count, locked_until FROM auth_attempts WHERE staff_id = ?1")
            .bind(&staff.id)
            .fetch_optional(&db.pool)
            .await?;
    let (mut failed, locked_until) = attempts.unwrap_or((0, None));
    if let Some(until) = locked_until.as_deref().and_then(parse_iso) {
        if now < until {
            return Err(AppError::AccountLocked { until: iso(until) });
        }
        failed = 0; // the lock has expired: start a fresh window
    }
    if auth.hasher.verify(pin, &staff.pin_hash) {
        let mut tx = db.write().await?;
        sqlx::query("DELETE FROM auth_attempts WHERE staff_id = ?1").bind(&staff.id).execute(tx.conn()).await?;
        tx.commit().await?;
        return Ok(());
    }
    failed += 1;
    let lock = failed >= i64::from(MAX_FAILED_ATTEMPTS);
    let until = lock.then(|| now + Duration::minutes(LOCKOUT_MINUTES));
    {
        let mut tx = db.write().await?;
        sqlx::query(
            "INSERT INTO auth_attempts (staff_id, failed_count, locked_until, last_failed_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(staff_id) DO UPDATE SET failed_count = excluded.failed_count,
               locked_until = excluded.locked_until, last_failed_at = excluded.last_failed_at",
        )
        .bind(&staff.id)
        .bind(failed)
        .bind(until.map(iso))
        .bind(iso(now))
        .execute(tx.conn())
        .await?;
        tx.commit().await?;
    }
    let mut entries = vec![AuditEntry {
        action: AuditAction::AUTH_FAILED,
        staff_id: Some(&staff.id),
        approved_by: None,
        entity_type: Some("staff"),
        entity_id: Some(&staff.id),
        metadata: json!({ "failed_count": failed }),
    }];
    if lock {
        entries.push(AuditEntry {
            action: AuditAction::AUTH_LOCKOUT,
            staff_id: Some(&staff.id),
            approved_by: None,
            entity_type: Some("staff"),
            entity_id: Some(&staff.id),
            metadata: json!({ "minutes": LOCKOUT_MINUTES }),
        });
    }
    write_audit(db, dev, now, entries).await?;
    match until {
        Some(until) => Err(AppError::AccountLocked { until: iso(until) }),
        None => Err(AppError::InvalidPin {
            attempts_left: MAX_FAILED_ATTEMPTS.saturating_sub(u32::try_from(failed).unwrap_or(MAX_FAILED_ATTEMPTS)),
        }),
    }
}

/// PIN login on this terminal. Replaces any existing session and clears outstanding approvals.
pub async fn login(
    db: &Db,
    auth: &Auth,
    clock: &dyn Clock,
    dev: &DeviceCtx,
    staff_id: &str,
    pin: &str,
) -> AppResult<SessionInfo> {
    let now = clock.now();
    let staff = staff::load(&db.pool, staff_id).await?;
    if !staff.is_active {
        return Err(AppError::validation("STAFF_INACTIVE", "this staff member is disabled"));
    }
    check_pin(db, auth, dev, now, &staff, pin).await?;
    write_audit(
        db,
        dev,
        now,
        vec![AuditEntry {
            action: AuditAction::AUTH_LOGIN,
            staff_id: Some(&staff.id),
            approved_by: None,
            entity_type: Some("staff"),
            entity_id: Some(&staff.id),
            metadata: json!({ "role": staff.role_name }),
        }],
    )
    .await?;
    let session = Session {
        staff_id: staff.id.clone(),
        display_name: staff.display_name.clone(),
        role_name: staff.role_name.clone(),
        perms: staff.effective_perms(),
        max_discount_bp: staff.max_discount_bp(),
        max_override_bp: staff.role_max_override_bp,
        ui_language: staff.ui_language.clone(),
        logged_in_at: now,
        last_activity: now,
    };
    let info = SessionInfo::from(&session);
    auth.approvals.clear();
    auth.sessions.start(session);
    Ok(info)
}

/// End the session (explicit logout or lock). Audited when someone was logged in.
pub async fn logout(db: &Db, auth: &Auth, clock: &dyn Clock, dev: &DeviceCtx) -> AppResult<()> {
    auth.approvals.clear();
    if let Some(s) = auth.sessions.end() {
        write_audit(
            db,
            dev,
            clock.now(),
            vec![AuditEntry {
                action: AuditAction::AUTH_LOGOUT,
                staff_id: Some(&s.staff_id),
                approved_by: None,
                entity_type: Some("staff"),
                entity_id: Some(&s.staff_id),
                metadata: json!({}),
            }],
        )
        .await?;
    }
    Ok(())
}

/// What a supervisor is asked to approve.
pub struct ApprovalRequest<'a> {
    pub perm: Perm,
    pub target_ref: &'a str,
    pub approver_id: &'a str,
    pub pin: &'a str,
}

/// A supervisor authorises `perm` on `target_ref` for the logged-in staff member (step-up auth).
pub async fn request_approval(
    db: &Db,
    auth: &Auth,
    clock: &dyn Clock,
    dev: &DeviceCtx,
    req: ApprovalRequest<'_>,
) -> AppResult<ApprovalGrant> {
    let ApprovalRequest { perm, target_ref, approver_id, pin } = req;
    let now = clock.now();
    let requester = auth.sessions.touch(now, auth.idle_lock)?;
    if !perm.approvable() {
        return Err(AppError::Forbidden { perm });
    }
    if approver_id == requester.staff_id {
        return Err(AppError::validation("SELF_APPROVAL", "a different staff member must approve"));
    }
    let approver = staff::load(&db.pool, approver_id).await?;
    if !approver.is_active {
        return Err(AppError::validation("STAFF_INACTIVE", "the approver is disabled"));
    }
    check_pin(db, auth, dev, now, &approver, pin).await?;
    let meta = json!({ "perm": perm.key(), "target_ref": target_ref, "requested_by": requester.staff_id });
    if !approver.effective_perms().has(perm) {
        write_audit(
            db,
            dev,
            now,
            vec![AuditEntry {
                action: AuditAction::APPROVAL_DENIED,
                staff_id: Some(&requester.staff_id),
                approved_by: Some(&approver.id),
                entity_type: Some("approval"),
                entity_id: Some(target_ref),
                metadata: meta,
            }],
        )
        .await?;
        return Err(AppError::Forbidden { perm });
    }
    let (approval_id, expires_at) = auth.approvals.issue(perm, target_ref, &requester.staff_id, &approver.id, now);
    write_audit(
        db,
        dev,
        now,
        vec![AuditEntry {
            action: AuditAction::APPROVAL_GRANTED,
            staff_id: Some(&requester.staff_id),
            approved_by: Some(&approver.id),
            entity_type: Some("approval"),
            entity_id: Some(target_ref),
            metadata: meta,
        }],
    )
    .await?;
    Ok(ApprovalGrant { approval_id, expires_at: iso(expires_at) })
}
