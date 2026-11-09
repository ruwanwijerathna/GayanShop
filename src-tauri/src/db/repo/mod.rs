//! Repositories: SQL for one aggregate each. Writes run inside the caller's `Db::write()` transaction
//! and enqueue their own outbox rows; commands add the RBAC guard, audit and `db://changed`.

pub mod batches;
pub mod cash;
pub mod categories;
pub mod checkout;
pub mod customers;
pub mod expenses;
pub mod held_carts;
pub mod inventory;
pub mod orders;
pub mod print_jobs;
pub mod products;
pub mod quick_sales;
pub mod receipts;
pub mod reports;
pub mod staff;
pub mod stock;
pub mod void;

/// Where and when a catalog write happens (row metadata, audit context, in-store code generation).
#[derive(Debug, Clone)]
pub struct WriteCtx {
    pub store_id: String,
    pub device_id: String,
    /// `T01`…`T09` — its digit prefixes this counter's in-store SKUs and barcodes (doc §7.4).
    pub device_code: String,
    pub now: String,
    pub business_date: String,
}
