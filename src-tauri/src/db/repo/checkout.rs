//! `checkout_order` (doc §8 Phase 1, §13.5): ONE transaction using the seal pattern, in exactly this order:
//!
//! 1. idempotency: an order with this `checkout_token` already exists ⇒ return it unchanged;
//! 2. resolve or create the customer (the order's `customer_id` is frozen once inserted);
//! 3. bump the receipt counter (`receipt_no` is frozen too);
//! 4. insert the order as `pending`;
//! 5. insert the lines, payments (all ≥ 0) and FIFO stock slices with `revenue_net`;
//! 6. SEAL it (`pending → completed`): SQL re-verifies every total, slice, limit and the open session;
//! 7. audit overrides and approved discounts;
//! 8. delete the `__current__` held cart;
//! 9. queue the receipt (and a drawer kick for cash);
//! 10. outbox rows for everything.
//!
//! Rust computes every number first (domain::pricing / payments / fifo) and refuses what the seal would
//! refuse, with a translatable error, so the SQL checks are a second line of defence, not the UI.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use sqlx::{FromRow, SqliteConnection};

use super::WriteCtx;
use super::customers;
use super::print_jobs::{self, PrintKind};
use super::stock::{self, Movement, StockReason};
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::clock::new_id;
use crate::db::{Db, outbox};
use crate::domain::business_date::parse_date;
use crate::domain::fifo::Slice;
use crate::domain::payments::{PaymentInput, PaymentMethod, PaymentRuleError, validate_payments};
use crate::domain::permissions::{Perm, PermMask};
use crate::domain::pricing::{self, LineInput, PricedOrder, PricingError};
use crate::domain::{quick_sale, receipt_no};
use crate::error::{AppError, AppResult};

