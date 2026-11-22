//! ZPL (Zebra and compatibles), doc §7.4: per row `^XA ^PW ^LL ^LH0,0`, one `^GFA` graphic field per column
//! (the sticker picture the webview drew), `^PQ n` for identical consecutive rows, `^XZ`.

use super::bitmap::{self, LabelBitmap};
use super::layout::{self, LabelTemplate};

/// `~SD` sets absolute darkness 0–30 (twice the TSPL `DENSITY` 0–15 the profile stores).
fn row_open(t: &LabelTemplate, darkness: u8) -> String {
    format!(
        "~SD{:02}^XA^PW{}^LL{}^LH0,0\r\n",
        darkness.min(15) * 2,
        layout::dots(layout::roll_width_mm(t), t.dpi),
        layout::dots(t.label_h_mm, t.dpi),
    )
}

/// `^GFA,total,total,row_bytes,HEX`: ZPL prints a 1 bit black, the picture's own packing.
fn graphic_field(x: i64, y: i64, b: &LabelBitmap) -> String {
    let total = b.data.len();
    format!("^FO{x},{y}^GFA,{total},{total},{},{}^FS\r\n", b.row_bytes(), hex::encode_upper(&b.data))
}

/// ZPL bytes for a job (each printed row is one ZPL label as wide as the roll).
pub fn emit(
    t: &LabelTemplate,
    offset_mm: (f64, f64),
    darkness: u8,
    pictures: &[LabelBitmap],
    rows: &[Vec<usize>],
) -> Vec<u8> {
    let mut out = String::new();
    for (row, n) in bitmap::runs(rows) {
        out.push_str(&row_open(t, darkness));
        for (col, &i) in row.iter().enumerate() {
            if let Some(b) = pictures.get(i) {
                let (x, y) = layout::column_origin(t, u8::try_from(col).unwrap_or(u8::MAX), offset_mm);
                out.push_str(&graphic_field(x, y, b));
            }
        }
        out.push_str(&format!("^PQ{n}\r\n^XZ\r\n"));
    }
    out.into_bytes()
}

/// The alignment test: every cell's outline (`^GB`) and its number.
pub fn alignment_test(t: &LabelTemplate, offset_mm: (f64, f64), darkness: u8) -> Vec<u8> {
    let mut out = row_open(t, darkness);
    for (i, (x, y, x2, y2)) in layout::cell_boxes(t, offset_mm).into_iter().enumerate() {
        out.push_str(&format!("^FO{x},{y}^GB{},{},2^FS\r\n", x2 - x, y2 - y));
        out.push_str(&format!("^FO{},{}^A0N,20,12^FD{}^FS\r\n", x + 8, y + 8, i + 1));
    }
    out.push_str("^PQ1\r\n^XZ\r\n");
    out.into_bytes()
}
