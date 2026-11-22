//! The one layout engine (doc §7.4): template + cells → a positioned `LabelPreview` that the UI draws as SVG
//! and the TSPL / ZPL emitters print, so the preview is what comes out of the printer.
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
    /// Printer built-in font step 1–4.
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

/// What one label shows (prepared from a product: Latin name, attributes, formatted price, codes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelCell {
    pub product_id: String,
    pub store_name: String,
    pub name: String,
    pub attributes: String,
    pub price: String,
    /// Candidate codes in order of preference: the barcode, then the short SKU.
    pub codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct PlacedText {
    pub kind: ElementKind,
    /// Absolute position on the roll, mm (calibration offset included).
    #[specta(type = specta_typescript::Number)]
    pub x_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub y_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub w_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub h_mm: f64,
    pub font_size: u8,
    pub bold: bool,
    pub align: Align,
    /// One entry per printed line, already fitted (wrapped, truncated, unprintable characters as `?`).
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct PlacedBarcode {
    pub symbology: Symbology,
    pub data: String,
    #[specta(type = specta_typescript::Number)]
    pub x_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub y_mm: f64,
    /// Bars only (the human-readable line sits under them, inside the element box).
    #[specta(type = specta_typescript::Number)]
    pub bar_h_mm: f64,
    pub module_dots: u8,
    #[specta(type = specta_typescript::Number)]
    pub module_mm: f64,
    /// Alternating bar / space widths in modules, starting with a bar (for the SVG preview).
    pub widths: Vec<u8>,
    /// Bars plus quiet zones fit the element box.
    pub fits: bool,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct PlacedLabel {
    pub row: u32,
    pub col: u8,
    pub product_id: String,
    /// Top-left of the label on the roll, mm.
    #[specta(type = specta_typescript::Number)]
    pub x_mm: f64,
    pub texts: Vec<PlacedText>,
    pub barcode: Option<PlacedBarcode>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct LabelWarning {
    /// `BARCODE_TOO_LONG`, `NO_CODE`, `TEXT_NOT_PRINTABLE`, `TEXT_TRUNCATED`.
    pub code: String,
    pub product_id: String,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
pub struct LabelPreview {
    pub template_id: String,
    pub dpi: u16,
    pub columns: u8,
    pub rows: u32,
    #[specta(type = specta_typescript::Number)]
    pub roll_width_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub label_w_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub label_h_mm: f64,
    #[specta(type = specta_typescript::Number)]
    pub row_gap_mm: f64,
    pub labels: Vec<PlacedLabel>,
    pub warnings: Vec<LabelWarning>,
}

/// Quiet zone on each side of the bars, in modules.
pub const QUIET_MODULES: u32 = 10;
/// The thinnest bar a CCD scanner reads reliably is 2 printer dots (0.25 mm at 203 dpi).
pub const MIN_MODULE_DOTS: u8 = 2;
const MAX_MODULE_DOTS: u8 = 4;
/// Space under the bars for the human-readable digits.
const HRI_MM: f64 = 2.0;

/// `dots = round(mm × dpi / 25.4)` — the only mm → dot conversion (done at emit time).
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

/// The built-in font cell for a font step, in dots: the TSPL fixed-pitch fonts "1"–"5" (at 203 dpi 8×12,
/// 12×20, 16×24, 24×32, 32×48). Font "5" is the largest, so every step above 5 prints, and measures, as 5.
pub fn font_dots(step: u8, dpi: u16) -> (i64, i64) {
    // The printers' built-in bitmap fonts are drawn in dots, so the same font is larger in dots on a 300 dpi
    // head (TSPL font table); either way each step measures about the same on the label.
    if dpi >= 300 {
        match step {
            0 | 1 => (12, 20),
            2 => (16, 28),
            3 => (24, 32),
            4 => (32, 48),
            _ => (48, 72),
        }
    } else {
        match step {
            0 | 1 => (8, 12),
            2 => (12, 20),
            3 => (16, 24),
            4 => (24, 32),
            _ => (32, 48),
        }
    }
}

/// The TSPL font number a step prints with (`TEXT x,y,"n",...`).
pub fn tspl_font(step: u8) -> u8 {
    step.clamp(1, 5)
}

/// How wide a line prints in a built-in font, in dots: fixed pitch, and a bold line is the same line drawn
/// again one dot to the right, so it is one dot wider (not a wider font).
pub fn text_width_dots(chars: usize, step: u8, bold: bool, dpi: u16) -> i64 {
    let (cw, _) = font_dots(step, dpi);
    i64::try_from(chars).unwrap_or(0) * cw + i64::from(bold)
}

pub fn roll_width_mm(t: &LabelTemplate) -> f64 {
    let n = f64::from(t.columns.max(1));
    t.left_margin_mm * 2.0 + n * t.label_w_mm + (n - 1.0) * t.col_gap_mm
}

/// Printable ASCII only; everything else (Sinhala, emoji, `·`) becomes `?`. A double quote becomes `'`
/// (it ends a TSPL string) and `^` / `~` become `?` (ZPL command prefixes).
fn printable(s: &str) -> (String, bool) {
    let mut lost = false;
    let out = s
        .chars()
        .map(|c| match c {
            '"' => '\'',
            '^' | '~' => '?',
            ' '..='}' => c,
            _ => {
                lost = true;
                '?'
            }
        })
        .collect();
    (out, lost)
}

fn truncate(s: &str, max: usize) -> (String, bool) {
    if s.chars().count() <= max {
        return (s.to_owned(), false);
    }
    if max <= 3 {
        return (s.chars().take(max).collect(), true);
    }
    let kept: String = s.chars().take(max - 3).collect();
    (format!("{}...", kept.trim_end()), true)
}

/// Word-wrap into at most `lines` lines of `max` characters, with "..." when it doesn't all fit.
pub fn fit_lines(s: &str, max: usize, lines: usize) -> (Vec<String>, bool) {
    let max = max.max(1);
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let next_len = if cur.is_empty() { w.chars().count() } else { cur.chars().count() + 1 + w.chars().count() };
        if next_len <= max {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
            i += 1;
        } else if cur.is_empty() {
            // A single word longer than a line: cut it.
            cur = w.chars().take(max).collect();
            i += 1;
            out.push(std::mem::take(&mut cur));
        } else {
            out.push(std::mem::take(&mut cur));
        }
        if out.len() == lines {
            break;
        }
    }
    if !cur.is_empty() && out.len() < lines {
        out.push(cur);
    }
    let all_used = i >= words.len();
    if !all_used && let Some(last) = out.last_mut() {
        // Room for "..." on the last line, cutting the line if needed.
        let keep = max.saturating_sub(3).min(last.chars().count());
        *last = format!("{}...", last.chars().take(keep).collect::<String>().trim_end());
    }
    (out, !all_used)
}

/// The most characters that fit a box, measured exactly as `text_x_dots` centres them.
fn chars_per_line(w_mm: f64, step: u8, bold: bool, dpi: u16) -> usize {
    let (cw, _) = font_dots(step, dpi);
    let room = dots(w_mm, dpi) - i64::from(bold);
    usize::try_from((room / cw).max(1)).unwrap_or(1)
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

fn place_label(
    t: &LabelTemplate,
    cell: &LabelCell,
    row: u32,
    col: u8,
    offset: (f64, f64),
    warnings: &mut Vec<LabelWarning>,
) -> PlacedLabel {
    let x0 = t.left_margin_mm + f64::from(col) * (t.label_w_mm + t.col_gap_mm) + offset.0;
    let y0 = offset.1;
    let mut warn = |code: &str| {
        if !warnings.iter().any(|w| w.code == code && w.product_id == cell.product_id) {
            warnings.push(LabelWarning { code: code.to_owned(), product_id: cell.product_id.clone() });
        }
    };
    let mut texts = Vec::new();
    let mut code = None;
    for e in &t.elements {
        let (raw, max_lines) = match e.kind {
            ElementKind::StoreName => (cell.store_name.as_str(), 1),
            ElementKind::ProductName => (cell.name.as_str(), usize::from(e.max_lines.unwrap_or(2).clamp(1, 2))),
            ElementKind::Attributes => (cell.attributes.as_str(), 1),
            ElementKind::Price => (cell.price.as_str(), 1),
            ElementKind::Barcode => {
                let w_mm = e.w_mm;
                match place_barcode(&cell.codes, t.symbology, w_mm, t.dpi) {
                    Some((enc, module_dots, fits)) => {
                        if !fits {
                            warn("BARCODE_TOO_LONG");
                        }
                        let module_mm = mm_of_dots(i64::from(module_dots), t.dpi);
                        let bars_mm = f64::from(enc.modules()) * module_mm;
                        let x = e.x_mm + ((w_mm - bars_mm) / 2.0).max(0.0);
                        code = Some(PlacedBarcode {
                            symbology: enc.symbology,
                            data: enc.data.clone(),
                            x_mm: x0 + x,
                            y_mm: y0 + e.y_mm,
                            bar_h_mm: (e.h_mm - HRI_MM).max(1.0),
                            module_dots,
                            module_mm,
                            widths: enc.widths,
                            fits,
                        });
                    }
                    None => warn("NO_CODE"),
                }
                continue;
            }
        };
        if raw.trim().is_empty() {
            continue;
        }
        let (clean, lost) = printable(raw);
        if lost {
            warn("TEXT_NOT_PRINTABLE");
        }
        let per_line = chars_per_line(e.w_mm, e.font_size, e.bold, t.dpi);
        let (lines, cut) = if max_lines > 1 {
            fit_lines(&clean, per_line, max_lines)
        } else {
            let (l, cut) = truncate(clean.trim(), per_line);
            (vec![l], cut)
        };
        if cut && e.kind == ElementKind::Price {
            warn("TEXT_TRUNCATED");
        }
        texts.push(PlacedText {
            kind: e.kind,
            x_mm: x0 + e.x_mm,
            y_mm: y0 + e.y_mm,
            w_mm: e.w_mm,
            h_mm: e.h_mm,
            font_size: e.font_size,
            bold: e.bold,
            align: e.align,
            lines,
        });
    }
    PlacedLabel { row, col, product_id: cell.product_id.clone(), x_mm: x0, texts, barcode: code }
}

/// Lay out a job: cells fill rows left to right; a partial last row is left blank (or filled with copies of the
/// last cell when `fill_last_row`).
pub fn layout(t: &LabelTemplate, cells: &[LabelCell], offset_mm: (f64, f64), fill_last_row: bool) -> LabelPreview {
    let cols = usize::from(t.columns.max(1));
    let mut cells: Vec<&LabelCell> = cells.iter().collect();
    if fill_last_row && !cells.is_empty() {
        while !cells.len().is_multiple_of(cols) {
            let last = cells[cells.len() - 1];
            cells.push(last);
        }
    }
    let mut warnings = Vec::new();
    let labels = cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let row = u32::try_from(i / cols).unwrap_or(u32::MAX);
            let col = u8::try_from(i % cols).unwrap_or(0);
            place_label(t, cell, row, col, offset_mm, &mut warnings)
        })
        .collect::<Vec<_>>();
    LabelPreview {
        template_id: t.id.clone(),
        dpi: t.dpi,
        columns: t.columns,
        rows: u32::try_from(cells.len().div_ceil(cols)).unwrap_or(u32::MAX),
        roll_width_mm: roll_width_mm(t),
        label_w_mm: t.label_w_mm,
        label_h_mm: t.label_h_mm,
        row_gap_mm: t.row_gap_mm,
        labels,
        warnings,
    }
}

/// The x position (dots) of a text line inside its box, by alignment and its printed width (TSPL; ZPL lets
/// the printer align inside a `^FB` box).
pub fn text_x_dots(t: &PlacedText, line: &str, dpi: u16) -> i64 {
    let text = text_width_dots(line.chars().count(), t.font_size, t.bold, dpi);
    let x = dots(t.x_mm, dpi);
    let w = dots(t.w_mm, dpi);
    match t.align {
        Align::Left => x,
        Align::Center => x + ((w - text) / 2).max(0),
        Align::Right => x + (w - text).max(0),
    }
}

/// The y position (dots) of line `i` of a text box.
pub fn text_y_dots(t: &PlacedText, i: usize, dpi: u16) -> i64 {
    let (_, ch) = font_dots(t.font_size, dpi);
    dots(t.y_mm, dpi) + i64::try_from(i).unwrap_or(0) * (ch + 2)
}

/// Bar height and the barcode's x / y, in dots.
pub fn barcode_dots(b: &PlacedBarcode, dpi: u16) -> (i64, i64, i64) {
    (dots(b.x_mm, dpi), dots(b.y_mm, dpi), dots(b.bar_h_mm, dpi))
}

/// The rectangle of each cell of one row (for the alignment test), in dots: x, y, x2, y2.
pub fn cell_boxes(t: &LabelTemplate, offset_mm: (f64, f64)) -> Vec<(i64, i64, i64, i64)> {
    (0..t.columns)
        .map(|col| {
            let x = t.left_margin_mm + f64::from(col) * (t.label_w_mm + t.col_gap_mm) + offset_mm.0;
            let y = offset_mm.1;
            (dots(x, t.dpi), dots(y, t.dpi), dots(x + t.label_w_mm, t.dpi), dots(y + t.label_h_mm, t.dpi))
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
    fn product_names_wrap_to_two_lines_then_ellipsis() {
        let (l, cut) = fit_lines("Ladies Sandal Leather Comfort Fit Extra", 22, 2);
        assert_eq!(l, vec!["Ladies Sandal Leather", "Comfort Fit Extra"]);
        assert!(!cut);
        let (l, cut) = fit_lines("Ladies Sandal Leather Comfort Fit Extra Wide Black", 22, 2);
        assert_eq!(l.len(), 2);
        assert!(cut && l[1].ends_with("..."), "{l:?}");
        assert!(l.iter().all(|s| s.chars().count() <= 22));
    }

    #[test]
    fn unprintable_characters_become_question_marks() {
        assert_eq!(printable("Size 38 · Black"), ("Size 38 ? Black".to_owned(), true));
        assert_eq!(printable("Shoe"), ("Shoe".to_owned(), false));
    }
}
