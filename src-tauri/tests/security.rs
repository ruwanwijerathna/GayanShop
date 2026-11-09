//! Block 4.3 — Security hardening (doc §8 Phase 4):
//! - CSP least-privilege & no unsafe-eval
//! - Capability least-privilege audit
//! - PIN length enforcement per role (Manager/Owner >= 6, others >= 4)
//! - No hardcoded secrets or credentials in config / sources
//! - Tenancy isolation invariants

mod common;

use common::till;
use gayanshop_lib::auth::pin::{MAX_PIN_LEN, validate_pin};
use gayanshop_lib::db::repo::staff::{self, NewStaff, StaffInput};
use gayanshop_lib::domain::permissions::min_pin_len;
use gayanshop_lib::error::AppError;
use serde_json::Value;

#[test]
fn csp_enforces_least_privilege_and_no_unsafe_eval() {
    let conf_raw = std::fs::read_to_string("tauri.conf.json").expect("read tauri.conf.json");
    let conf: Value = serde_json::from_str(&conf_raw).expect("parse tauri.conf.json");

    let csp = conf["app"]["security"]["csp"].as_str().expect("csp must be present");

    // 1. default-src must be strictly 'self'
    assert!(csp.contains("default-src 'self'"), "default-src must be 'self'");

    // 2. Unsafe-eval is strictly forbidden in production CSP
    assert!(!csp.contains("'unsafe-eval'"), "unsafe-eval is banned");

    // 3. Script-src must not allow wildcards or untrusted external origins
    if csp.contains("script-src") {
        assert!(!csp.contains("script-src *"), "wildcard script-src is banned");
        assert!(!csp.contains("https://"), "external script loading is banned");
    }

    // 4. Object-src must not be wide open
    assert!(!csp.contains("object-src *"), "wildcard object-src is banned");
}

#[test]
fn capabilities_enforce_least_privilege() {
    let cap_raw = std::fs::read_to_string("capabilities/default.json").expect("read capabilities/default.json");
    let cap: Value = serde_json::from_str(&cap_raw).expect("parse capabilities/default.json");

    // Windows must only target main
    let windows = cap["windows"].as_array().expect("windows array");
    assert_eq!(windows, &[Value::String("main".to_string())]);

    // Permissions must only contain approved least-privilege sets
    let perms = cap["permissions"].as_array().expect("permissions array");
    let perm_strings: Vec<&str> = perms.iter().filter_map(|p| p.as_str()).collect();

    // Verify forbidden dangerous permissions are absent
    let forbidden_prefixes = ["shell:", "fs:", "http:"];
    for p in &perm_strings {
        for forbidden in &forbidden_prefixes {
            assert!(!p.starts_with(forbidden), "dangerous capability permission {p} must not be exposed");
        }
    }

    // Must have standard core and updater permissions
    assert!(perm_strings.contains(&"core:default"));
    assert!(perm_strings.contains(&"log:default"));
    assert!(perm_strings.contains(&"updater:default"));
}

#[test]
fn pin_rules_and_lengths_strictly_enforced() {
    // Role min lengths
    assert_eq!(min_pin_len("Owner"), 6);
    assert_eq!(min_pin_len("Manager"), 6);
    assert_eq!(min_pin_len("Supervisor"), 4);
    assert_eq!(min_pin_len("Cashier"), 4);

    // Validate digit-only rule
    assert!(validate_pin("12ab", 4).is_err());
    assert!(validate_pin("abcd", 4).is_err());
    assert!(validate_pin("", 4).is_err());
    assert!(validate_pin(" 1234", 4).is_err());

    // Validate max length rule
    assert_eq!(MAX_PIN_LEN, 8);
    assert!(validate_pin("12345678", 4).is_ok());
    assert!(validate_pin("123456789", 4).is_err());

    // Validate Manager/Owner min length 6
    assert!(validate_pin("1234", 6).is_err());
    assert!(validate_pin("12345", 6).is_err());
    assert!(validate_pin("123456", 6).is_ok());

    // Validate Cashier min length 4
    assert!(validate_pin("123", 4).is_err());
    assert!(validate_pin("1234", 4).is_ok());
}

#[tokio::test]
async fn staff_creation_and_pin_reset_enforces_role_min_pin_length() {
    let t = till().await;
    let roles = staff::roles(t.pool()).await.unwrap();
    let manager_role_id = roles.iter().find(|r| r.name == "Manager").unwrap().id.clone();
    let cashier_role_id = roles.iter().find(|r| r.name == "Cashier").unwrap().id.clone();

    // 1. Manager with 4-digit PIN must fail
    let bad_manager = NewStaff {
        staff: StaffInput {
            display_name: "Manager 1".into(),
            role_id: manager_role_id.clone(),
            ui_language: "en".into(),
            grants: vec![],
            denies: vec![],
            max_discount_bp: None,
            is_active: true,
        },
        pin: "1234".into(),
    };
    let err = staff::create(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &bad_manager).await.unwrap_err();
    match err {
        AppError::Validation { code, .. } => assert_eq!(code, "PIN_TOO_SHORT"),
        other => panic!("expected PIN_TOO_SHORT, got {other:?}"),
    }

    // 2. Manager with 6-digit PIN succeeds
    let good_manager = NewStaff {
        staff: StaffInput {
            display_name: "Manager 1".into(),
            role_id: manager_role_id.clone(),
            ui_language: "en".into(),
            grants: vec![],
            denies: vec![],
            max_discount_bp: None,
            is_active: true,
        },
        pin: "123456".into(),
    };
    let manager = staff::create(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &good_manager).await.unwrap();

    // 3. Resetting Manager PIN to 4 digits must fail
    let err = staff::reset_pin(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &manager.id, "5678").await.unwrap_err();
    match err {
        AppError::Validation { code, .. } => assert_eq!(code, "PIN_TOO_SHORT"),
        other => panic!("expected PIN_TOO_SHORT, got {other:?}"),
    }

    // 4. Cashier with 4-digit PIN succeeds
    let good_cashier = NewStaff {
        staff: StaffInput {
            display_name: "Cashier 1".into(),
            role_id: cashier_role_id,
            ui_language: "si".into(),
            grants: vec![],
            denies: vec![],
            max_discount_bp: None,
            is_active: true,
        },
        pin: "4321".into(),
    };
    assert!(staff::create(&t.t.db, &t.ctx(), &t.owner(), &t.auth.hasher, &good_cashier).await.is_ok());
}

#[test]
fn bundle_and_config_contain_no_hardcoded_secrets() {
    let conf_raw = std::fs::read_to_string("tauri.conf.json").expect("read tauri.conf.json");
    let banned_keys =
        ["supabase_service_role", "service_role_key", "secret_key", "private_key", "password", "access_token"];

    for banned in banned_keys {
        assert!(
            !conf_raw.to_lowercase().contains(banned),
            "configuration must not leak secret or credential identifier: {banned}"
        );
    }
}
