//! One salesperson's sales (Staff screen → "Sales performance"): the daily totals of the last 30 business days
//! for the chart, and the week, month and year totals for the three cards. A sale belongs to the person whose
//! staff number was typed at the till (`orders.sold_by`; older orders fall back to the logged-in cashier).
//! Only completed orders count, so a voided sale drops out. Money is integer minor units (`grand_total`).

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;
use specta::Type;
use sqlx::{FromRow, SqlitePool};

use crate::domain::business_date::{StoreCalendar, format_date};
use crate::error::{AppError, AppResult};

/// Days shown on the daily chart, ending today.
pub const CHART_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct DailySales {
    /// Business date, `YYYY-MM-DD`.
    pub date: String,
    #[specta(type = specta_typescript::Number)]
    pub total: i64,
    #[specta(type = specta_typescript::Number)]
    pub orders: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq, Default)]
pub struct PeriodSales {
    #[specta(type = specta_typescript::Number)]
    pub total: i64,
    #[specta(type = specta_typescript::Number)]
    pub orders: i64,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct StaffSales {
    pub staff_id: String,
    /// The business date the figures are relative to.
    pub today: String,
    /// Every day of the last [`CHART_DAYS`], oldest first; days without sales are zero.
    pub daily: Vec<DailySales>,
    /// The store week (Monday start by default) containing today.
    pub week: PeriodSales,
    /// The calendar month containing today.
    pub month: PeriodSales,
    /// The calendar year containing today.
    pub year: PeriodSales,
}

#[derive(FromRow)]
struct DayRow {
    business_date: String,
    total: i64,
    orders: i64,
}

fn add(p: &mut PeriodSales, total: i64, orders: i64) {
    p.total = p.total.saturating_add(total);
    p.orders = p.orders.saturating_add(orders);
}

pub async fn sales(pool: &SqlitePool, cal: &StoreCalendar, today: NaiveDate, staff_id: &str) -> AppResult<StaffSales> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM staff WHERE id = ?1 AND deleted_at IS NULL")
        .bind(staff_id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::validation("STAFF_NOT_FOUND", staff_id.to_owned()));
    }
    let year_start = NaiveDate::from_ymd_opt(today.year(), 1, 1).ok_or_else(|| AppError::internal("bad date"))?;
    let month_start =
        NaiveDate::from_ymd_opt(today.year(), today.month(), 1).ok_or_else(|| AppError::internal("bad date"))?;
    let (week_start, week_end) = cal.week_of(today);
    let chart_start = today - Duration::days(CHART_DAYS - 1);
    let from = year_start.min(chart_start);

    // `ix_orders_sold_by` serves this exact expression and predicate.
    let rows: Vec<DayRow> = sqlx::query_as(
        "SELECT business_date, SUM(grand_total - exchange_credit) AS total, COUNT(*) AS orders FROM orders
         WHERE status = 'completed' AND COALESCE(sold_by, staff_id) = ?1 AND business_date BETWEEN ?2 AND ?3
         GROUP BY business_date",
    )
    .bind(staff_id)
    .bind(format_date(from))
    .bind(format_date(today))
    .fetch_all(pool)
    .await?;
    let by_day: HashMap<&str, (i64, i64)> =
        rows.iter().map(|r| (r.business_date.as_str(), (r.total, r.orders))).collect();

    let (mut week, mut month, mut year) = (PeriodSales::default(), PeriodSales::default(), PeriodSales::default());
    for r in &rows {
        let Some(d) = crate::domain::business_date::parse_date(&r.business_date) else { continue };
        if d >= year_start {
            add(&mut year, r.total, r.orders);
        }
        if d >= month_start {
            add(&mut month, r.total, r.orders);
        }
        if d >= week_start && d <= week_end {
            add(&mut week, r.total, r.orders);
        }
    }
    let daily = (0..CHART_DAYS)
        .map(|i| {
            let date = format_date(chart_start + Duration::days(i));
            let (total, orders) = by_day.get(date.as_str()).copied().unwrap_or((0, 0));
            DailySales { date, total, orders }
        })
        .collect();
    Ok(StaffSales { staff_id: staff_id.to_owned(), today: format_date(today), daily, week, month, year })
}
