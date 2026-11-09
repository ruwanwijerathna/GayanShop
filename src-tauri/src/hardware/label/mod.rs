//! Barcode labels (doc §7.4): one layout engine (`layout`) feeding the SVG preview and two printer languages
//! (`tspl`, `zpl`) on multi-column rolls: the built-in 2-up 2″×1″ and 3-up 1″×1″ templates at 203 dpi with a
//! 2 mm die-cut gap. Templates and the label printer profile (language, dpi, darkness, speed, calibration
//! offsets) are device-local, because each counter's printer and roll can differ.

pub mod barcode;
pub mod layout;
pub mod tspl;
pub mod zpl;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::db::settings;
use crate::domain::money::format_label_price;
use crate::error::{AppError, AppResult};
use crate::hardware::printer::TransportConfig;
use layout::{LabelCell, LabelLanguage, LabelPreview, LabelTemplate};

/// `print_jobs.printer_key` (and `device_settings` `printer.label`) of this counter's label printer.
pub const LABEL_PRINTER: &str = "label";
pub const TEMPLATES_KEY: &str = "label.templates";
/// Copies per product and labels per job, so a typo can't empty a roll.
pub const MAX_COPIES: i64 = 999;
pub const MAX_LABELS: i64 = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct LabelPrinterProfile {
    pub transport: TransportConfig,
    pub language: LabelLanguage,
    /// 203 or 300.
    pub dpi: u16,
    /// TSPL `DENSITY` 0–15 / ZPL darkness.
    pub darkness: u8,
    /// Inches per second, 1–6.
    pub speed: u8,
    /// Calibration: shift everything right / down, mm.
    #[specta(type = specta_typescript::Number)]
    pub offset_x_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub offset_y_mm: f64,
}

impl LabelPrinterProfile {
    pub fn offset(&self) -> (f64, f64) {
        (self.offset_x_mm, self.offset_y_mm)
    }

    pub fn validate(&self) -> AppResult<()> {
        let bad = |why: &str| Err(AppError::validation("LABEL_PROFILE_INVALID", why.to_owned()));
        if !matches!(self.dpi, 203 | 300) {
            return bad("dpi");
        }
        if self.darkness > 15 || !(1..=6).contains(&self.speed) {
            return bad("darkness or speed");
        }
        if !(-10.0..=10.0).contains(&self.offset_x_mm) || !(-10.0..=10.0).contains(&self.offset_y_mm) {
            return bad("offset");
        }
        Ok(())
    }
}

pub async fn load_profile(pool: &SqlitePool) -> AppResult<Option<LabelPrinterProfile>> {
    settings::device_get(pool, &crate::hardware::printer::profile_key(LABEL_PRINTER)).await
}

/// The built-in templates as first-run seeded them (§11): "Reset to default" restores these.
pub fn builtin_templates() -> AppResult<Vec<LabelTemplate>> {
    Ok(serde_json::from_value(crate::setup::builtin_label_templates())?)
}

pub async fn templates(pool: &SqlitePool) -> AppResult<Vec<LabelTemplate>> {
    let stored: Option<Vec<LabelTemplate>> = settings::device_get(pool, TEMPLATES_KEY).await?;
    match stored {
        Some(t) if !t.is_empty() => Ok(t),
        _ => builtin_templates(),
    }
}

pub async fn template(pool: &SqlitePool, id: &str) -> AppResult<LabelTemplate> {
    templates(pool).await?.into_iter().find(|t| t.id == id).ok_or_else(|| AppError::not_found("label template"))
}

/// Calibration of a template (Settings → Labels): roll geometry only; the element boxes are the built-ins'.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct TemplateCalibration {
    #[specta(type = specta_typescript::Number)]
    pub col_gap_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub row_gap_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub left_margin_mm: f64,
}

