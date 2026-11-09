//! Operating expenses (doc §5.1, Block 2.6): categories, entry, edit, delete and the list for a §5 period.
//! An expense can be **paid from the drawer**: then it is cash, dated today, needs this counter's open session,
//! and writes a `pay_out` cash movement (plus a drawer kick) in the same transaction, so the Z-report's
//! expected cash reconciles. Cash movements are append-only, so a drawer-paid expense keeps its amount, date
//! and method; deleting it while its session is still open appends a matching `pay_in` (the cash went back
//! into the drawer). Batch purchases are never expenses (they are inventory; §5.1).

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use super::WriteCtx;
use super::cash;
use crate::audit::{self, AuditAction, AuditCtx, AuditEntry};
use crate::auth::guard::Actor;
use crate::clock::new_id;
use crate::db::{Db, outbox};
use crate::domain::business_date::{StoreCalendar, parse_date};
use crate::domain::payments::PaymentMethod;
use crate::domain::period::{DateRange, Period};
use crate::error::{AppError, AppResult};

const MONEY_MAX: i64 = 10_000_000_000;
const NAME_MAX: usize = 60;
const TEXT_MAX: usize = 200;

fn audit_ctx(ctx: &WriteCtx) -> AuditCtx<'_> {
    AuditCtx { store_id: &ctx.store_id, device_id: &ctx.device_id, business_date: &ctx.business_date, now: &ctx.now }
}

fn tidy(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

// ───────── categories ─────────

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct ExpenseCategory {
    pub id: String,
    pub name: String,
    pub is_active: bool,
    /// Expenses recorded against it (not deleted): a used category can be retired, never removed.
    #[specta(type = specta_typescript::Number)]
    pub used: i64,
}

#[derive(Debug, Clone, Deserialize, Type)]
pub struct ExpenseCategoryInput {
    pub name: String,
    pub is_active: bool,
}

pub async fn categories(pool: &SqlitePool, store_id: &str) -> AppResult<Vec<ExpenseCategory>> {
    Ok(sqlx::query_as(
        "SELECT c.id, c.name, c.is_active,
                (SELECT COUNT(*) FROM expenses e WHERE e.category_id = c.id AND e.deleted_at IS NULL) AS used
         FROM expense_categories c
         WHERE c.store_id = ?1 AND c.deleted_at IS NULL
         ORDER BY c.is_active DESC, c.name COLLATE NOCASE",
    )
    .bind(store_id)
    .fetch_all(pool)
    .await?)
}

fn clean_category_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(AppError::validation("EXPENSE_CATEGORY_NAME_INVALID", format!("1–{NAME_MAX} characters")));
    }
    Ok(name.to_owned())
}

async fn name_taken(conn: &mut SqliteConnection, store_id: &str, name: &str, except: Option<&str>) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM expense_categories
         WHERE store_id = ?1 AND deleted_at IS NULL AND name = ?2 COLLATE NOCASE AND (?3 IS NULL OR id <> ?3)",
    )
    .bind(store_id)
    .bind(name)
    .bind(except)
    .fetch_one(&mut *conn)
    .await?;
    Ok(n > 0)
}

/// Create (`id = None`) or rename / retire a category. Audited as `SETTINGS_CHANGE` (categories are
/// configuration, not money).
pub async fn save_category(
    db: &Db,
    ctx: &WriteCtx,
    actor: &Actor,
    id: Option<&str>,
    input: &ExpenseCategoryInput,
) -> AppResult<String> {
    let name = clean_category_name(&input.name)?;
    let mut tx = db.write().await?;
    let conn = tx.conn();
    if let Some(id) = id {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM expense_categories WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(&ctx.store_id)
        .fetch_one(&mut *conn)
        .await?;
        if exists == 0 {
            return Err(AppError::not_found("expense_category"));
        }
    }
    if name_taken(conn, &ctx.store_id, &name, id).await? {
        return Err(AppError::validation("EXPENSE_CATEGORY_EXISTS", name));
    }
    let id = match id {
        None => {
            let id = new_id();
            sqlx::query(
                "INSERT INTO expense_categories (id, store_id, name, is_active, created_at, updated_at, updated_by_device)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            )
            .bind(&id)
            .bind(&ctx.store_id)
            .bind(&name)
            .bind(input.is_active)
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .execute(&mut *conn)
            .await?;
            id
        }
        Some(id) => {
            let done = sqlx::query(
                "UPDATE expense_categories SET name = ?1, is_active = ?2, updated_at = ?3, updated_by_device = ?4,
                        version = version + 1
                 WHERE id = ?5 AND store_id = ?6 AND deleted_at IS NULL",
            )
            .bind(&name)
            .bind(input.is_active)
            .bind(&ctx.now)
            .bind(&ctx.device_id)
            .bind(id)
            .bind(&ctx.store_id)
            .execute(&mut *conn)
            .await?;
            if done.rows_affected() == 0 {
                return Err(AppError::not_found("expense_category"));
            }
            id.to_owned()
        }
    };
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::SETTINGS_CHANGE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("expense_category"),
            entity_id: Some(&id),
            metadata: json!({ "name": name, "is_active": input.is_active }),
        },
    )
    .await?;
    outbox::enqueue_upsert(conn, "expense_categories", &id, &ctx.now).await?;
    tx.commit().await?;
    Ok(id)
}

