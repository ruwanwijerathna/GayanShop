//! Block 3.3: registering a counter (doc §1.3, decisions #18–#19). The sign-in, the register-device function
//! and the RPC endpoint are a local mock server speaking the same HTTP as Supabase (GoTrue, functions,
//! PostgREST); the keyring is the in-memory store. The function itself runs against the real stack in the
//! chaos suite (Block 3.6).

use std::sync::Arc;

use gayanshop_lib::audit;
use gayanshop_lib::auth::pin::PinHasher;
use gayanshop_lib::clock::ManualClock;
use gayanshop_lib::db::Db;
use gayanshop_lib::error::AppError;
use gayanshop_lib::setup::{self, SetupInput, first_run};
use gayanshop_lib::sync::api::{HttpApi, SyncApi, SyncError};
use gayanshop_lib::sync::config::{self, ServerConfig};
use gayanshop_lib::sync::credentials::{KeyringStore, MemoryStore, SecretStore, refresh_token_account};
use gayanshop_lib::sync::registration::{self, BOOTSTRAP_PENDING_KEY, RegisterInput};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OWNER_PASSWORD: &str = "correct horse battery staple";

fn clock() -> ManualClock {
    ManualClock::new(chrono::DateTime::parse_from_rfc3339("2026-11-09T04:30:00Z").unwrap().with_timezone(&chrono::Utc))
}

async fn open() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db =
        Db::open(&dir.path().join(gayanshop_lib::db::DB_FILE_NAME), &dir.path().join("backups")).await.unwrap();
    (dir, db)
}

async fn set_up(db: &Db) -> setup::SetupResult {
    let input = SetupInput {
        store_name: "GayanShop".into(),
        owner_name: "Gayan".into(),
        owner_pin: "246810".into(),
        device_code: "T01".into(),
        ui_language: "en".into(),
    };
    first_run(db, &PinHasher::fast_for_tests(), &clock(), &input).await.unwrap()
}

fn input(server: &MockServer) -> RegisterInput {
    RegisterInput {
        server_url: server.uri(),
        anon_key: "anon-key".into(),
        owner_email: " owner@gayanshop.lk ".into(),
        owner_password: OWNER_PASSWORD.into(),
        device_name: Some("Front counter".into()),
    }
}

/// GoTrue's password grant for the owner's account.
async fn owner_sign_in(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "password"))
        .and(header("apikey", "anon-key"))
        .and(body_partial_json(json!({ "email": "owner@gayanshop.lk", "password": OWNER_PASSWORD })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "owner-access", "refresh_token": "owner-refresh", "expires_in": 3600
        })))
        .mount(server)
        .await;
}

fn registered(store_id: &str, device_id: &str, code: &str) -> Value {
    json!({
        "store_id": store_id, "store_name": "GayanShop", "device_id": device_id, "device_code": code,
        "refresh_token": "device-refresh-1", "access_token": "device-access-1", "expires_in": 3600
    })
}

