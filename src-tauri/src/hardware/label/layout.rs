//! Sticker layout (doc §7.4, decision #30): template + products → each sticker's boxes in whole printer dots,
//! which the webview draws as HTML/CSS and turns into the 1-bit picture the printer receives. The preview on
//! screen is that same drawing, so what you see is what prints.
//!
//! Rust keeps the parts that must be exact: the roll geometry, the mm → dot conversion, the barcode encoding
//! and fit rules (bars at a whole number of dots per module), and the column positions the emitters place each
//! sticker picture at.
//!
//! This is the only module allowed float arithmetic (AGENT.md §2): millimetre geometry and dot conversion.
//! It never touches money; prices arrive already formatted.
#![allow(clippy::float_arithmetic)]

use serde::{Deserialize, Serialize};
use specta::Type;

use super::barcode::{self, Encoded, Symbology};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LabelLanguage {
    Tspl,
    Zpl,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    StoreName,
    ProductName,
    Attributes,
    Price,
    Barcode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct LabelElement {
    pub kind: ElementKind,
    #[specta(type = specta_typescript::Number)]
    pub x_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub y_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub w_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub h_mm: f64,
    /// Text size step 1–6 (see [`font_mm`]).
    pub font_size: u8,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub max_lines: Option<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbologyRule {
    Auto,
    Code128,
    Ean13,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct LabelTemplate {
    pub id: String,
    pub name: String,
    /// Labels per row on the roll.
    pub columns: u8,
    #[specta(type = specta_typescript::Number)]
    pub label_w_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub label_h_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub col_gap_mm: f64,
    /// The die-cut gap between rows.
    #[specta(type = specta_typescript::Number)]
    pub row_gap_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub left_margin_mm: f64,
    pub dpi: u16,
    pub language: LabelLanguage,
    pub symbology: SymbologyRule,
    pub elements: Vec<LabelElement>,
}

/// What one sticker shows (prepared from a product: name, attributes, formatted price, its barcode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelCell {
    pub product_id: String,
    pub store_name: String,
    pub name: String,
    pub attributes: String,
    pub price: String,
    /// The code to print: the product's barcode, or nothing (the SKU is never printed). Kept a list so a
    /// custom layout can still offer alternatives; the first that fits wins.
    pub codes: Vec<String>,
}

/// One text box on a sticker, in dots from the sticker's top-left corner.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct StickerText {
    pub kind: ElementKind,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// Font size in dots (one CSS pixel per dot when the sticker is drawn).
    pub font_px: u16,
    pub bold: bool,
    pub align: Align,
    /// More than 1: wrap, then cut with an ellipsis after this many lines.
    pub max_lines: u8,
    pub text: String,
}

/// The bars of one sticker, in dots from the sticker's top-left corner: every bar and space is a whole
/// number of dots wide, so the picture's bars are exactly what the scanner expects.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct StickerBarcode {
    pub symbology: Symbology,
    /// What the scanner reads (EAN-13 includes its check digit), also the human-readable line.
    pub data: String,
    pub x: u16,
    pub y: u16,
    /// Bars only; the human-readable line sits under them.
    pub bar_h: u16,
    /// Dots per module (2–4).
    pub module: u8,
    /// Alternating bar / space widths in modules, starting with a bar.
    pub widths: Vec<u8>,
    /// Font size of the human-readable line, in dots.
    pub hri_px: u16,
    /// Bars plus quiet zones fit the element box.
    pub fits: bool,
}

/// One product's sticker.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct Sticker {
    pub product_id: String,
    pub texts: Vec<StickerText>,
    pub barcode: Option<StickerBarcode>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct LabelWarning {
    /// `BARCODE_TOO_LONG`, `NO_CODE`.
    pub code: String,
    pub product_id: String,
}

/// The stickers of a job, one per product, as this counter's printer will print them.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct LabelLayout {
    pub template_id: String,
    pub dpi: u16,
    pub columns: u8,
    /// The sticker picture's size in dots. The width is rounded up to whole bytes (the printers take 8 dots
    /// per byte); the extra columns stay white.
    pub width: u16,
    pub height: u16,
    #[specta(type = specta_typescript::Number)]
    pub label_w_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub label_h_mm: f64,
    pub stickers: Vec<Sticker>,
    pub warnings: Vec<LabelWarning>,
}