/// The crash-recovery cart row, deleted inside the sale transaction (doc §13.2).
pub const CURRENT_CART: &str = "__current__";
const MAX_LINES: usize = 500;
const QTY_MAX: i64 = 100_000;
const MONEY_MAX: i64 = 10_000_000_000;
const NAME_MAX: usize = 80;
const TEXT_MAX: usize = 120;

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CheckoutLine {
    /// A catalog product, or `None` for a quick-sale line (then `name` is required).
    pub product_id: Option<String>,
    /// Quick-sale name as typed (ignored for catalog lines, which use the product's printable name).
    pub name: Option<String>,
    /// Price per unit actually charged. Below the product's list price = a price override.
    #[specta(type = specta_typescript::Number)]
    pub unit_price: i64,
    #[specta(type = specta_typescript::Number)]
    pub qty: i64,
    #[specta(type = specta_typescript::Number)]
    pub line_discount: i64,
    /// Quick-sale only: the cost per unit if the cashier knows it.
    #[specta(type = Option<specta_typescript::Number>)]
    pub unit_cost: Option<i64>,
    pub override_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct PaymentLine {
    pub method: PaymentMethod,
    /// Applied to the bill (never negative: all sales are final).
    #[specta(type = specta_typescript::Number)]
    pub amount: i64,
    /// Cash handed over; change = tendered − amount. Cash only.
    #[specta(type = Option<specta_typescript::Number>)]
    pub tendered: Option<i64>,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct CheckoutInput {
    /// Generated per cart by the UI; a retry with the same token returns the same order.
    pub checkout_token: String,
    pub lines: Vec<CheckoutLine>,
    #[specta(type = specta_typescript::Number)]
    pub bill_discount: i64,
    pub payments: Vec<PaymentLine>,
    pub customer_phone: Option<String>,
    pub customer_name: Option<String>,
    pub note: Option<String>,
    /// A supervisor's single-use approval bound to (perm, `checkout_token`, this cashier).
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CheckoutResult {
    pub order_id: String,
    pub receipt_no: String,
    #[specta(type = specta_typescript::Number)]
    pub grand_total: i64,
    #[specta(type = specta_typescript::Number)]
    pub change_due: i64,
    pub customer_id: Option<String>,
    /// True when this token had already been checked out: nothing new was written.
    pub replayed: bool,
}

/// Who is selling, with their effective permissions and limits (from the session).
#[derive(Debug, Clone)]
pub struct Seller {
    pub staff_id: String,
    pub perms: PermMask,
    pub max_discount_bp: i64,
    pub max_override_bp: i64,
}

/// Fault-injection seam for `tests/crash_mid_checkout.rs`: called between every step of the transaction.
/// Production passes [`NoFaults`]; an error aborts the transaction exactly like a crash would.
pub trait Checkpoint: Send + Sync {
    fn reached(&self, step: &'static str) -> AppResult<()>;
}

pub struct NoFaults;

impl Checkpoint for NoFaults {
    fn reached(&self, _step: &'static str) -> AppResult<()> {
        Ok(())
    }
}

#[derive(FromRow)]
struct ProductRow {
    name: String,
    barcode: Option<String>,
    price: i64,
    min_price: Option<i64>,
    default_cost: i64,
    track_stock: bool,
    tax_rate_bp: i64,
    is_active: bool,
}

/// One validated line, ready to price and write.
struct Line {
    item_id: String,
    product_id: Option<String>,
    custom_key: Option<String>,
    name: String,
    barcode: Option<String>,
    list_price: i64,
    unit_price: i64,
    min_price: Option<i64>,
    qty: i64,
    line_discount: i64,
    tax_rate_bp: i64,
    tracked: bool,
    /// Non-tracked lines: (cost_total, cost_known).
    flat_cost: (i64, bool),
    override_reason: Option<String>,
}

impl Line {
    fn overridden(&self) -> bool {
        self.unit_price < self.list_price
    }
}

struct Approver {
    staff_id: String,
    perms: PermMask,
    max_discount_bp: i64,
    max_override_bp: i64,
}

fn printable(c: char) -> bool {
    matches!(c, ' '..='~' | '\u{00A0}'..='\u{00FF}')
}

fn opt_text(v: Option<&str>, max: usize) -> AppResult<Option<String>> {
    let v = v.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    if v.as_ref().is_some_and(|s| s.chars().count() > max) {
        return Err(AppError::validation("TEXT_TOO_LONG", format!("max {max}")));
    }
    Ok(v)
}

fn pricing_err(e: PricingError) -> AppError {
    match e {
        PricingError::NoLines => AppError::validation("CART_EMPTY", e.to_string()),
        PricingError::BadLineDiscount(_) => AppError::validation("LINE_DISCOUNT_INVALID", e.to_string()),
        PricingError::BadBillDiscount => AppError::validation("BILL_DISCOUNT_INVALID", e.to_string()),
        other => AppError::validation("LINE_INVALID", other.to_string()),
    }
}

fn payment_err(e: PaymentRuleError) -> AppError {
    match e {
        PaymentRuleError::Mismatch { .. } => AppError::validation("PAYMENT_MISMATCH", e.to_string()),
        PaymentRuleError::Negative(_) => AppError::validation("PAYMENT_NEGATIVE", e.to_string()),
        other => AppError::validation("PAYMENT_INVALID", other.to_string()),
    }
}

async fn store_setting<T: serde::de::DeserializeOwned>(conn: &mut SqliteConnection, key: &str) -> AppResult<Option<T>> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM store_settings WHERE key = ?1")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
    raw.map(|r| serde_json::from_str(&r).map_err(|e| AppError::internal(format!("setting {key}: {e}")))).transpose()
}

async fn build_lines(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    seller: &Seller,
    input: &CheckoutInput,
    default_tax_bp: i64,
) -> AppResult<Vec<Line>> {
    if input.lines.is_empty() {
        return Err(AppError::validation("CART_EMPTY", "no lines"));
    }
    if input.lines.len() > MAX_LINES {
        return Err(AppError::validation("LINE_INVALID", format!("max {MAX_LINES} lines")));
    }
    let mut out = Vec::with_capacity(input.lines.len());
    for l in &input.lines {
        if !(1..=QTY_MAX).contains(&l.qty) {
            return Err(AppError::validation("QTY_INVALID", l.qty.to_string()));
        }
        if !(0..=MONEY_MAX).contains(&l.unit_price) {
            return Err(AppError::validation("PRICE_INVALID", l.unit_price.to_string()));
        }
        let override_reason = opt_text(l.override_reason.as_deref(), TEXT_MAX)?;
        match &l.product_id {
            Some(pid) => {
                let p: ProductRow = sqlx::query_as(
                    "SELECT name, barcode, price, min_price, default_cost, track_stock, tax_rate_bp, is_active
                     FROM products WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL",
                )
                .bind(pid)
                .bind(&ctx.store_id)
                .fetch_optional(&mut *conn)
                .await?
                .ok_or_else(|| AppError::validation("PRODUCT_UNAVAILABLE", pid.clone()))?;
                if !p.is_active {
                    return Err(AppError::validation("PRODUCT_UNAVAILABLE", pid.clone()));
                }
                let flat = p
                    .default_cost
                    .checked_mul(l.qty)
                    .ok_or_else(|| AppError::validation("LINE_INVALID", "overflow"))?;
                out.push(Line {
                    item_id: new_id(),
                    product_id: Some(pid.clone()),
                    custom_key: None,
                    name: p.name,
                    barcode: p.barcode,
                    list_price: p.price.max(l.unit_price),
                    unit_price: l.unit_price,
                    min_price: p.min_price,
                    qty: l.qty,
                    line_discount: l.line_discount,
                    tax_rate_bp: p.tax_rate_bp,
                    tracked: p.track_stock,
                    flat_cost: (flat, p.default_cost > 0),
                    override_reason,
                });
            }
            None => {
                if !seller.perms.has(Perm::QUICK_SALE) {
                    return Err(AppError::Forbidden { perm: Perm::QUICK_SALE });
                }
                let name = l.name.as_deref().map(str::trim).unwrap_or_default().to_owned();
                if name.is_empty() || name.chars().count() > NAME_MAX || !name.chars().all(printable) {
                    return Err(AppError::validation("QUICK_SALE_NAME_INVALID", name));
                }
                let cost = match l.unit_cost {
                    Some(c) if (0..=MONEY_MAX).contains(&c) => {
                        (c.checked_mul(l.qty).ok_or_else(|| AppError::validation("LINE_INVALID", "overflow"))?, true)
                    }
                    Some(c) => return Err(AppError::validation("COST_INVALID", c.to_string())),
                    None => (0, false),
                };
                out.push(Line {
                    item_id: new_id(),
                    product_id: None,
                    custom_key: Some(quick_sale::custom_key(&name)),
                    name,
                    barcode: None,
                    list_price: l.unit_price,
                    unit_price: l.unit_price,
                    min_price: None,
                    qty: l.qty,
                    line_discount: l.line_discount,
                    tax_rate_bp: default_tax_bp,
                    tracked: false,
                    flat_cost: cost,
                    override_reason: None,
                });
            }
        }
    }
    Ok(out)
}

/// What needs a supervisor, as (perm to request, all perms the approver must hold).
fn approvals_needed(lines: &[Line], priced: &PricedOrder, bill_discount: i64, seller: &Seller) -> Vec<Perm> {
    let mut needed = Vec::new();
    let mut need = |p: Perm| {
        if !needed.contains(&p) {
            needed.push(p);
        }
    };
    if lines.iter().any(|l| l.line_discount > 0) && !seller.perms.has(Perm::LINE_DISCOUNT) {
        need(Perm::LINE_DISCOUNT);
    }
    if bill_discount > 0 && !seller.perms.has(Perm::ORDER_DISCOUNT) {
        need(Perm::ORDER_DISCOUNT);
    }
    if !pricing::lines_over_discount_limit(priced, seller.max_discount_bp).is_empty() {
        need(if bill_discount > 0 { Perm::ORDER_DISCOUNT } else { Perm::LINE_DISCOUNT });
    }
    for l in lines.iter().filter(|l| l.overridden()) {
        let cut = l.list_price - l.unit_price;
        let over_limit = i128::from(cut) * 10_000 > i128::from(seller.max_override_bp) * i128::from(l.list_price);
        let below_floor = l.min_price.is_some_and(|m| l.unit_price < m);
        if !seller.perms.has(Perm::PRICE_OVERRIDE) || over_limit || below_floor {
            need(Perm::PRICE_OVERRIDE);
        }
    }
    needed
}

/// The single perm the UI asks a supervisor for (one approval covers the whole order).
fn primary(needed: &[Perm]) -> Option<Perm> {
    [Perm::PRICE_OVERRIDE, Perm::ORDER_DISCOUNT, Perm::LINE_DISCOUNT].into_iter().find(|p| needed.contains(p))
}

fn approver_covers(a: &Approver, needed: &[Perm], lines: &[Line], priced: &PricedOrder) -> bool {
    needed.iter().all(|p| a.perms.has(*p))
        && pricing::lines_over_discount_limit(priced, a.max_discount_bp).is_empty()
        && lines.iter().filter(|l| l.overridden()).all(|l| {
            i128::from(l.list_price - l.unit_price) * 10_000 <= i128::from(a.max_override_bp) * i128::from(l.list_price)
        })
}

async fn load_approver(conn: &mut SqliteConnection, staff_id: &str) -> AppResult<Approver> {
    let row: Option<(i64, i64, i64, bool)> = sqlx::query_as(
        "SELECT perms, max_discount_bp, max_override_bp, is_active FROM v_staff_effective WHERE staff_id = ?1",
    )
    .bind(staff_id)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        Some((perms, max_discount_bp, max_override_bp, true)) => {
            Ok(Approver { staff_id: staff_id.to_owned(), perms: PermMask(perms), max_discount_bp, max_override_bp })
        }
        _ => Err(AppError::InvalidApproval),
    }
}

async fn existing(conn: &mut SqliteConnection, token: &str) -> AppResult<Option<CheckoutResult>> {
    let row: Option<(String, String, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, receipt_no, grand_total, change_due, customer_id FROM orders WHERE checkout_token = ?1",
    )
    .bind(token)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|(order_id, receipt_no, grand_total, change_due, customer_id)| CheckoutResult {
        order_id,
        receipt_no,
        grand_total,
        change_due,
        customer_id,
        replayed: true,
    }))
}

