//! Block 0.6 — authentication, lockout, idle lock, approvals, guard and the owner invariant.

mod common;

use chrono::{Duration, TimeZone, Utc};
use common::{META, TestDb, exec, open, scalar_i64};
use gayanshop_lib::auth::guard::{Actor, require};
use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::auth::{ApprovalRequest, Auth, DeviceCtx, login, logout, request_approval, staff};
use gayanshop_lib::clock::ManualClock;
use gayanshop_lib::domain::business_date::StoreCalendar;
use gayanshop_lib::domain::permissions::{OWNER_ROLE, Perm, PermMask, default_roles};
use gayanshop_lib::error::AppError;

struct Fixture {
    t: TestDb,
    auth: Auth,
    clock: ManualClock,
    dev: DeviceCtx,
}

async fn fixture() -> Fixture {
    let t = open().await;
    let hasher = PinHasher::fast_for_tests();
    for r in default_roles() {
        exec(t.pool(), &format!(
            "INSERT INTO roles (id, store_id, name, permissions, max_discount_bp, max_override_bp, is_system, created_at, updated_at, updated_by_device) VALUES ('{0}', 's', '{0}', {1}, {2}, {3}, 1, {META})",
            r.name, r.perms.0, r.max_discount_bp, r.max_override_bp
        )).await;
    }
    for (id, role, pin, active) in [
        ("owner", OWNER_ROLE, "246810", 1),
        ("sup", "Supervisor", "1357", 1),
        ("cashier", "Cashier", "1111", 1),
        ("cashier2", "Cashier", "2222", 1),
        ("gone", "Cashier", "3333", 0),
    ] {
        let hash = hasher.hash(pin).unwrap();
        exec(t.pool(), &format!(
            "INSERT INTO staff (id, store_id, display_name, role_id, pin_hash, ui_language, is_active, created_at, updated_at, updated_by_device) VALUES ('{id}', 's', '{id}', '{role}', '{hash}', 'si', {active}, {META})"
        )).await;
    }
    Fixture {
        t,
        auth: Auth::new(hasher),
        clock: ManualClock::new(Utc.with_ymd_and_hms(2026, 11, 8, 4, 0, 0).unwrap()),
        dev: DeviceCtx { store_id: "s".into(), device_id: "dev1".into(), calendar: StoreCalendar::colombo_default() },
    }
}

async fn audit_count(f: &Fixture, action: &str) -> i64 {
    scalar_i64(f.t.pool(), &format!("SELECT COUNT(*) FROM audit_logs WHERE action = '{action}'")).await
}

#[tokio::test]
async fn login_success_builds_the_session_and_audits() {
    let f = fixture().await;
    let info = login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "1111").await.unwrap();
    assert_eq!((info.role_name.as_str(), info.max_discount_bp, info.ui_language.as_str()), ("Cashier", 500, "si"));
    assert!(info.permissions.contains(&Perm::SALE_CREATE) && !info.permissions.contains(&Perm::VOID_ORDER));
    assert_eq!(audit_count(&f, "AUTH_LOGIN").await, 1);
    logout(&f.t.db, &f.auth, &f.clock, &f.dev).await.unwrap();
    assert_eq!(audit_count(&f, "AUTH_LOGOUT").await, 1);
    assert!(f.auth.sessions.peek().is_none());
}

#[tokio::test]
async fn five_wrong_pins_lock_for_five_minutes() {
    let f = fixture().await;
    for left in [4u32, 3, 2, 1] {
        assert_eq!(
            login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "9999").await,
            Err(AppError::InvalidPin { attempts_left: left })
        );
    }
    let locked = login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "9999").await;
    assert!(matches!(locked, Err(AppError::AccountLocked { .. })), "{locked:?}");
    // Even the correct PIN is refused while locked.
    assert!(matches!(
        login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "1111").await,
        Err(AppError::AccountLocked { .. })
    ));
    assert_eq!(audit_count(&f, "AUTH_FAILED").await, 5);
    assert_eq!(audit_count(&f, "AUTH_LOCKOUT").await, 1);
    f.clock.advance(Duration::minutes(5));
    assert!(login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "1111").await.is_ok());
    assert_eq!(scalar_i64(f.t.pool(), "SELECT COUNT(*) FROM auth_attempts").await, 0, "success clears the counter");
    assert!(gayanshop_lib::audit::verify(f.t.pool()).await.unwrap().iter().all(|r| r.ok));
}

#[tokio::test]
async fn disabled_or_unknown_staff_cannot_log_in() {
    let f = fixture().await;
    assert_eq!(
        login(&f.t.db, &f.auth, &f.clock, &f.dev, "gone", "3333").await.unwrap_err().i18n_key(),
        "errors.VALIDATION.STAFF_INACTIVE"
    );
    assert_eq!(login(&f.t.db, &f.auth, &f.clock, &f.dev, "nobody", "1").await, Err(AppError::not_found("staff")));
}

#[tokio::test]
async fn idle_session_locks() {
    let f = fixture().await;
    login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "1111").await.unwrap();
    f.clock.advance(Duration::minutes(4));
    assert!(require(&f.auth, &f.clock, Perm::SALE_CREATE, "", None).is_ok());
    f.clock.advance(Duration::minutes(6));
    assert_eq!(require(&f.auth, &f.clock, Perm::SALE_CREATE, "", None), Err(AppError::SessionLocked));
    assert_eq!(require(&f.auth, &f.clock, Perm::SALE_CREATE, "", None), Err(AppError::NotLoggedIn));
}