/// Every text value this counter's database could hold a secret in, to prove none landed there.
async fn all_text(db: &Db) -> String {
    let dump: Vec<String> = sqlx::query_scalar(
        "SELECT key || '=' || value FROM device_settings
         UNION ALL SELECT key || '=' || value FROM store_settings
         UNION ALL SELECT metadata FROM audit_logs
         UNION ALL SELECT payload FROM sync_outbox
         UNION ALL SELECT COALESCE(details, '') || COALESCE(error_message, '') FROM sync_logs",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    dump.join("\n")
}

#[tokio::test]
async fn a_set_up_counter_registers_adopting_its_own_ids() {
    let (_dir, db) = open().await;
    let s = set_up(&db).await;
    let identity = setup::load_identity(&db.pool).await.unwrap().unwrap();
    let server = MockServer::start().await;
    owner_sign_in(&server).await;
    Mock::given(method("POST"))
        .and(path("/functions/v1/register-device"))
        .and(header("authorization", "Bearer owner-access"))
        .and(header("apikey", "anon-key"))
        .and(body_partial_json(json!({
            "store_id": s.store_id, "store_name": "GayanShop", "device_id": s.device_id, "device_code": "T01",
            "device_name": "Front counter"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(registered(&s.store_id, &s.device_id, "T01")))
        .expect(1)
        .mount(&server)
        .await;
    let secrets = MemoryStore::new();

    let r = registration::register_this_counter(&db, &clock(), &secrets, &identity, &s.owner_staff_id, &input(&server))
        .await
        .unwrap();

    assert_eq!(
        (r.store_id.as_str(), r.device_id.as_str(), r.device_code.as_str()),
        (s.store_id.as_str(), s.device_id.as_str(), "T01")
    );
    // The device's refresh token is in the credential store, under this device.
    assert_eq!(secrets.get(&refresh_token_account(&s.device_id)).unwrap().as_deref(), Some("device-refresh-1"));
    // The server is per-device configuration; the anon key is the only key kept.
    let cfg = config::load(&db.pool).await.unwrap().unwrap();
    assert_eq!(cfg, ServerConfig { url: server.uri(), anon_key: "anon-key".into() });
    assert!(config::registered_at(&db.pool).await.unwrap().is_some());
    assert!(registration::is_registered(&db, &secrets).await.unwrap());
    // Audited, on this device's chain.
    let (action, staff, meta): (String, Option<String>, String) =
        sqlx::query_as("SELECT action, staff_id, metadata FROM audit_logs ORDER BY device_seq DESC LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(action, "DEVICE_REGISTER");
    assert_eq!(staff.as_deref(), Some(s.owner_staff_id.as_str()));
    let meta: Value = serde_json::from_str(&meta).unwrap();
    assert_eq!(meta["mode"], "adopt");
    assert_eq!(meta["device_code"], "T01");
    assert!(audit::verify(&db.pool).await.unwrap().iter().all(|r| r.ok), "the chain still verifies");
    // No secret of any kind is in the database: not the owner's password, not a token.
    let text = all_text(&db).await;
    for secret in [OWNER_PASSWORD, "owner-access", "owner-refresh", "device-refresh-1", "device-access-1"] {
        assert!(!text.contains(secret), "{secret} leaked into gayanshop_pos.db");
    }
}

#[tokio::test]
async fn a_wrong_owner_password_registers_nothing() {
    let (_dir, db) = open().await;
    let s = set_up(&db).await;
    let identity = setup::load_identity(&db.pool).await.unwrap().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "error_code": "invalid_credentials" })))
        .mount(&server)
        .await;
    Mock::given(path("/functions/v1/register-device"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let secrets = MemoryStore::new();
    let err =
        registration::register_this_counter(&db, &clock(), &secrets, &identity, &s.owner_staff_id, &input(&server))
            .await
            .unwrap_err();
    assert!(matches!(&err, AppError::Validation { code, .. } if code == "SYNC_INVALID_CREDENTIALS"), "{err:?}");
    assert!(secrets.get(&refresh_token_account(&s.device_id)).unwrap().is_none());
    assert!(config::load(&db.pool).await.unwrap().is_none());
    assert!(!registration::is_registered(&db, &secrets).await.unwrap());
}

#[tokio::test]
async fn the_server_refusing_the_store_registers_nothing() {
    let (_dir, db) = open().await;
    let s = set_up(&db).await;
    let identity = setup::load_identity(&db.pool).await.unwrap().unwrap();
    let server = MockServer::start().await;
    owner_sign_in(&server).await;
    Mock::given(path("/functions/v1/register-device"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({ "error": "STORE_OWNED_BY_ANOTHER_ACCOUNT" })))
        .mount(&server)
        .await;
    let secrets = MemoryStore::new();
    let err =
        registration::register_this_counter(&db, &clock(), &secrets, &identity, &s.owner_staff_id, &input(&server))
            .await
            .unwrap_err();
    assert!(
        matches!(&err, AppError::Validation { code, .. } if code == "SYNC_STORE_OWNED_BY_ANOTHER_ACCOUNT"),
        "{err:?}"
    );
    let audits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs WHERE action = 'DEVICE_REGISTER'")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(audits, 0);
    assert!(secrets.get(&refresh_token_account(&s.device_id)).unwrap().is_none());
}

#[tokio::test]
async fn a_server_that_reassigns_ids_is_not_trusted() {
    let (_dir, db) = open().await;
    let s = set_up(&db).await;
    let identity = setup::load_identity(&db.pool).await.unwrap().unwrap();
    let server = MockServer::start().await;
    owner_sign_in(&server).await;
    Mock::given(path("/functions/v1/register-device"))
        .respond_with(ResponseTemplate::new(200).set_body_json(registered(&s.store_id, &s.device_id, "T02")))
        .mount(&server)
        .await;
    let secrets = MemoryStore::new();
    let err =
        registration::register_this_counter(&db, &clock(), &secrets, &identity, &s.owner_staff_id, &input(&server))
            .await
            .unwrap_err();
    assert!(matches!(&err, AppError::Invariant { code, .. } if code == "SYNC_IDENTITY_MISMATCH"), "{err:?}");
    assert!(secrets.get(&refresh_token_account(&s.device_id)).unwrap().is_none(), "no token for a mismatched identity");
    assert_eq!(
        setup::load_identity(&db.pool).await.unwrap().unwrap().device_code,
        "T01",
        "the ids are never reassigned"
    );
}

#[tokio::test]
async fn bad_input_and_an_unreachable_server_are_reported() {
    let (_dir, db) = open().await;
    let s = set_up(&db).await;
    let identity = setup::load_identity(&db.pool).await.unwrap().unwrap();
    let secrets = MemoryStore::new();
    let mut bad = RegisterInput {
        server_url: "http://example.com".into(),
        anon_key: "k".into(),
        owner_email: "owner@gayanshop.lk".into(),
        owner_password: "x".into(),
        device_name: None,
    };
    let err = registration::register_this_counter(&db, &clock(), &secrets, &identity, &s.owner_staff_id, &bad)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AppError::Validation { code, .. } if code == "SYNC_URL_INVALID"),
        "plain HTTP only to this machine"
    );
    bad.server_url = "https://x.supabase.co".into();
    bad.owner_email = "not-an-email".into();
    let err = registration::register_this_counter(&db, &clock(), &secrets, &identity, &s.owner_staff_id, &bad)
        .await
        .unwrap_err();
    assert!(matches!(&err, AppError::Validation { code, .. } if code == "SYNC_EMAIL_INVALID"));
    // Nothing listens on port 9 of this machine.
    bad.server_url = "http://127.0.0.1:9".into();
    bad.owner_email = "owner@gayanshop.lk".into();
    let err = registration::register_this_counter(&db, &clock(), &secrets, &identity, &s.owner_staff_id, &bad)
        .await
        .unwrap_err();
    assert!(matches!(&err, AppError::Validation { code, .. } if code == "SYNC_OFFLINE"), "{err:?}");
}

#[tokio::test]
async fn a_new_counter_joins_the_owners_store_with_the_next_code() {
    let (_dir, db) = open().await;
    let server = MockServer::start().await;
    owner_sign_in(&server).await;
    let store_id = "0196f0c2-0000-7000-8000-000000000001";
    // The function gets no store and no code (it assigns the next free one), only this counter's new id.
    Mock::given(path("/functions/v1/register-device"))
        .and(body_partial_json(json!({ "store_id": null, "device_code": null })))
        .respond_with(move |req: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            let device_id = body["device_id"].as_str().unwrap().to_owned();
            ResponseTemplate::new(200).set_body_json(registered(store_id, &device_id, "T02"))
        })
        .expect(1)
        .mount(&server)
        .await;
    let secrets = MemoryStore::new();

    let r = registration::join_store(&db, &clock(), &secrets, &input(&server), "si").await.unwrap();

    assert_eq!((r.store_id.as_str(), r.device_code.as_str()), (store_id, "T02"));
    let identity = setup::load_identity(&db.pool).await.unwrap().unwrap();
    assert_eq!(
        (identity.store_id.as_str(), identity.device_id.as_str(), identity.device_code.as_str()),
        (store_id, r.device_id.as_str(), "T02")
    );
    assert_eq!(secrets.get(&refresh_token_account(&r.device_id)).unwrap().as_deref(), Some("device-refresh-1"));
    let pending: Option<bool> = gayanshop_lib::db::settings::device_get(&db.pool, BOOTSTRAP_PENDING_KEY).await.unwrap();
    assert_eq!(pending, Some(true), "the store's history is still to be pulled");
    let lang: Option<String> = gayanshop_lib::db::settings::device_get(&db.pool, "ui.default_language").await.unwrap();
    assert_eq!(lang.as_deref(), Some("si"));
    // No store was seeded locally: roles, staff and settings come from the cloud.
    let seeded: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM roles) + (SELECT COUNT(*) FROM staff) + (SELECT COUNT(*) FROM store_settings)",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(seeded, 0);
    let (action, seq): (String, i64) =
        sqlx::query_as("SELECT action, device_seq FROM audit_logs").fetch_one(&db.pool).await.unwrap();
    assert_eq!((action.as_str(), seq), ("DEVICE_REGISTER", 1), "the new counter's chain starts with its registration");
    // A second join is refused: this terminal already has an identity.
    let err = registration::join_store(&db, &clock(), &secrets, &input(&server), "en").await.unwrap_err();
    assert!(matches!(err, AppError::AlreadyInitialized));
}