fn audit_ctx(ctx: &WriteCtx) -> AuditCtx<'_> {
    AuditCtx { store_id: &ctx.store_id, device_id: &ctx.device_id, business_date: &ctx.business_date, now: &ctx.now }
}

async fn audit_row(
    conn: &mut SqliteConnection,
    ctx: &WriteCtx,
    seller: &Seller,
    approved_by: Option<&str>,
    action: AuditAction,
    order_id: &str,
    metadata: Value,
) -> AppResult<()> {
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action,
            staff_id: Some(&seller.staff_id),
            approved_by,
            entity_type: Some("order"),
            entity_id: Some(order_id),
            metadata,
        },
    )
    .await
    .map(|_| ())
}

/// Sell a cart. `approver` is a supervisor already resolved from a consumed approval token (the command
/// layer retries with it after `ApprovalRequired`). See the module docs for the step order.
pub async fn checkout(
    db: &Db,
    ctx: &WriteCtx,
    seller: &Seller,
    input: &CheckoutInput,
    approver: Option<&str>,
    cp: &dyn Checkpoint,
) -> AppResult<CheckoutResult> {
    let token = input.checkout_token.trim();
    if token.is_empty() || token.len() > 64 {
        return Err(AppError::validation("CHECKOUT_TOKEN_INVALID", token.to_owned()));
    }
    let mut tx = db.write().await?;
    let conn = tx.conn();

    // 1. Idempotency: a double-tap or a retry after a lost response returns the original sale.
    if let Some(done) = existing(conn, token).await? {
        return Ok(done);
    }
    cp.reached("idempotency")?;

    // Validate and price everything before writing anything.
    let session_id = super::cash::require_open(conn, &ctx.device_id).await?;
    let tax_inclusive: bool = store_setting(conn, "tax.inclusive").await?.unwrap_or(true);
    let default_tax_bp: i64 = store_setting(conn, "tax.default_rate_bp").await?.unwrap_or(0);
    let lines = build_lines(conn, ctx, seller, input, default_tax_bp).await?;
    let pricing_input: Vec<LineInput> = lines
        .iter()
        .map(|l| LineInput {
            unit_price: l.unit_price,
            qty: l.qty,
            line_discount: l.line_discount,
            tax_rate_bp: l.tax_rate_bp,
        })
        .collect();
    let priced = pricing::price_lines(&pricing_input, input.bill_discount, tax_inclusive).map_err(pricing_err)?;
    let payments: Vec<PaymentInput> = input
        .payments
        .iter()
        .map(|p| PaymentInput { method: p.method, amount: p.amount, tendered: p.tendered })
        .collect();
    let change_due = validate_payments(priced.grand_total, &payments).map_err(payment_err)?;
    let references: Vec<Option<String>> =
        input.payments.iter().map(|p| opt_text(p.reference.as_deref(), 60)).collect::<AppResult<_>>()?;
    let note = opt_text(input.note.as_deref(), 500)?;
    let customer = match input.customer_phone.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(phone) => Some(customers::clean(phone, input.customer_name.as_deref())?),
        None => None,
    };

    // Permissions and limits (doc §3.7): one supervisor approval covers the whole order.
    let needed = approvals_needed(&lines, &priced, input.bill_discount, seller);
    let approver = match (primary(&needed), approver) {
        (None, _) => None,
        (Some(perm), None) => return Err(AppError::ApprovalRequired { perm }),
        (Some(_), Some(id)) => {
            if id == seller.staff_id {
                return Err(AppError::validation("SELF_APPROVAL", id.to_owned()));
            }
            let a = load_approver(conn, id).await?;
            if !approver_covers(&a, &needed, &lines, &priced) {
                return Err(AppError::validation("APPROVER_LIMIT_TOO_LOW", id.to_owned()));
            }
            Some(a)
        }
    };
    let approved_by = approver.as_ref().map(|a| a.staff_id.as_str());
    let actor = Actor { staff_id: seller.staff_id.clone(), approved_by: approved_by.map(str::to_owned) };

    // FIFO slices for tracked lines (planned before the header, which carries the cost total).
    let mut planned: HashMap<String, i64> = HashMap::new();
    let mut slices: Vec<Vec<Slice>> = Vec::with_capacity(lines.len());
    for (l, p) in lines.iter().zip(&priced.lines) {
        slices.push(match (&l.product_id, l.tracked) {
            (Some(pid), true) => stock::plan_sale_within(conn, ctx, pid, l.qty, p.line_net, &mut planned).await?,
            _ => Vec::new(),
        });
    }
    let line_costs: Vec<(i64, bool)> = lines
        .iter()
        .zip(&slices)
        .map(|(l, s)| if l.tracked { (s.iter().map(|x| x.qty * x.unit_cost).sum(), true) } else { l.flat_cost })
        .collect();
    let cost_total: i64 = line_costs.iter().map(|c| c.0).sum();
    let quick_sale_net: i64 =
        lines.iter().zip(&priced.lines).filter(|(l, _)| l.custom_key.is_some()).map(|(_, p)| p.line_net).sum();
    let unknown_cost_net: i64 =
        line_costs.iter().zip(&priced.lines).filter(|(c, _)| !c.1).map(|(_, p)| p.line_net).sum();
    let item_count: i64 = lines.iter().map(|l| l.qty).sum();
    cp.reached("validated")?;

    // 2. Customer, before the order (customer_id is frozen once the order exists).
    let customer_ref = match &customer {
        Some((phone, name)) => Some(customers::resolve_or_create(conn, ctx, &actor, phone, name.as_deref()).await?),
        None => None,
    };
    cp.reached("customer")?;

    // 3. Receipt number from this counter's per-business-day sequence.
    let seq: i64 = sqlx::query_scalar(
        "INSERT INTO receipt_counters (day, last_seq) VALUES (?1, 1)
         ON CONFLICT(day) DO UPDATE SET last_seq = last_seq + 1 RETURNING last_seq",
    )
    .bind(&ctx.business_date)
    .fetch_one(&mut *conn)
    .await?;
    let day = parse_date(&ctx.business_date).ok_or_else(|| AppError::internal("bad business date"))?;
    let receipt = receipt_no::receipt_no(&ctx.device_code, day, u32::try_from(seq).unwrap_or(u32::MAX));
    cp.reached("receipt_counter")?;

    // 4. The order, born pending.
    let order_id = new_id();
    sqlx::query(
        "INSERT INTO orders (id, store_id, device_id, cash_session_id, receipt_no, checkout_token, status, staff_id,
                             approved_by, customer_id, line_count, item_count, gross_sales, discount_total, net_sales,
                             tax_total, rounding, grand_total, paid_total, change_due, cost_total, quick_sale_net,
                             unknown_cost_net, business_date, completed_at, note, created_at, updated_at,
                             updated_by_device)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 0, ?16, ?16, ?17, ?18,
                 ?19, ?20, ?21, ?22, ?23, ?22, ?22, ?3)",
    )
    .bind(&order_id)
    .bind(&ctx.store_id)
    .bind(&ctx.device_id)
    .bind(&session_id)
    .bind(&receipt)
    .bind(token)
    .bind(&seller.staff_id)
    .bind(approved_by)
    .bind(customer_ref.as_ref().map(|c| c.id.as_str()))
    .bind(i64::try_from(lines.len()).unwrap_or(i64::MAX))
    .bind(item_count)
    .bind(priced.gross_sales)
    .bind(priced.discount_total)
    .bind(priced.net_sales)
    .bind(priced.tax_total)
    .bind(priced.grand_total)
    .bind(change_due)
    .bind(cost_total)
    .bind(quick_sale_net)
    .bind(unknown_cost_net)
    .bind(&ctx.business_date)
    .bind(&ctx.now)
    .bind(note.as_deref())
    .execute(&mut *conn)
    .await?;
    cp.reached("order_pending")?;

    // 5. Lines, their stock slices, and the payments.
    for (i, ((l, p), (s, cost))) in lines.iter().zip(&priced.lines).zip(slices.iter().zip(&line_costs)).enumerate() {
        let line_approver =
            if l.overridden() || l.line_discount + p.order_discount_share > 0 { approved_by } else { None };
        sqlx::query(
            "INSERT INTO order_items (id, store_id, device_id, order_id, line_no, product_id, is_custom, custom_key, name,
                                      barcode, list_price, unit_price, qty, line_gross, line_discount,
                                      order_discount_share, tax_rate_bp, tax_amount, line_total, line_net, cost_total,
                                      cost_known, price_overridden, override_reason, approved_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21,
                     ?22, ?23, ?24, ?25, ?26)",
        )
        .bind(&l.item_id)
        .bind(&ctx.store_id)
        .bind(&ctx.device_id)
        .bind(&order_id)
        .bind(i64::try_from(i + 1).unwrap_or(i64::MAX))
        .bind(l.product_id.as_deref())
        .bind(l.custom_key.is_some())
        .bind(l.custom_key.as_deref())
        .bind(&l.name)
        .bind(l.barcode.as_deref())
        .bind(l.list_price)
        .bind(l.unit_price)
        .bind(l.qty)
        .bind(p.line_gross)
        .bind(p.line_discount)
        .bind(p.order_discount_share)
        .bind(l.tax_rate_bp)
        .bind(p.tax_amount)
        .bind(p.line_total)
        .bind(p.line_net)
        .bind(cost.0)
        .bind(cost.1)
        .bind(l.overridden())
        .bind(l.override_reason.as_deref())
        .bind(line_approver)
        .bind(&ctx.now)
        .execute(&mut *conn)
        .await?;
        outbox::enqueue_upsert(conn, "order_items", &l.item_id, &ctx.now).await?;
        if let Some(pid) = &l.product_id {
            for slice in s {
                stock::record(
                    conn,
                    ctx,
                    &Movement {
                        product_id: pid,
                        batch_item_id: &slice.batch_item_id,
                        qty_delta: -slice.qty,
                        reason: StockReason::Sale,
                        ref_type: Some("order"),
                        ref_id: Some(&order_id),
                        order_item_id: Some(&l.item_id),
                        unit_cost: slice.unit_cost,
                        revenue_net: Some(slice.revenue_net),
                        staff_id: Some(&seller.staff_id),
                        approved_by: line_approver,
                        note: None,
                    },
                )
                .await?;
            }
        }
        cp.reached("line")?;
    }
    for (p, reference) in input.payments.iter().zip(&references) {
        let id = new_id();
        sqlx::query(
            "INSERT INTO order_payments (id, store_id, device_id, order_id, method, amount, tendered, reference,
                                         created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(&id)
        .bind(&ctx.store_id)
        .bind(&ctx.device_id)
        .bind(&order_id)
        .bind(p.method.as_db())
        .bind(p.amount)
        .bind(p.tendered)
        .bind(reference.as_deref())
        .bind(&ctx.now)
        .execute(&mut *conn)
        .await?;
        outbox::enqueue_upsert(conn, "order_payments", &id, &ctx.now).await?;
    }
    cp.reached("payments")?;

    // 6. Seal: SQL re-verifies totals, slices, limits, approvers and the open session.
    sqlx::query("UPDATE orders SET status = 'completed' WHERE id = ?1").bind(&order_id).execute(&mut *conn).await?;
    cp.reached("sealed")?;

    // 7. Audit what needed authority.
    for (i, l) in lines.iter().enumerate().filter(|(_, l)| l.overridden()) {
        audit_row(
            conn,
            ctx,
            seller,
            approved_by,
            AuditAction::PRICE_OVERRIDE,
            &order_id,
            json!({ "receipt_no": receipt, "line_no": i + 1, "name": l.name, "list_price": l.list_price,
                    "unit_price": l.unit_price, "qty": l.qty, "reason": l.override_reason }),
        )
        .await?;
    }
    if let Some(a) = &approver {
        for perm in [Perm::LINE_DISCOUNT, Perm::ORDER_DISCOUNT].into_iter().filter(|p| needed.contains(p)) {
            let action =
                if perm == Perm::ORDER_DISCOUNT { AuditAction::ORDER_DISCOUNT } else { AuditAction::LINE_DISCOUNT };
            audit_row(
                conn,
                ctx,
                seller,
                Some(&a.staff_id),
                action,
                &order_id,
                json!({ "receipt_no": receipt, "discount_total": priced.discount_total,
                        "bill_discount": input.bill_discount, "gross_sales": priced.gross_sales,
                        "seller_limit_bp": seller.max_discount_bp }),
            )
            .await?;
        }
    }
    cp.reached("audit")?;

    // 8. The recovered cart can never be sold twice.
    sqlx::query("DELETE FROM held_carts WHERE id = ?1").bind(CURRENT_CART).execute(&mut *conn).await?;
    cp.reached("held_cart")?;

    // 9. Receipt (and the drawer for cash) — printed by the worker after commit, never before.
    print_jobs::enqueue(conn, ctx, PrintKind::Receipt, Some(&order_id), &json!({ "order_id": order_id })).await?;
    if input.payments.iter().any(|p| p.method == PaymentMethod::Cash) {
        print_jobs::enqueue(conn, ctx, PrintKind::DrawerKick, Some(&order_id), &json!({ "reason": "sale" })).await?;
    }
    cp.reached("print_jobs")?;

    // 10. The sealed order for sync, then its lines, payments and slices behind it: they were queued as
    //     written, before the order could be sealed, and the cloud needs the parent first (§6.1).
    outbox::enqueue_upsert(conn, "orders", &order_id, &ctx.now).await?;
    let children: Vec<i64> = sqlx::query_scalar(
        "SELECT q.seq FROM sync_outbox q
         WHERE (q.table_name = 'order_items' AND q.row_id IN (SELECT id FROM order_items WHERE order_id = ?1))
            OR (q.table_name = 'order_payments' AND q.row_id IN (SELECT id FROM order_payments WHERE order_id = ?1))
            OR (q.table_name = 'stock_movements' AND q.row_id IN
                  (SELECT m.id FROM stock_movements m JOIN order_items oi ON oi.id = m.order_item_id
                   WHERE oi.order_id = ?1))
         ORDER BY q.seq",
    )
    .bind(&order_id)
    .fetch_all(&mut *conn)
    .await?;
    outbox::requeue_after(conn, &children).await?;
    cp.reached("outbox")?;

    tx.commit().await?;
    Ok(CheckoutResult {
        order_id,
        receipt_no: receipt,
        grand_total: priced.grand_total,
        change_due,
        customer_id: customer_ref.map(|c| c.id),
        replayed: false,
    })
}
