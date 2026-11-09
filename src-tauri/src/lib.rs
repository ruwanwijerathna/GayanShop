//! GayanShop POS — Rust core. See docs/ARCHITECTURE.md and AGENT.md.

// AGENT.md §2: no panicking shortcuts in production code, no float arithmetic (money is i64 cents).
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]
#![deny(clippy::float_arithmetic)]

pub mod audit;
pub mod auth;
pub mod backup;
pub mod clock;
pub mod commands;
pub mod db;
pub mod domain;
pub mod error;
pub mod export;
pub mod hardware;
pub mod prefs;
pub mod setup;
pub mod state;
pub mod sync;

use std::sync::Arc;

use tauri::Manager;

use crate::auth::Auth;
use crate::auth::pin::PinHasher;
use crate::clock::SystemClock;
use crate::db::Db;
use crate::db::repo::print_jobs::RECEIPT_PRINTER;
use crate::hardware::printer::worker::{PrintHub, WorkerConfig};
use crate::hardware::printer::{self, TransportConfig};
use crate::state::{AppState, Services};
use crate::sync::credentials::{KEYRING_SERVICE, KeyringStore, SecretStore};
use crate::sync::worker::{SyncEvents, SyncHandle, SyncStatus, Worker, http_connector};

/// Environment override for the data folder (holds `pos.db` and `backups\`).
pub const DATA_DIR_ENV: &str = "GAYANSHOP_DATA_DIR";

/// The typed IPC surface. `cargo test export_bindings` writes it to `src/db/bindings.ts`.
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
        commands::system::app_info,
        commands::system::app_status,
        commands::system::integrity_check,
        commands::system::business_today,
        commands::setup::first_run_setup,
        commands::auth::list_login_staff,
        commands::auth::login,
        commands::auth::logout,
        commands::auth::current_session,
        commands::auth::request_approval,
        commands::auth::authorize,
        commands::audit::audit_page,
        commands::audit::audit_verify,
        commands::prefs::device_preferences,
        commands::prefs::set_device_theme,
        commands::prefs::set_ui_language,
        commands::products::list_categories,
        commands::products::create_category,
        commands::products::update_category,
        commands::products::delete_category,
        commands::products::list_products,
        commands::products::get_product,
        commands::products::create_product,
        commands::products::update_product,
        commands::products::delete_product,
        commands::products::search_products,
        commands::products::lookup_product_code,
        commands::batches::list_batches,
        commands::batches::get_batch,
        commands::batches::create_batch,
        commands::batches::update_batch,
        commands::batches::set_batch_item,
        commands::batches::remove_batch_item,
        commands::batches::receive_batch,
        commands::batches::ad_hoc_purchase,
        commands::batches::product_layers,
        commands::batches::batch_allocation_preview,
        commands::batches::reopen_batch,
        commands::batches::add_late_batch_item,
        commands::batches::close_batch,
        commands::labels::label_templates,
        commands::labels::label_printer_profile,
        commands::labels::set_label_printer_profile,
        commands::labels::calibrate_label_template,
        commands::labels::label_preview,
        commands::labels::print_labels,
        commands::labels::label_alignment_test,
        commands::inventory::record_damage,
        commands::inventory::adjust_stock,
        commands::inventory::record_stock_count,
        commands::inventory::write_off_batch,
        commands::inventory::stock_movements,
        commands::inventory::stock_valuation,
        commands::inventory::oversold_layers,
        commands::quick_sales::quick_sale_review,
        commands::quick_sales::quick_sale_detail,
        commands::quick_sales::promote_quick_sale,
        commands::quick_sales::ignore_quick_sale,
        commands::quick_sales::quick_sale_hint,
        commands::cash::current_cash_session,
        commands::cash::open_cash_session,
        commands::cash::cash_drawer_movement,
        commands::cash::no_sale,
        commands::cash::close_cash_session,
        commands::cash::list_cash_sessions,
        commands::cash::cash_session_report,
        commands::checkout::checkout_order,
        commands::customers::customer_lookup,
        commands::customers::customer_list,
        commands::customers::customer_profile,
        commands::customers::customer_history,
        commands::customers::customer_order_receipt,
        commands::customers::customer_analytics,
        commands::customers::update_customer,
        commands::customers::erase_customer,
        commands::expenses::expense_categories,
        commands::expenses::save_expense_category,
        commands::expenses::expense_list,
        commands::expenses::record_expense,
        commands::expenses::update_expense,
        commands::expenses::delete_expense,
        commands::reports::report_summary,
        commands::reports::report_trend,
        commands::reports::report_sales_by,
        commands::reports::report_voids,
        commands::reports::report_devices,
        commands::reports::report_batch_sell_through,
        commands::reports::rebuild_derived,
        commands::export::export_pick_folder,
        commands::export::export_csv,
        commands::orders::order_history,
        commands::orders::order_receipt,
        commands::hardware::printer_status,
        commands::hardware::printer_profile,
        commands::hardware::set_printer_profile,
        commands::hardware::print_test_page,
        commands::hardware::print_retry,
        commands::hardware::print_dismiss_failed,
        commands::hardware::print_recovery_resolve,
        commands::pos::cart_save_current,
        commands::pos::cart_discard_current,
        commands::pos::cart_current,
        commands::pos::cart_hold,
        commands::pos::held_carts,
        commands::pos::cart_recall,
        commands::backup::backup_status,
        commands::backup::backup_pick_folder,
        commands::backup::backup_now,
        commands::staff::list_staff,
        commands::staff::list_roles,
        commands::staff::create_staff,
        commands::staff::update_staff,
        commands::staff::reset_staff_pin,
        commands::void::void_order,
        commands::void::session_orders,
        commands::sync::sync_info,
        commands::sync::sync_register,
        commands::sync::sync_status,
        commands::sync::sync_now,
    ])
}