#[tokio::test]
async fn the_device_token_is_refreshed_and_the_rotated_refresh_token_kept() {
    let server = MockServer::start().await;
    let device_id = "0196f0c2-0000-7000-8000-0000000000d1";
    let secrets: Arc<MemoryStore> = Arc::new(MemoryStore::new());
    secrets.set(&refresh_token_account(device_id), "device-refresh-1").unwrap();
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .and(body_partial_json(json!({ "refresh_token": "device-refresh-1" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "device-access-2", "refresh_token": "device-refresh-2", "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/v1/rpc/sync_health"))
        .and(header("authorization", "Bearer device-access-2"))
        .and(header("apikey", "anon-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "server_time": "2026-11-09T04:30:00.000Z", "store_id": "s", "device_id": device_id,
            "device_code": "T01", "chain_broken": []
        })))
        .expect(2)
        .mount(&server)
        .await;
    let api =
        HttpApi::new(ServerConfig::parse(&server.uri(), "anon-key").unwrap(), device_id, secrets.clone()).unwrap();

    assert_eq!(api.health().await.unwrap().device_code, "T01");
    assert_eq!(api.health().await.unwrap().device_code, "T01", "the access token is reused until it nears expiry");
    assert_eq!(
        secrets.get(&refresh_token_account(device_id)).unwrap().as_deref(),
        Some("device-refresh-2"),
        "GoTrue rotates refresh tokens; the new one replaces the old in the keyring"
    );
}