// ───────── expenses ─────────

#[derive(Debug, Clone, Deserialize, Type)]
pub struct ExpenseInput {
    pub category_id: String,
    #[specta(type = specta_typescript::Number)]
    pub amount: i64,
    /// The expense's business date; `None` = today. Never in the future.
    pub business_date: Option<String>,
    pub payment_method: PaymentMethod,
    /// Cash taken from this counter's drawer now (a `pay_out` in the open session).
    pub paid_from_drawer: bool,
    pub payee: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, FromRow)]
pub struct ExpenseRow {
    pub id: String,
    pub category_id: String,
    pub category_name: String,
    #[specta(type = specta_typescript::Number)]
    pub amount: i64,
    pub business_date: String,
    pub payment_method: String,
    pub paid_from_drawer: bool,
    pub cash_session_id: Option<String>,
    pub payee: Option<String>,
    pub note: Option<String>,
    pub recorded_by_name: Option<String>,
    pub created_at: String,
}

const EXPENSE_SELECT: &str = "SELECT e.id, e.category_id, c.name AS category_name, e.amount, e.business_date,
        e.payment_method, e.paid_from_drawer, e.cash_session_id, e.payee, e.note,
        s.display_name AS recorded_by_name, e.created_at
 FROM expenses e
 JOIN expense_categories c ON c.id = e.category_id
 LEFT JOIN staff s ON s.id = e.recorded_by";

struct Clean {
    amount: i64,
    business_date: String,
    payee: Option<String>,
    note: Option<String>,
}

fn clean(ctx: &WriteCtx, input: &ExpenseInput) -> AppResult<Clean> {
    if !(1..=MONEY_MAX).contains(&input.amount) {
        return Err(AppError::validation("AMOUNT_INVALID", input.amount.to_string()));
    }
    let business_date = match input.business_date.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => ctx.business_date.clone(),
        Some(d) => {
            let date = parse_date(d).ok_or_else(|| AppError::validation("EXPENSE_DATE_INVALID", d.to_owned()))?;
            if parse_date(&ctx.business_date).is_some_and(|today| date > today) {
                return Err(AppError::validation("EXPENSE_DATE_INVALID", "in the future"));
            }
            d.to_owned()
        }
    };
    if input.paid_from_drawer {
        if input.payment_method != PaymentMethod::Cash {
            return Err(AppError::validation("EXPENSE_DRAWER_CASH_ONLY", input.payment_method.as_db()));
        }
        if business_date != ctx.business_date {
            return Err(AppError::validation("EXPENSE_DRAWER_TODAY_ONLY", business_date));
        }
    }
    let (payee, note) = (tidy(input.payee.as_deref()), tidy(input.note.as_deref()));
    if payee.as_ref().is_some_and(|p| p.chars().count() > TEXT_MAX)
        || note.as_ref().is_some_and(|n| n.chars().count() > TEXT_MAX)
    {
        return Err(AppError::validation("EXPENSE_TEXT_TOO_LONG", format!("max {TEXT_MAX}")));
    }
    Ok(Clean { amount: input.amount, business_date, payee, note })
}

async fn active_category(conn: &mut SqliteConnection, store_id: &str, id: &str) -> AppResult<String> {
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT name, is_active FROM expense_categories WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(store_id)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        None => Err(AppError::not_found("expense_category")),
        Some((_, false)) => Err(AppError::validation("EXPENSE_CATEGORY_INACTIVE", id.to_owned())),
        Some((name, true)) => Ok(name),
    }
}