/// The sync worker's events, into the app: `sync://status` for the status pill, and `db://changed` for
/// the tables a pull changed (a pulled store setting also refreshes this counter's identity: store name,
/// calendar).
struct AppSyncEvents {
    app: tauri::AppHandle,
}

impl SyncEvents for AppSyncEvents {
    fn status(&self, status: &SyncStatus) {
        if let Err(e) = tauri::Emitter::emit(&self.app, "sync://status", status) {
            log::warn!("sync://status emit failed: {e}");
        }
    }

    fn applied(&self, tables: &[&'static str]) {
        if tables.contains(&"store_settings") {
            let app = self.app.clone();
            tauri::async_runtime::spawn(async move {
                let Some(state) = app.try_state::<AppState>() else { return };
                match setup::load_identity(&state.db.pool).await {
                    Ok(Some(identity)) => {
                        if let Err(e) = state.set_identity(identity) {
                            log::warn!("sync: identity refresh failed: {e}");
                        }
                    }
                    Ok(None) => {}
                    Err(e) => log::warn!("sync: identity reload failed: {e}"),
                }
            });
        }
        // Straight to the UI: going through emit_changed would wake the worker for nothing.
        if let Err(e) = tauri::Emitter::emit(&self.app, "db://changed", commands::DbChanged { tables: tables.to_vec() })
        {
            log::warn!("db://changed emit failed: {e}");
        }
    }
}

/// Builds and runs the Tauri application. Errors are returned to `main`, never panicked.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), tauri::Error> {
    let builder = specta_builder();
    tauri::Builder::default()
        // Must be first: a second launch focuses the running till instead of opening another DB writer.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            // `GAYANSHOP_DATA_DIR` points the app at another folder (the e2e suite, a support copy); by default
            // it is `%APPDATA%\com.gayanshop.pos`.
            let data_dir = match std::env::var_os(DATA_DIR_ENV) {
                Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
                _ => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&data_dir)?;
            let db = tauri::async_runtime::block_on(Db::open(&data_dir.join("pos.db"), &data_dir.join("backups")))?;
            let identity = tauri::async_runtime::block_on(setup::load_identity(&db.pool))?;
            let auth = Auth::new(PinHasher::production());
            let clock: Arc<dyn clock::Clock> = Arc::new(SystemClock);
            let tz = identity.as_ref().map_or(chrono_tz::Asia::Colombo, |i| i.calendar.timezone);
            let hub = |key: &'static str| {
                let handle = app.handle().clone();
                tauri::async_runtime::block_on(PrintHub::start(
                    db.clone(),
                    key,
                    Arc::new(|c: &TransportConfig| printer::connect(c)),
                    clock.clone(),
                    tz,
                    WorkerConfig::default(),
                    Arc::new(move || commands::emit_changed(&handle, &["print_jobs"])),
                ))
            };
            let printer = hub(RECEIPT_PRINTER)?;
            let label_printer = hub(hardware::label::LABEL_PRINTER)?;
            let secrets: Arc<dyn SecretStore> = Arc::new(KeyringStore::new(KEYRING_SERVICE));
            let sync = SyncHandle::new();
            let worker = Worker {
                db: db.clone(),
                clock: clock.clone(),
                secrets: secrets.clone(),
                events: Arc::new(AppSyncEvents { app: app.handle().clone() }),
                handle: sync.clone(),
                connect: http_connector(),
            };
            let services = Services { printer, label_printer, secrets, sync };
            app.manage(AppState::new(db, clock, data_dir, auth, identity, services));
            tauri::async_runtime::spawn(worker.run());
            Ok(())
        })
        .run(tauri::generate_context!())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    const BINDINGS_HEADER: &str =
        "// GENERATED by tauri-specta via `cargo test export_bindings` — do not edit.\n// @ts-nocheck";

    #[test]
    fn export_bindings() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/db/bindings.ts");
        super::specta_builder()
            .export(specta_typescript::Typescript::default().header(BINDINGS_HEADER), &path)
            .expect("export TypeScript bindings");
        let ts = std::fs::read_to_string(&path).expect("read bindings");
        for cmd in [
            "appInfo",
            "appStatus",
            "integrityCheck",
            "firstRunSetup",
            "listLoginStaff",
            "login",
            "logout",
            "currentSession",
            "requestApproval",
            "authorize",
            "auditPage",
            "auditVerify",
            "devicePreferences",
            "setDeviceTheme",
            "setUiLanguage",
            "listCategories",
            "createCategory",
            "updateCategory",
            "deleteCategory",
            "listProducts",
            "getProduct",
            "createProduct",
            "updateProduct",
            "deleteProduct",
            "searchProducts",
            "lookupProductCode",
            "listBatches",
            "getBatch",
            "createBatch",
            "updateBatch",
            "setBatchItem",
            "removeBatchItem",
            "receiveBatch",
            "adHocPurchase",
            "productLayers",
            "batchAllocationPreview",
            "reopenBatch",
            "addLateBatchItem",
            "closeBatch",
            "labelTemplates",
            "labelPrinterProfile",
            "setLabelPrinterProfile",
            "calibrateLabelTemplate",
            "labelPreview",
            "printLabels",
            "labelAlignmentTest",
            "recordDamage",
            "adjustStock",
            "recordStockCount",
            "writeOffBatch",
            "stockMovements",
            "stockValuation",
            "oversoldLayers",
            "quickSaleReview",
            "quickSaleDetail",
            "promoteQuickSale",
            "ignoreQuickSale",
            "quickSaleHint",
            "currentCashSession",
            "openCashSession",
            "cashDrawerMovement",
            "noSale",
            "closeCashSession",
            "listCashSessions",
            "cashSessionReport",
            "checkoutOrder",
            "voidOrder",
            "sessionOrders",
            "syncInfo",
            "syncRegister",
            "syncStatus",
            "syncNow",
            "customerLookup",
            "printerStatus",
            "printerProfile",
            "setPrinterProfile",
            "printTestPage",
            "printRetry",
            "printDismissFailed",
            "printRecoveryResolve",
            "cartSaveCurrent",
            "cartDiscardCurrent",
            "cartCurrent",
            "cartHold",
            "heldCarts",
            "cartRecall",
            "backupStatus",
            "backupPickFolder",
            "backupNow",
            "listStaff",
            "listRoles",
            "createStaff",
            "updateStaff",
            "resetStaffPin",
        ] {
            assert!(ts.contains(&format!("{cmd}:")), "command {cmd} missing from bindings");
        }
        assert!(ts.contains("NOT_LOGGED_IN") || ts.contains("AppError"), "AppError type missing from bindings");
    }
}