/// Quiet zone on each side of the bars, in modules.
pub const QUIET_MODULES: u32 = 10;
/// The thinnest bar a CCD scanner reads reliably is 2 printer dots (0.25 mm at 203 dpi).
pub const MIN_MODULE_DOTS: u8 = 2;
const MAX_MODULE_DOTS: u8 = 4;
/// Space under the bars for the human-readable digits.
const HRI_MM: f64 = 2.0;
/// The largest sticker picture accepted, in dots per side (a 4.2″ head at 300 dpi is 1,260 dots).
pub const MAX_STICKER_DOTS: u16 = 2_400;

/// `dots = round(mm × dpi / 25.4)`: the only mm → dot conversion.
pub fn dots(mm: f64, dpi: u16) -> i64 {
    (mm * f64::from(dpi) / 25.4).round() as i64
}

pub fn mm_of_dots(dots: i64, dpi: u16) -> f64 {
    dots as f64 * 25.4 / f64::from(dpi)
}

/// mm with one decimal for printer commands (`SIZE 106.6 mm`).
pub fn mm_text(mm: f64) -> String {
    format!("{:.1}", (mm * 10.0).round() / 10.0)
}

/// The text height of a size step, in mm: the sizes the stickers printed at before they were pictures
/// (12, 20, 24, 32 and 48 dots at 203 dpi), so templates keep their look and scale with the head's dpi.
pub fn font_mm(step: u8) -> f64 {
    match step {
        0 | 1 => 1.5,
        2 => 2.5,
        3 => 3.0,
        4 => 4.0,
        _ => 6.0,
    }
}

/// A dot count that fits a picture side; out-of-range values clamp (a template is validated before use).
fn side(v: i64) -> u16 {
    u16::try_from(v.clamp(0, i64::from(MAX_STICKER_DOTS))).unwrap_or(MAX_STICKER_DOTS)
}

/// The sticker picture's size in dots: the label rounded to dots, the width up to whole bytes.
pub fn sticker_size(t: &LabelTemplate) -> (u16, u16) {
    let w = dots(t.label_w_mm, t.dpi);
    (side((w + 7) / 8 * 8), side(dots(t.label_h_mm, t.dpi)))
}

pub fn roll_width_mm(t: &LabelTemplate) -> f64 {
    let n = f64::from(t.columns.max(1));
    t.left_margin_mm * 2.0 + n * t.label_w_mm + (n - 1.0) * t.col_gap_mm
}

/// Where column `col`'s sticker picture goes on the roll, in dots: the margin, the columns and gaps before it,
/// and the calibration offset. A negative position (an offset pulling past the edge) is clamped to 0.
pub fn column_origin(t: &LabelTemplate, col: u8, offset_mm: (f64, f64)) -> (i64, i64) {
    let x = t.left_margin_mm + f64::from(col) * (t.label_w_mm + t.col_gap_mm) + offset_mm.0;
    (dots(x, t.dpi).max(0), dots(offset_mm.1, t.dpi).max(0))
}

/// Choose the code and module size for a barcode box: the first candidate that fits at ≥ 2 dots, the largest
/// module that still fits; if none fits, the first candidate at 2 dots, flagged.
pub fn place_barcode(candidates: &[String], rule: SymbologyRule, w_mm: f64, dpi: u16) -> Option<(Encoded, u8, bool)> {
    let preferred = match rule {
        SymbologyRule::Auto => None,
        SymbologyRule::Code128 => Some(Symbology::Code128),
        SymbologyRule::Ean13 => Some(Symbology::Ean13),
    };
    let width_dots = dots(w_mm, dpi);
    let mut first: Option<Encoded> = None;
    for code in candidates.iter().filter(|c| !c.trim().is_empty()) {
        let Some(enc) = barcode::encode(code.trim(), preferred).or_else(|| barcode::code128(code.trim())) else {
            continue;
        };
        let total = i64::from(enc.modules() + 2 * QUIET_MODULES);
        if let Some(m) = (MIN_MODULE_DOTS..=MAX_MODULE_DOTS).rev().find(|m| total * i64::from(*m) <= width_dots) {
            return Some((enc, m, true));
        }
        if first.is_none() {
            first = Some(enc);
        }
    }
    first.map(|enc| (enc, MIN_MODULE_DOTS, false))
}