fn in_range(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

/// Save a calibrated template; `None` restores the built-in ("Reset to default"). Built-ins can't be deleted.
pub async fn calibrate(
    conn: &mut SqliteConnection,
    pool: &SqlitePool,
    id: &str,
    cal: Option<&TemplateCalibration>,
) -> AppResult<LabelTemplate> {
    let mut all = templates(pool).await?;
    let builtin = builtin_templates()?.into_iter().find(|t| t.id == id);
    let slot = all.iter_mut().find(|t| t.id == id).ok_or_else(|| AppError::not_found("label template"))?;
    match cal {
        None => {
            *slot = builtin.ok_or_else(|| AppError::not_found("built-in label template"))?;
        }
        Some(c) => {
            if !in_range(c.col_gap_mm, 0.0, 10.0)
                || !in_range(c.row_gap_mm, 0.0, 10.0)
                || !in_range(c.left_margin_mm, 0.0, 10.0)
            {
                return Err(AppError::validation("LABEL_CALIBRATION_INVALID", "0–10 mm"));
            }
            slot.col_gap_mm = c.col_gap_mm;
            slot.row_gap_mm = c.row_gap_mm;
            slot.left_margin_mm = c.left_margin_mm;
        }
    }
    let saved = slot.clone();
    settings::device_set(conn, TEMPLATES_KEY, &all).await?;
    Ok(saved)
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct LabelJobItem {
    pub product_id: String,
    #[specta(type = specta_typescript::Number)]
    pub copies: i64,
}

/// What a label job prints (also the `print_jobs` payload, re-laid out from the database at print time).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct LabelJob {
    pub template_id: String,
    pub items: Vec<LabelJobItem>,
    /// Fill a partial last row with copies of the last label instead of leaving it blank.
    #[serde(default)]
    pub fill_last_row: bool,
}

impl LabelJob {
    pub fn validate(&self) -> AppResult<i64> {
        if self.items.is_empty() {
            return Err(AppError::validation("LABEL_JOB_EMPTY", "no products"));
        }
        let mut total: i64 = 0;
        for i in &self.items {
            if !(1..=MAX_COPIES).contains(&i.copies) {
                return Err(AppError::validation("LABEL_COPIES_INVALID", i.copies.to_string()));
            }
            total += i.copies;
        }
        if total > MAX_LABELS {
            return Err(AppError::validation("LABEL_JOB_TOO_LARGE", total.to_string()));
        }
        Ok(total)
    }
}

#[derive(FromRow)]
struct ProductRow {
    id: String,
    name: String,
    price: i64,
    sku: Option<String>,
    barcode: Option<String>,
    attributes: String,
}

/// `size` first, then the other attributes in name order, joined with " - " (the printer fonts have no `·`).
pub fn attributes_line(json: &str) -> String {
    let map: BTreeMap<String, Value> = serde_json::from_str(json).unwrap_or_default();
    let text = |v: &Value| match v {
        Value::String(s) => s.trim().to_owned(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(size) = map.iter().find(|(k, _)| k.eq_ignore_ascii_case("size")) {
        parts.push(format!("Size {}", text(size.1)));
    }
    parts.extend(map.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("size")).map(|(_, v)| text(v)));
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" - ")
}

/// One cell per copy, in job order.
pub async fn cells(pool: &SqlitePool, store_id: &str, job: &LabelJob) -> AppResult<Vec<LabelCell>> {
    let store_name: String = settings::store_get(pool, settings::keys::STORE_NAME).await?.unwrap_or_default();
    let mut out = Vec::new();
    for item in &job.items {
        let p: ProductRow = sqlx::query_as(
            "SELECT id, name, price, sku, barcode, attributes FROM products
             WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL",
        )
        .bind(&item.product_id)
        .bind(store_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::validation("LABEL_PRODUCT_INVALID", item.product_id.clone()))?;
        let cell = LabelCell {
            product_id: p.id,
            store_name: store_name.clone(),
            name: p.name,
            attributes: attributes_line(&p.attributes),
            price: format_label_price(p.price),
            codes: [p.barcode, p.sku].into_iter().flatten().collect(),
        };
        for _ in 0..item.copies {
            out.push(cell.clone());
        }
    }
    Ok(out)
}

/// The template as this counter's printer will print it (its language and dpi).
pub fn for_printer(mut t: LabelTemplate, profile: Option<&LabelPrinterProfile>) -> LabelTemplate {
    if let Some(p) = profile {
        t.language = p.language;
        t.dpi = p.dpi;
    }
    t
}

pub async fn preview(pool: &SqlitePool, store_id: &str, job: &LabelJob) -> AppResult<LabelPreview> {
    job.validate()?;
    let profile = load_profile(pool).await?;
    let t = for_printer(template(pool, &job.template_id).await?, profile.as_ref());
    let cells = cells(pool, store_id, job).await?;
    let offset = profile.as_ref().map_or((0.0, 0.0), LabelPrinterProfile::offset);
    Ok(layout::layout(&t, &cells, offset, job.fill_last_row))
}

/// Printer bytes for a job.
pub fn emit(preview: &LabelPreview, profile: &LabelPrinterProfile) -> Vec<u8> {
    match profile.language {
        LabelLanguage::Tspl => tspl::emit(preview, profile.darkness, profile.speed),
        LabelLanguage::Zpl => zpl::emit(preview, profile.darkness),
    }
}

/// The print worker's renderer for the label printer: `labels` jobs and the `test` (alignment) job.
pub async fn render_job(
    pool: &SqlitePool,
    kind: &str,
    payload: &Value,
    profile: &LabelPrinterProfile,
) -> AppResult<Vec<u8>> {
    match kind {
        "labels" => {
            let job: LabelJob = serde_json::from_value(payload.clone())?;
            let store_id: String = payload.get("store_id").and_then(Value::as_str).unwrap_or_default().to_owned();
            let t = for_printer(template(pool, &job.template_id).await?, Some(profile));
            let cells = cells(pool, &store_id, &job).await?;
            Ok(emit(&layout::layout(&t, &cells, profile.offset(), job.fill_last_row), profile))
        }
        "test" => {
            let id = payload.get("template_id").and_then(Value::as_str).unwrap_or("two_up_2x1");
            let t = for_printer(template(pool, id).await?, Some(profile));
            Ok(match profile.language {
                LabelLanguage::Tspl => tspl::alignment_test(&t, profile.offset(), profile.darkness, profile.speed),
                LabelLanguage::Zpl => zpl::alignment_test(&t, profile.offset(), profile.darkness),
            })
        }
        other => Err(AppError::internal(format!("not a label-printer job: {other}"))),
    }
}