async fn get_on(conn: &mut SqliteConnection, id: &str) -> AppResult<ExpenseRow> {
    sqlx::query_as(&format!("{EXPENSE_SELECT} WHERE e.id = ?1 AND e.deleted_at IS NULL"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::not_found("expense"))
}

/// The drawer reason a pay-out or its reversal carries (shown on the Z-report).
fn drawer_reason(prefix: &str, category: &str, payee: Option<&str>) -> String {
    let mut r = format!("{prefix}: {category}");
    if let Some(p) = payee {
        r.push_str(" – ");
        r.push_str(p);
    }
    r.chars().take(TEXT_MAX).collect()
}

/// Record an expense. Paid from the drawer ⇒ also a `pay_out` (and drawer kick) in this counter's open
/// session, audited as `CASH_PAY_OUT` beside `EXPENSE_CREATE`.
pub async fn create(db: &Db, ctx: &WriteCtx, actor: &Actor, input: &ExpenseInput) -> AppResult<ExpenseRow> {
    let c = clean(ctx, input)?;
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let category = active_category(conn, &ctx.store_id, &input.category_id).await?;
    let session_id = if input.paid_from_drawer {
        let session_id = cash::require_open(conn, &ctx.device_id).await?;
        let reason = drawer_reason("Expense", &category, c.payee.as_deref());
        let movement = cash::insert_movement(conn, ctx, actor, &session_id, "pay_out", c.amount, &reason).await?;
        audit::record(
            conn,
            &audit_ctx(ctx),
            AuditEntry {
                action: AuditAction::CASH_PAY_OUT,
                staff_id: Some(&actor.staff_id),
                approved_by: actor.approved_by.as_deref(),
                entity_type: Some("cash_movement"),
                entity_id: Some(&movement),
                metadata: json!({ "amount": c.amount, "reason": reason }),
            },
        )
        .await?;
        Some(session_id)
    } else {
        None
    };
    let id = new_id();
    sqlx::query(
        "INSERT INTO expenses (id, store_id, category_id, amount, business_date, payment_method, paid_from_drawer,
                               cash_session_id, payee, note, recorded_by, created_at, updated_at, updated_by_device)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13)",
    )
    .bind(&id)
    .bind(&ctx.store_id)
    .bind(&input.category_id)
    .bind(c.amount)
    .bind(&c.business_date)
    .bind(input.payment_method.as_db())
    .bind(input.paid_from_drawer)
    .bind(session_id.as_deref())
    .bind(c.payee.as_deref())
    .bind(c.note.as_deref())
    .bind(&actor.staff_id)
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .execute(&mut *conn)
    .await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::EXPENSE_CREATE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("expense"),
            entity_id: Some(&id),
            metadata: json!({
                "amount": c.amount, "category": category, "business_date": c.business_date,
                "method": input.payment_method.as_db(), "paid_from_drawer": input.paid_from_drawer,
            }),
        },
    )
    .await?;
    outbox::enqueue_upsert(conn, "expenses", &id, &ctx.now).await?;
    let row = get_on(conn, &id).await?;
    tx.commit().await?;
    Ok(row)
}

/// Edit an expense. A drawer-paid one keeps its amount, date, method and drawer link (its cash movement is
/// append-only); only the category, payee and note can change.
pub async fn update(db: &Db, ctx: &WriteCtx, actor: &Actor, id: &str, input: &ExpenseInput) -> AppResult<ExpenseRow> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let before = get_on(conn, id).await?;
    if before.paid_from_drawer {
        let locked_changed = input.amount != before.amount
            || input.payment_method.as_db() != before.payment_method
            || !input.paid_from_drawer
            || input.business_date.as_deref().is_some_and(|d| d.trim() != before.business_date);
        if locked_changed {
            return Err(AppError::validation("EXPENSE_DRAWER_LOCKED", id.to_owned()));
        }
    } else if input.paid_from_drawer {
        return Err(AppError::validation("EXPENSE_DRAWER_LOCKED", "a recorded expense can't move into the drawer"));
    }
    // A drawer-paid expense was validated against its own day; keep it.
    let check_ctx = WriteCtx {
        business_date: if before.paid_from_drawer { before.business_date.clone() } else { ctx.business_date.clone() },
        ..ctx.clone()
    };
    let c = clean(&check_ctx, input)?;
    let category = if input.category_id == before.category_id {
        before.category_name.clone()
    } else {
        active_category(conn, &ctx.store_id, &input.category_id).await?
    };
    sqlx::query(
        "UPDATE expenses SET category_id = ?1, amount = ?2, business_date = ?3, payment_method = ?4, payee = ?5,
                note = ?6, updated_at = ?7, updated_by_device = ?8, version = version + 1
         WHERE id = ?9",
    )
    .bind(&input.category_id)
    .bind(c.amount)
    .bind(&c.business_date)
    .bind(input.payment_method.as_db())
    .bind(c.payee.as_deref())
    .bind(c.note.as_deref())
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::EXPENSE_UPDATE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("expense"),
            entity_id: Some(id),
            metadata: json!({
                "before": { "amount": before.amount, "category": before.category_name,
                            "business_date": before.business_date, "method": before.payment_method },
                "after": { "amount": c.amount, "category": category,
                           "business_date": c.business_date, "method": input.payment_method.as_db() },
            }),
        },
    )
    .await?;
    outbox::enqueue_upsert(conn, "expenses", id, &ctx.now).await?;
    let row = get_on(conn, id).await?;
    tx.commit().await?;
    Ok(row)
}