fn place_sticker(t: &LabelTemplate, cell: &LabelCell, warnings: &mut Vec<LabelWarning>) -> Sticker {
    let mut warn = |code: &str| {
        if !warnings.iter().any(|w| w.code == code && w.product_id == cell.product_id) {
            warnings.push(LabelWarning { code: code.to_owned(), product_id: cell.product_id.clone() });
        }
    };
    let d = |mm: f64| side(dots(mm, t.dpi));
    let mut texts = Vec::new();
    let mut code = None;
    for e in &t.elements {
        let (raw, max_lines) = match e.kind {
            ElementKind::StoreName => (cell.store_name.as_str(), 1),
            ElementKind::ProductName => (cell.name.as_str(), e.max_lines.unwrap_or(2).clamp(1, 3)),
            ElementKind::Attributes => (cell.attributes.as_str(), 1),
            ElementKind::Price => (cell.price.as_str(), 1),
            ElementKind::Barcode => {
                match place_barcode(&cell.codes, t.symbology, e.w_mm, t.dpi) {
                    Some((enc, module, fits)) => {
                        if !fits {
                            warn("BARCODE_TOO_LONG");
                        }
                        let bars = i64::from(enc.modules()) * i64::from(module);
                        let box_x = dots(e.x_mm, t.dpi);
                        let x = box_x + ((dots(e.w_mm, t.dpi) - bars) / 2).max(0);
                        let bar_h = (dots(e.h_mm, t.dpi) - dots(HRI_MM, t.dpi)).max(dots(1.0, t.dpi));
                        code = Some(StickerBarcode {
                            symbology: enc.symbology,
                            data: enc.data.clone(),
                            x: side(x),
                            y: d(e.y_mm),
                            bar_h: side(bar_h),
                            module,
                            widths: enc.widths,
                            hri_px: d(HRI_MM),
                            fits,
                        });
                    }
                    None => warn("NO_CODE"),
                }
                continue;
            }
        };
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        texts.push(StickerText {
            kind: e.kind,
            x: d(e.x_mm),
            y: d(e.y_mm),
            w: d(e.w_mm),
            h: d(e.h_mm),
            font_px: d(font_mm(e.font_size)),
            bold: e.bold,
            align: e.align,
            max_lines,
            text: text.to_owned(),
        });
    }
    Sticker { product_id: cell.product_id.clone(), texts, barcode: code }
}

/// Lay out one sticker per cell (the caller passes each product once; copies are counted when printing).
pub fn layout(t: &LabelTemplate, cells: &[LabelCell]) -> LabelLayout {
    let (width, height) = sticker_size(t);
    let mut warnings = Vec::new();
    let stickers = cells.iter().map(|c| place_sticker(t, c, &mut warnings)).collect();
    LabelLayout {
        template_id: t.id.clone(),
        dpi: t.dpi,
        columns: t.columns,
        width,
        height,
        label_w_mm: t.label_w_mm,
        label_h_mm: t.label_h_mm,
        stickers,
        warnings,
    }
}

/// The rectangle of each cell of one row (for the alignment test), in dots: x, y, x2, y2.
pub fn cell_boxes(t: &LabelTemplate, offset_mm: (f64, f64)) -> Vec<(i64, i64, i64, i64)> {
    (0..t.columns)
        .map(|col| {
            let (x, y) = column_origin(t, col, offset_mm);
            (x, y, x + dots(t.label_w_mm, t.dpi), y + dots(t.label_h_mm, t.dpi))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_round_at_203_and_300_dpi() {
        assert_eq!(dots(25.4, 203), 203);
        assert_eq!(dots(50.8, 203), 406);
        assert_eq!(dots(2.0, 203), 16);
        assert_eq!(dots(25.4, 300), 300);
        assert_eq!(mm_text(106.6), "106.6");
    }

    #[test]
    fn text_sizes_keep_their_millimetres_on_either_head() {
        assert_eq!(dots(font_mm(4), 203), 32);
        assert_eq!(dots(font_mm(6), 203), 48);
        assert_eq!(dots(font_mm(6), 300), 71);
        assert_eq!(font_mm(9), font_mm(5));
    }
}