#[tokio::test]
async fn guard_distinguishes_forbidden_from_approval_required() {
    let f = fixture().await;
    assert_eq!(require(&f.auth, &f.clock, Perm::SALE_CREATE, "", None), Err(AppError::NotLoggedIn));
    login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "1111").await.unwrap();
    assert_eq!(
        require(&f.auth, &f.clock, Perm::SALE_CREATE, "", None),
        Ok(Actor { staff_id: "cashier".into(), approved_by: None })
    );
    assert_eq!(
        require(&f.auth, &f.clock, Perm::VOID_ORDER, "o1", None),
        Err(AppError::ApprovalRequired { perm: Perm::VOID_ORDER })
    );
    assert_eq!(
        require(&f.auth, &f.clock, Perm::USER_MANAGE, "", None),
        Err(AppError::Forbidden { perm: Perm::USER_MANAGE })
    );
    assert_eq!(require(&f.auth, &f.clock, Perm::VOID_ORDER, "o1", Some("bogus")), Err(AppError::InvalidApproval));
}

#[tokio::test]
async fn supervisor_approval_works_exactly_once_for_its_target() {
    let f = fixture().await;
    login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "1111").await.unwrap();
    let req = |target| ApprovalRequest { perm: Perm::VOID_ORDER, target_ref: target, approver_id: "sup", pin: "1357" };
    let grant = request_approval(&f.t.db, &f.auth, &f.clock, &f.dev, req("o1")).await.unwrap();
    assert_eq!(
        require(&f.auth, &f.clock, Perm::VOID_ORDER, "o1", Some(&grant.approval_id)),
        Ok(Actor { staff_id: "cashier".into(), approved_by: Some("sup".into()) })
    );
    assert_eq!(
        require(&f.auth, &f.clock, Perm::VOID_ORDER, "o1", Some(&grant.approval_id)),
        Err(AppError::InvalidApproval)
    );
    let other = request_approval(&f.t.db, &f.auth, &f.clock, &f.dev, req("o1")).await.unwrap();
    assert_eq!(
        require(&f.auth, &f.clock, Perm::VOID_ORDER, "o2", Some(&other.approval_id)),
        Err(AppError::InvalidApproval)
    );
    let late = request_approval(&f.t.db, &f.auth, &f.clock, &f.dev, req("o3")).await.unwrap();
    f.clock.advance(Duration::seconds(61));
    assert_eq!(
        require(&f.auth, &f.clock, Perm::VOID_ORDER, "o3", Some(&late.approval_id)),
        Err(AppError::InvalidApproval)
    );
    assert_eq!(audit_count(&f, "APPROVAL_GRANTED").await, 3);
}

#[tokio::test]
async fn approval_rules() {
    let f = fixture().await;
    login(&f.t.db, &f.auth, &f.clock, &f.dev, "cashier", "1111").await.unwrap();
    let ask = |perm, approver: &'static str, pin: &'static str| ApprovalRequest {
        perm,
        target_ref: "o1",
        approver_id: approver,
        pin,
    };
    // Admin powers can't be borrowed with a PIN.
    assert_eq!(
        request_approval(&f.t.db, &f.auth, &f.clock, &f.dev, ask(Perm::USER_MANAGE, "owner", "246810")).await,
        Err(AppError::Forbidden { perm: Perm::USER_MANAGE })
    );
    // No self-approval.
    assert_eq!(
        request_approval(&f.t.db, &f.auth, &f.clock, &f.dev, ask(Perm::VOID_ORDER, "cashier", "1111"))
            .await
            .unwrap_err()
            .i18n_key(),
        "errors.VALIDATION.SELF_APPROVAL"
    );
    // A peer without the permission can't approve, and the denial is audited.
    assert_eq!(
        request_approval(&f.t.db, &f.auth, &f.clock, &f.dev, ask(Perm::VOID_ORDER, "cashier2", "2222")).await,
        Err(AppError::Forbidden { perm: Perm::VOID_ORDER })
    );
    assert_eq!(audit_count(&f, "APPROVAL_DENIED").await, 1);
    // A wrong approver PIN counts toward that approver's lockout.
    assert_eq!(
        request_approval(&f.t.db, &f.auth, &f.clock, &f.dev, ask(Perm::VOID_ORDER, "sup", "0000")).await,
        Err(AppError::InvalidPin { attempts_left: 4 })
    );
}

#[tokio::test]
async fn owner_invariant() {
    let f = fixture().await;
    let mut tx = f.t.db.write().await.unwrap();
    sqlx::query("UPDATE staff SET is_active = 0 WHERE id = 'owner'").execute(tx.conn()).await.unwrap();
    assert_eq!(staff::ensure_owner_invariant(tx.conn()).await.unwrap_err().i18n_key(), "errors.INVARIANT.LAST_OWNER");
    drop(tx); // rolled back
    let mut tx = f.t.db.write().await.unwrap();
    sqlx::query(&format!("INSERT INTO staff (id, store_id, display_name, role_id, pin_hash, created_at, updated_at, updated_by_device) VALUES ('owner2', 's', 'O2', '{OWNER_ROLE}', 'h', {META})")).execute(tx.conn()).await.unwrap();
    sqlx::query("UPDATE staff SET is_active = 0 WHERE id = 'owner'").execute(tx.conn()).await.unwrap();
    staff::ensure_owner_invariant(tx.conn()).await.unwrap();
    tx.commit().await.unwrap();
    // The Owner role always has every permission, whatever its deny mask says.
    sqlx::query(&format!("UPDATE staff SET deny_mask = {} WHERE id = 'owner2'", PermMask::all().0))
        .execute(f.t.pool())
        .await
        .unwrap();
    assert_eq!(staff::load(f.t.pool(), "owner2").await.unwrap().effective_perms(), PermMask::all());
}
