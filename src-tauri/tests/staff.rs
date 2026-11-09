//! Block 1.11 — staff management (doc §3.5): create staff with a PIN, assign a role, reset a PIN, set the
//! UI language, grant / deny overrides (`USER_MANAGE`), and the owner invariant.

mod common;

use common::{CASHIER, SUP, Till, till};
use gayanshop_lib::auth::guard::require;
use gayanshop_lib::auth::login;
use gayanshop_lib::db::repo::staff::{self, NewStaff, StaffInput};
use gayanshop_lib::domain::permissions::Perm;
use gayanshop_lib::error::AppError;

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } | AppError::Invariant { code, .. } => code,
        other => panic!("expected a coded error, got {other:?}"),
    }
}

async fn role(t: &Till, name: &str) -> String {
    staff::roles(t.pool()).await.unwrap().into_iter().find(|r| r.name == name).unwrap().id
}

fn input(name: &str, role_id: &str) -> StaffInput {
    StaffInput {
        display_name: name.into(),
        role_id: role_id.into(),
        ui_language: "si".into(),
        grants: vec![],
        denies: vec![],
        max_discount_bp: None,
        is_active: true,
    }
}

async fn create(t: &Till, name: &str, role_name: &str, pin: &str) -> Result<staff::StaffMember, AppError> {
    let new = NewStaff { staff: input(name, &role(t, role_name).await), pin: pin.into() };
    staff::create(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &new).await
}

#[tokio::test]
async fn the_owner_creates_staff_who_can_log_in_with_their_pin_and_language() {
    let t = till().await;
    let m = create(&t, "  Kasun   Perera ", "Cashier", "4321").await.unwrap();
    assert_eq!(
        (m.display_name.as_str(), m.role_name.as_str(), m.ui_language.as_str()),
        ("Kasun Perera", "Cashier", "si")
    );
    assert!(m.effective.contains(&Perm::SALE_CREATE) && !m.effective.contains(&Perm::VOID_ORDER));

    let s = login(&t.t.db, &t.auth, &t.clock, &t.dev, &m.id, "4321").await.unwrap();
    assert_eq!((s.display_name.as_str(), s.ui_language.as_str()), ("Kasun Perera", "si"));
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'USER_CREATE'").await, 1);
    assert_eq!(
        t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'USER_CREATE' AND metadata LIKE '%argon%'").await,
        0,
        "no PIN material in the audit log"
    );
    assert_eq!(
        t.count(&format!("SELECT COUNT(*) FROM sync_outbox WHERE table_name = 'staff' AND row_id = '{}'", m.id)).await,
        1
    );
}

#[tokio::test]
async fn pins_and_names_are_checked_per_role() {
    let t = till().await;
    assert_eq!(code(create(&t, "A", "Cashier", "123").await.unwrap_err()), "PIN_TOO_SHORT");
    assert_eq!(code(create(&t, "A", "Manager", "1234").await.unwrap_err()), "PIN_TOO_SHORT", "managers need 6");
    assert!(create(&t, "M", "Manager", "123456").await.is_ok());
    assert_eq!(code(create(&t, "A", "Cashier", "12ab").await.unwrap_err()), "PIN_NOT_DIGITS");
    assert_eq!(code(create(&t, "   ", "Cashier", "1234").await.unwrap_err()), "STAFF_NAME_INVALID");
    assert_eq!(code(create(&t, &"x".repeat(41), "Cashier", "1234").await.unwrap_err()), "STAFF_NAME_INVALID");
    let mut bad_lang = NewStaff { staff: input("A", &role(&t, "Cashier").await), pin: "1234".into() };
    bad_lang.staff.ui_language = "fr".into();
    assert_eq!(
        code(staff::create(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &bad_lang).await.unwrap_err()),
        "LANGUAGE_INVALID"
    );
}

#[tokio::test]
async fn roles_overrides_and_limits_change_what_someone_can_do() {
    let t = till().await;
    let m = create(&t, "Ruwan", "Cashier", "5555").await.unwrap();
    let mut edit = input("Ruwan", &m.role_id);
    edit.grants = vec![Perm::VOID_ORDER];
    edit.denies = vec![Perm::QUICK_SALE];
    edit.max_discount_bp = Some(1_000);
    edit.ui_language = "en".into();
    let after = staff::update(&t.t.db, &t.ctx(), &t.owner(), &m.id, &edit).await.unwrap();
    assert!(after.effective.contains(&Perm::VOID_ORDER) && !after.effective.contains(&Perm::QUICK_SALE));
    assert_eq!((after.max_discount_bp, after.ui_language.as_str()), (Some(1_000), "en"));
    let s = login(&t.t.db, &t.auth, &t.clock, &t.dev, &m.id, "5555").await.unwrap();
    assert_eq!(s.max_discount_bp, 1_000);
    assert!(s.permissions.contains(&Perm::VOID_ORDER) && !s.permissions.contains(&Perm::QUICK_SALE));

    // Promotion to Supervisor by role.
    edit.role_id = role(&t, "Supervisor").await;
    edit.grants.clear();
    edit.denies.clear();
    let sup = staff::update(&t.t.db, &t.ctx(), &t.owner(), &m.id, &edit).await.unwrap();
    assert_eq!(sup.role_name, "Supervisor");
    assert!(sup.effective.contains(&Perm::VOID_ORDER));

    edit.grants = vec![Perm::PRICE_OVERRIDE];
    edit.denies = vec![Perm::PRICE_OVERRIDE];
    assert_eq!(
        code(staff::update(&t.t.db, &t.ctx(), &t.owner(), &m.id, &edit).await.unwrap_err()),
        "OVERRIDE_CONFLICT"
    );
    edit.denies.clear();
    edit.max_discount_bp = Some(10_001);
    assert_eq!(
        code(staff::update(&t.t.db, &t.ctx(), &t.owner(), &m.id, &edit).await.unwrap_err()),
        "DISCOUNT_LIMIT_INVALID"
    );
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'USER_UPDATE'").await, 2);
}