/// Delete (soft) an expense. A drawer-paid one can only go while its session is still open on this counter,
/// and the cash goes back: a matching `pay_in` is appended (`CASH_PAY_IN`).
pub async fn delete(db: &Db, ctx: &WriteCtx, actor: &Actor, id: &str) -> AppResult<()> {
    let mut tx = db.write().await?;
    let conn = tx.conn();
    let before = get_on(conn, id).await?;
    if before.paid_from_drawer {
        let session_id = match cash::require_open(conn, &ctx.device_id).await {
            Ok(open) if before.cash_session_id.as_deref() == Some(open.as_str()) => open,
            _ => return Err(AppError::validation("EXPENSE_DRAWER_LOCKED", "the drawer session is closed")),
        };
        let reason = drawer_reason("Expense deleted", &before.category_name, before.payee.as_deref());
        let movement = cash::insert_movement(conn, ctx, actor, &session_id, "pay_in", before.amount, &reason).await?;
        audit::record(
            conn,
            &audit_ctx(ctx),
            AuditEntry {
                action: AuditAction::CASH_PAY_IN,
                staff_id: Some(&actor.staff_id),
                approved_by: actor.approved_by.as_deref(),
                entity_type: Some("cash_movement"),
                entity_id: Some(&movement),
                metadata: json!({ "amount": before.amount, "reason": reason }),
            },
        )
        .await?;
    }
    sqlx::query(
        "UPDATE expenses SET deleted_at = ?1, updated_at = ?1, updated_by_device = ?2, version = version + 1
         WHERE id = ?3",
    )
    .bind(&ctx.now)
    .bind(&ctx.device_id)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    audit::record(
        conn,
        &audit_ctx(ctx),
        AuditEntry {
            action: AuditAction::EXPENSE_DELETE,
            staff_id: Some(&actor.staff_id),
            approved_by: actor.approved_by.as_deref(),
            entity_type: Some("expense"),
            entity_id: Some(id),
            metadata: json!({ "amount": before.amount, "category": before.category_name,
                              "business_date": before.business_date, "paid_from_drawer": before.paid_from_drawer }),
        },
    )
    .await?;
    outbox::enqueue_upsert(conn, "expenses", id, &ctx.now).await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CategoryTotal {
    pub category_id: String,
    pub name: String,
    #[specta(type = specta_typescript::Number)]
    pub amount: i64,
    #[specta(type = specta_typescript::Number)]
    pub count: i64,
}

/// Expenses in a §5 period (newest first) with totals by category — the P&L's operating expenses.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct ExpenseList {
    pub range: DateRange,
    pub rows: Vec<ExpenseRow>,
    pub by_category: Vec<CategoryTotal>,
    #[specta(type = specta_typescript::Number)]
    pub total: i64,
    #[specta(type = specta_typescript::Number)]
    pub from_drawer: i64,
}

pub async fn list(
    pool: &SqlitePool,
    store_id: &str,
    cal: &StoreCalendar,
    period: &Period,
    category_id: Option<&str>,
) -> AppResult<ExpenseList> {
    let range = period.resolve(cal)?;
    let rows: Vec<ExpenseRow> = sqlx::query_as(&format!(
        "{EXPENSE_SELECT}
         WHERE e.store_id = ?1 AND e.deleted_at IS NULL
           AND (?2 IS NULL OR e.business_date >= ?2) AND (?3 IS NULL OR e.business_date <= ?3)
           AND (?4 IS NULL OR e.category_id = ?4)
         ORDER BY e.business_date DESC, e.created_at DESC, e.id DESC"
    ))
    .bind(store_id)
    .bind(range.from.as_deref())
    .bind(range.to.as_deref())
    .bind(category_id)
    .fetch_all(pool)
    .await?;
    let mut by_category: Vec<CategoryTotal> = Vec::new();
    for r in &rows {
        match by_category.iter_mut().find(|t| t.category_id == r.category_id) {
            Some(t) => {
                t.amount += r.amount;
                t.count += 1;
            }
            None => by_category.push(CategoryTotal {
                category_id: r.category_id.clone(),
                name: r.category_name.clone(),
                amount: r.amount,
                count: 1,
            }),
        }
    }
    by_category.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.name.cmp(&b.name)));
    let total = rows.iter().map(|r| r.amount).sum();
    let from_drawer = rows.iter().filter(|r| r.paid_from_drawer).map(|r| r.amount).sum();
    Ok(ExpenseList { range, rows, by_category, total, from_drawer })
}

/// Whether an expense moved drawer cash (the command then also needs `CASH_PAY_IN_OUT` to delete it).
pub async fn paid_from_drawer(pool: &SqlitePool, id: &str) -> AppResult<bool> {
    sqlx::query_scalar("SELECT paid_from_drawer FROM expenses WHERE id = ?1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("expense"))
}
