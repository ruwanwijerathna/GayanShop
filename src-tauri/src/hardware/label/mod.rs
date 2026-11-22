//! Barcode labels (doc §7.4, decision #30). Rust lays each product's sticker out in whole printer dots
//! (`layout`); the webview draws it as HTML/CSS, turns it into a 1-bit picture (`bitmap`) and sends it back;
//! the emitters place those pictures on the roll's columns in TSPL (`BITMAP`) or ZPL (`^GFA`). Templates and
//! the label printer profile (language, dpi, darkness, speed, calibration offsets) are device-local, because
//! each counter's printer and roll can differ.

pub mod barcode;
pub mod bitmap;
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
use bitmap::{LabelBitmap, StoredBitmap};
use layout::{LabelCell, LabelLanguage, LabelLayout, LabelTemplate};

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
    /// Print a sticker for every unit that arrives in stock (batch receive, quick purchase, late item).
    /// Profiles saved before this field existed keep it off.
    #[serde(default)]
    pub auto_print_on_receive: bool,
    /// The sticker roll chosen in Settings → Label printer: what "print when stock arrives" prints on.
    /// `None` (profiles saved before this field existed) falls back to the roll this counter used last.
    #[serde(default)]
    pub default_template_id: Option<String>,
}

/// The shop name printed at the top of every sticker.
pub const STICKER_SHOP_NAME: &str = "Sale Center";

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
        if let Some(id) = &self.default_template_id
            && (id.trim().is_empty() || id.len() > 64)
        {
            return bad("default template");
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

/// Built-in rolls this app used before the 50 x 25 mm and 30 x 16 mm stickers; stores that seeded them
/// are moved to the current ones the next time the list is read.
const RETIRED_IDS: [&str; 2] = ["two_up_2x1", "three_up_1x1"];

/// A stored list brought up to date: retired built-ins dropped, any current built-in that is missing added
/// (a calibrated copy keeps its roll geometry but always takes the built-in's element layout), custom templates
/// untouched.
pub fn reconcile(stored: Vec<LabelTemplate>, builtin: Vec<LabelTemplate>) -> Vec<LabelTemplate> {
    let mut kept: Vec<LabelTemplate> = stored.into_iter().filter(|t| !RETIRED_IDS.contains(&t.id.as_str())).collect();
    let mut out = Vec::with_capacity(kept.len() + builtin.len());
    for b in builtin {
        match kept.iter().position(|t| t.id == b.id) {
            Some(i) => {
                let mut t = kept.remove(i);
                t.elements = b.elements;
                out.push(t);
            }
            None => out.push(b),
        }
    }
    out.extend(kept);
    out
}

pub async fn templates(pool: &SqlitePool) -> AppResult<Vec<LabelTemplate>> {
    let stored: Option<Vec<LabelTemplate>> = settings::device_get(pool, TEMPLATES_KEY).await?;
    match stored {
        Some(t) if !t.is_empty() => Ok(reconcile(t, builtin_templates()?)),
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
    /// Calibrations saved before the top margin existed leave it at 0.
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub top_margin_mm: f64,
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
                || !in_range(c.top_margin_mm, 0.0, 10.0)
            {
                return Err(AppError::validation("LABEL_CALIBRATION_INVALID", "0–10 mm"));
            }
            slot.col_gap_mm = c.col_gap_mm;
            slot.row_gap_mm = c.row_gap_mm;
            slot.left_margin_mm = c.left_margin_mm;
            slot.top_margin_mm = c.top_margin_mm;
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

/// Which products, how many of each, on which roll: what the stickers are laid out from.
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

/// One product of a print job, with the sticker picture the webview drew for it.
#[derive(Debug, Clone, Deserialize, Type, PartialEq, Eq)]
pub struct LabelPrintItem {
    pub product_id: String,
    #[specta(type = specta_typescript::Number)]
    pub copies: i64,
    pub image: LabelBitmap,
}

/// A label job as `print_labels` receives it: the [`LabelJob`] plus one picture per product.
#[derive(Debug, Clone, Deserialize, Type, PartialEq, Eq)]
pub struct LabelPrintJob {
    pub template_id: String,
    pub items: Vec<LabelPrintItem>,
    #[serde(default)]
    pub fill_last_row: bool,
}

impl LabelPrintJob {
    pub fn job(&self) -> LabelJob {
        LabelJob {
            template_id: self.template_id.clone(),
            items: self
                .items
                .iter()
                .map(|i| LabelJobItem { product_id: i.product_id.clone(), copies: i.copies })
                .collect(),
            fill_last_row: self.fill_last_row,
        }
    }
}

/// The `print_jobs` payload of a label job: the pictures and how many of each, at the dpi they were drawn for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredJob {
    template_id: String,
    fill_last_row: bool,
    dpi: u16,
    items: Vec<StoredItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredItem {
    product_id: String,
    copies: u32,
    image: StoredBitmap,
}

/// The `print_jobs` payload of a "Print test label" job: one sample picture, printed across one row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSample {
    template_id: String,
    sample: bool,
    dpi: u16,
    image: StoredBitmap,
}

#[derive(FromRow)]
struct ProductRow {
    id: String,
    name: String,
    price: i64,
    barcode: Option<String>,
    attributes: String,
}

/// `size` first, then the other attributes in name order, joined with " - ".
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

/// One cell per job item, in job order.
pub async fn cells(pool: &SqlitePool, store_id: &str, job: &LabelJob) -> AppResult<Vec<LabelCell>> {
    let mut out = Vec::with_capacity(job.items.len());
    for item in &job.items {
        let p: ProductRow = sqlx::query_as(
            "SELECT id, name, price, barcode, attributes FROM products
             WHERE id = ?1 AND store_id = ?2 AND deleted_at IS NULL",
        )
        .bind(&item.product_id)
        .bind(store_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::validation("LABEL_PRODUCT_INVALID", item.product_id.clone()))?;
        out.push(LabelCell {
            product_id: p.id,
            store_name: STICKER_SHOP_NAME.to_owned(),
            name: p.name,
            attributes: attributes_line(&p.attributes),
            price: format_label_price(p.price),
            // Only the barcode is printed. The SKU is the cashier's typed shorthand and never goes on a
            // sticker; a product without a barcode lays out with a `NO_CODE` warning.
            codes: p.barcode.into_iter().collect(),
        });
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

fn sample_cell() -> LabelCell {
    LabelCell {
        product_id: "sample".into(),
        store_name: STICKER_SHOP_NAME.to_owned(),
        name: "Sample item".into(),
        attributes: String::new(),
        price: format_label_price(125_000),
        codes: vec!["21000010".into()],
    }
}

/// One sample sticker on the given roll, as this counter prints it (Settings → Label printer): the shop name,
/// a made-up price and an 8-digit in-store barcode.
pub async fn sample_preview(pool: &SqlitePool, template_id: &str) -> AppResult<LabelLayout> {
    let profile = load_profile(pool).await?;
    let t = for_printer(template(pool, template_id).await?, profile.as_ref());
    Ok(layout::layout(&t, &[sample_cell()]))
}

/// The stickers of a job, one per item, laid out for this counter's printer.
pub async fn preview(pool: &SqlitePool, store_id: &str, job: &LabelJob) -> AppResult<LabelLayout> {
    job.validate()?;
    let profile = load_profile(pool).await?;
    let t = for_printer(template(pool, &job.template_id).await?, profile.as_ref());
    Ok(layout::layout(&t, &cells(pool, store_id, job).await?))
}

/// Check a print job against the products and the sticker size, and turn it into its `print_jobs` payload.
/// Returns the payload, the number of labels and the layout (its warnings go into the audit row).
pub async fn prepare_print(
    pool: &SqlitePool,
    store_id: &str,
    job: &LabelPrintJob,
) -> AppResult<(Value, i64, LabelLayout)> {
    let plain = job.job();
    let labels = plain.validate()?;
    let laid = preview(pool, store_id, &plain).await?;
    let mut items = Vec::with_capacity(job.items.len());
    for i in &job.items {
        i.image.validate((laid.width, laid.height))?;
        let copies =
            u32::try_from(i.copies).map_err(|_| AppError::validation("LABEL_COPIES_INVALID", i.copies.to_string()))?;
        items.push(StoredItem { product_id: i.product_id.clone(), copies, image: StoredBitmap::from(&i.image) });
    }
    let stored =
        StoredJob { template_id: job.template_id.clone(), fill_last_row: job.fill_last_row, dpi: laid.dpi, items };
    Ok((serde_json::to_value(&stored)?, labels, laid))
}

/// Check a "Print test label" picture against the sample sticker and turn it into its `print_jobs` payload.
pub async fn prepare_sample(pool: &SqlitePool, template_id: &str, image: &LabelBitmap) -> AppResult<Value> {
    let laid = sample_preview(pool, template_id).await?;
    image.validate((laid.width, laid.height))?;
    let stored = StoredSample {
        template_id: template_id.to_owned(),
        sample: true,
        dpi: laid.dpi,
        image: StoredBitmap::from(image),
    };
    Ok(serde_json::to_value(&stored)?)
}

/// Printer bytes for the pictures laid out in `rows`.
pub fn emit(
    t: &LabelTemplate,
    profile: &LabelPrinterProfile,
    pictures: &[LabelBitmap],
    rows: &[Vec<usize>],
) -> Vec<u8> {
    match profile.language {
        LabelLanguage::Tspl => tspl::emit(t, profile.offset(), profile.darkness, profile.speed, pictures, rows),
        LabelLanguage::Zpl => zpl::emit(t, profile.offset(), profile.darkness, pictures, rows),
    }
}

/// The pictures of a stored job, checked against the roll they will print on. A job drawn for another dpi
/// (the printer was changed after it was queued) fails rather than printing at the wrong size.
fn pictures_for(t: &LabelTemplate, dpi: u16, stored: &[&StoredBitmap]) -> AppResult<Vec<LabelBitmap>> {
    if dpi != t.dpi {
        return Err(AppError::validation(
            "LABEL_DPI_CHANGED",
            format!("drawn for {dpi} dpi, the printer is {}", t.dpi),
        ));
    }
    let size = layout::sticker_size(t);
    stored
        .iter()
        .map(|s| {
            let b = s.decode()?;
            b.validate(size)?;
            Ok(b)
        })
        .collect()
}

/// The print worker's renderer for the label printer: `labels` jobs, and the `test` job (a sample sticker
/// row, or the alignment outline).
pub async fn render_job(
    pool: &SqlitePool,
    kind: &str,
    payload: &Value,
    profile: &LabelPrinterProfile,
) -> AppResult<Vec<u8>> {
    match kind {
        "labels" => {
            let job: StoredJob = serde_json::from_value(payload.clone())?;
            let t = for_printer(template(pool, &job.template_id).await?, Some(profile));
            let pictures = pictures_for(&t, job.dpi, &job.items.iter().map(|i| &i.image).collect::<Vec<_>>())?;
            let copies: Vec<(usize, u32)> = job.items.iter().enumerate().map(|(i, item)| (i, item.copies)).collect();
            Ok(emit(&t, profile, &pictures, &bitmap::rows(&copies, t.columns, job.fill_last_row)))
        }
        "test" if payload.get("sample").and_then(Value::as_bool) == Some(true) => {
            let s: StoredSample = serde_json::from_value(payload.clone())?;
            let t = for_printer(template(pool, &s.template_id).await?, Some(profile));
            let pictures = pictures_for(&t, s.dpi, &[&s.image])?;
            Ok(emit(&t, profile, &pictures, &bitmap::rows(&[(0, u32::from(t.columns))], t.columns, false)))
        }
        "test" => {
            let id = payload.get("template_id").and_then(Value::as_str).unwrap_or("two_up_50x25");
            let t = for_printer(template(pool, id).await?, Some(profile));
            Ok(match profile.language {
                LabelLanguage::Tspl => tspl::alignment_test(&t, profile.offset(), profile.darkness, profile.speed),
                LabelLanguage::Zpl => zpl::alignment_test(&t, profile.offset(), profile.darkness),
            })
        }
        other => Err(AppError::internal(format!("not a label-printer job: {other}"))),
    }
}
