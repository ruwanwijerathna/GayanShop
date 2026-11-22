//! TSPL (Xprinter, TSC and most generic label printers), doc §7.4: `SIZE` / `GAP` / `DIRECTION` / `REFERENCE`
//! once, then per row `CLS`, one `BITMAP` per column (the sticker picture the webview drew), `PRINT n` for
//! identical consecutive rows. Nothing is drawn with the printer's own fonts.

use super::bitmap::{self, LabelBitmap};
use super::layout::{self, LabelTemplate};

fn header(t: &LabelTemplate, darkness: u8, speed: u8) -> String {
    format!(
        "SIZE {} mm,{} mm\r\nGAP {} mm,0 mm\r\nDIRECTION 1\r\nREFERENCE 0,0\r\nDENSITY {}\r\nSPEED {}\r\n",
        layout::mm_text(layout::roll_width_mm(t)),
        layout::mm_text(t.label_h_mm),
        layout::mm_text(t.row_gap_mm),
        darkness.min(15),
        speed.clamp(1, 6),
    )
}

/// `BITMAP x,y,width_bytes,height,0,<bytes>`: mode 0 overwrites. TSPL prints a **0** bit black, the opposite
/// of the picture's packing (1 = black), so every byte is inverted on the way out.
fn bitmap_command(out: &mut Vec<u8>, x: i64, y: i64, b: &LabelBitmap) {
    out.extend(format!("BITMAP {x},{y},{},{},0,", b.row_bytes(), b.height).into_bytes());
    out.extend(b.data.iter().map(|byte| !byte));
    out.extend(b"\r\n");
}

/// TSPL bytes for a job: `rows` from [`bitmap::rows`], indexing `pictures`.
pub fn emit(
    t: &LabelTemplate,
    offset_mm: (f64, f64),
    darkness: u8,
    speed: u8,
    pictures: &[LabelBitmap],
    rows: &[Vec<usize>],
) -> Vec<u8> {
    let mut out = header(t, darkness, speed).into_bytes();
    for (row, n) in bitmap::runs(rows) {
        out.extend(b"CLS\r\n");
        for (col, &i) in row.iter().enumerate() {
            if let Some(b) = pictures.get(i) {
                let (x, y) = layout::column_origin(t, u8::try_from(col).unwrap_or(u8::MAX), offset_mm);
                bitmap_command(&mut out, x, y, b);
            }
        }
        out.extend(format!("PRINT {n}\r\n").into_bytes());
    }
    out
}

/// Settings → Labels "Alignment test": the outline of every cell of one row, numbered, for calibration.
pub fn alignment_test(t: &LabelTemplate, offset_mm: (f64, f64), darkness: u8, speed: u8) -> Vec<u8> {
    let mut out = header(t, darkness, speed);
    out.push_str("CLS\r\n");
    for (i, (x, y, x2, y2)) in layout::cell_boxes(t, offset_mm).into_iter().enumerate() {
        out.push_str(&format!("BOX {x},{y},{x2},{y2},2\r\n"));
        out.push_str(&format!("TEXT {},{},\"2\",0,1,1,\"{}\"\r\n", x + 8, y + 8, i + 1));
    }
    out.push_str("PRINT 1\r\n");
    out.into_bytes()
}