#[tokio::test]
async fn the_last_owner_can_never_be_disabled_or_demoted() {
    let t = till().await;
    let owner_id = t.s.owner_staff_id.clone();
    let owners = staff::list(t.pool()).await.unwrap();
    let me = owners.iter().find(|m| m.id == owner_id).unwrap();
    assert!(me.is_owner && me.last_owner, "the UI greys out disable / demote");

    let mut disable = input(&me.display_name, &me.role_id);
    disable.is_active = false;
    assert_eq!(
        code(staff::update(&t.t.db, &t.ctx(), &t.owner(), &owner_id, &disable).await.unwrap_err()),
        "LAST_OWNER"
    );
    let demote = input(&me.display_name, &role(&t, "Manager").await);
    assert_eq!(code(staff::update(&t.t.db, &t.ctx(), &t.owner(), &owner_id, &demote).await.unwrap_err()), "LAST_OWNER");
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action IN ('USER_UPDATE', 'USER_DISABLE')").await, 0);
    assert_eq!(t.count(&format!("SELECT is_active FROM staff WHERE id = '{owner_id}'")).await, 1, "rolled back");

    // With a second Owner, the first can step down.
    let second = create(&t, "Second Owner", "Owner", "998877").await.unwrap();
    assert!(second.is_owner);
    assert!(staff::list(t.pool()).await.unwrap().iter().all(|m| !m.last_owner));
    let out = staff::update(&t.t.db, &t.ctx(), &t.owner(), &owner_id, &disable).await.unwrap();
    assert!(!out.is_active);
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'USER_DISABLE'").await, 1);
    let refused = login(&t.t.db, &t.auth, &t.clock, &t.dev, &owner_id, "246810").await.unwrap_err();
    assert_eq!(code(refused), "STAFF_INACTIVE", "a disabled owner can't log in");
}

#[tokio::test]
async fn only_an_owner_hands_out_owner_powers() {
    let t = till().await;
    let mgr = create(&t, "Manager", "Manager", "135790").await.unwrap();
    let as_mgr = gayanshop_lib::auth::guard::Actor { staff_id: mgr.id.clone(), approved_by: None };
    let owner_role = role(&t, "Owner").await;
    let new = NewStaff { staff: input("Sneaky", &owner_role), pin: "112233".into() };
    assert_eq!(
        code(staff::create(&t.t.db, &t.ctx(), &as_mgr, &t.auth.hasher, &new).await.unwrap_err()),
        "OWNER_ROLE_OWNER_ONLY"
    );
    let mut grant = NewStaff { staff: input("Sneaky", &role(&t, "Cashier").await), pin: "1122".into() };
    grant.staff.grants = vec![Perm::USER_MANAGE];
    assert_eq!(
        code(staff::create(&t.t.db, &t.ctx(), &as_mgr, &t.auth.hasher, &grant).await.unwrap_err()),
        "OWNER_ROLE_OWNER_ONLY"
    );
    let owner_id = t.s.owner_staff_id.clone();
    assert_eq!(
        code(staff::reset_pin(&t.t.db, &t.ctx(), &as_mgr, &t.auth.hasher, &owner_id, "000000").await.unwrap_err()),
        "OWNER_ROLE_OWNER_ONLY"
    );
}

#[tokio::test]
async fn a_pin_reset_replaces_the_pin_and_lifts_the_lockout() {
    let t = till().await;
    for _ in 0..5 {
        let _ = login(&t.t.db, &t.auth, &t.clock, &t.dev, CASHIER, "0000").await;
    }
    assert!(matches!(
        login(&t.t.db, &t.auth, &t.clock, &t.dev, CASHIER, common::CASHIER_PIN).await.unwrap_err(),
        AppError::AccountLocked { .. }
    ));
    staff::reset_pin(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, CASHIER, "8642").await.unwrap();
    assert!(login(&t.t.db, &t.auth, &t.clock, &t.dev, CASHIER, common::CASHIER_PIN).await.is_err(), "old PIN gone");
    login(&t.t.db, &t.auth, &t.clock, &t.dev, CASHIER, "8642").await.unwrap();
    assert_eq!(t.count("SELECT COUNT(*) FROM audit_logs WHERE action = 'USER_PIN_RESET'").await, 1);
    assert_eq!(
        code(staff::reset_pin(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, SUP, "12").await.unwrap_err()),
        "PIN_TOO_SHORT"
    );
}

#[tokio::test]
async fn staff_management_needs_user_manage() {
    let t = till().await;
    t.login(CASHIER, common::CASHIER_PIN).await;
    assert!(matches!(
        require(&t.auth, &t.clock, Perm::USER_MANAGE, "staff", None).unwrap_err(),
        AppError::Forbidden { perm: Perm::USER_MANAGE }
    ));
    t.login(SUP, common::SUP_PIN).await;
    assert!(require(&t.auth, &t.clock, Perm::USER_MANAGE, "staff", None).is_err(), "supervisors can't either");
}

#[tokio::test]
async fn the_list_never_exposes_pin_hashes() {
    let t = till().await;
    let json = serde_json::to_string(&staff::list(t.pool()).await.unwrap()).unwrap();
    assert!(!json.contains("argon2") && !json.contains("pin_hash"));
    assert_eq!(staff::list(t.pool()).await.unwrap().len(), 3);
}