#[tokio::test]
async fn a_revoked_identity_is_unauthorised_and_an_outage_is_offline() {
    let server = MockServer::start().await;
    let device_id = "0196f0c2-0000-7000-8000-0000000000d1";
    let secrets: Arc<MemoryStore> = Arc::new(MemoryStore::new());
    let cfg = ServerConfig::parse(&server.uri(), "anon-key").unwrap();
    let api = HttpApi::new(cfg.clone(), device_id, secrets.clone()).unwrap();
    assert_eq!(
        api.health().await.unwrap_err(),
        SyncError::Unauthorized("NOT_REGISTERED".into()),
        "no token stored yet"
    );

    secrets.set(&refresh_token_account(device_id), "revoked").unwrap();
    Mock::given(path("/auth/v1/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "error_code": "refresh_token_not_found" })))
        .mount(&server)
        .await;
    assert!(matches!(api.health().await.unwrap_err(), SyncError::Unauthorized(_)));

    let outage = MockServer::start().await;
    Mock::given(path("/auth/v1/token")).respond_with(ResponseTemplate::new(503)).mount(&outage).await;
    let api = HttpApi::new(ServerConfig::parse(&outage.uri(), "anon-key").unwrap(), device_id, secrets).unwrap();
    assert!(matches!(api.health().await.unwrap_err(), SyncError::Offline(_)), "a 5xx is retried later, not fatal");
}

#[test]
fn the_windows_credential_manager_keeps_the_token() {
    // The real store the counters use, under a test-only service name so a counter's own entry is never touched.
    let store = KeyringStore::new("com.gayanshop.pos.tests");
    let account = refresh_token_account(&gayanshop_lib::clock::new_id());
    assert_eq!(store.get(&account).unwrap(), None);
    store.set(&account, "device-refresh-1").unwrap();
    assert_eq!(store.get(&account).unwrap().as_deref(), Some("device-refresh-1"));
    store.set(&account, "device-refresh-2").unwrap();
    assert_eq!(store.get(&account).unwrap().as_deref(), Some("device-refresh-2"), "a rotated token replaces the old");
    store.delete(&account).unwrap();
    assert_eq!(store.get(&account).unwrap(), None);
    store.delete(&account).unwrap();
}
