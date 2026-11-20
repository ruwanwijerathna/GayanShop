//! TSPL (Xprinter, TSC and most generic label printers), doc §7.4: `SIZE` / `GAP` / `DIRECTION` / `REFERENCE`
//! once, then per row `CLS`, every column's `TEXT` and `BARCODE`, `PRINT 1`; identical consecutive rows
//! collapse into `PRINT n`. Everything is placed from the shared `LabelPreview`.

use super::barcode::Symbology;
use super::layout::{self, LabelPreview, LabelTemplate, PlacedLabel};

fn header(p: &LabelPreview, darkness: u8, speed: u8) -> String {
    format!(
        "SIZE {} mm,{} mm\r\nGAP {} mm,0 mm\r\nDIRECTION 1\r\nREFERENCE 0,0\r\nDENSITY {}\r\nSPEED {}\r\n",
        layout::mm_text(p.roll_width_mm),
        layout::mm_text(p.label_h_mm),
        layout::mm_text(p.row_gap_mm),
        darkness.min(15),
        speed.clamp(1, 6),
    )
}

/// The commands that draw one label (position already includes column and calibration offset).
fn label_body(l: &PlacedLabel, dpi: u16) -> String {
    let mut out = String::new();
    for t in &l.texts {
        for (i, line) in t.lines.iter().enumerate() {
            let x = layout::text_x_dots(t, line, dpi);
            let y = layout::text_y_dots(t, i, dpi);
            let font = t.font_size.clamp(1, 5);
            // Bold has no TSPL attribute: a bold line is drawn twice, one dot apart.
            out.push_str(&format!("TEXT {x},{y},\"{font}\",0,1,1,\"{line}\"\r\n"));
            if t.bold {
                out.push_str(&format!("TEXT {},{y},\"{font}\",0,1,1,\"{line}\"\r\n", x + 1));
            }
        }
    }
    if let Some(b) = &l.barcode {
        let (x, y, h) = layout::barcode_dots(b, dpi);
        let m = b.module_dots;
        match b.symbology {
            Symbology::Ean13 => out.push_str(&format!(
                "BARCODE {x},{y},\"EAN13\",{h},1,0,{m},{m},\"{}\"\r\n",
                b.data.get(..12).unwrap_or(&b.data)
            )),
            Symbology::Code128 => out.push_str(&format!("BARCODE {x},{y},\"128\",{h},1,0,{m},{m},\"{}\"\r\n", b.data)),
        }
    }
    out
}

/// TSPL bytes for a laid-out job.
pub fn emit(p: &LabelPreview, darkness: u8, speed: u8) -> Vec<u8> {
    let mut out = header(p, darkness, speed);
    let rows: Vec<String> = (0..p.rows)
        .map(|r| p.labels.iter().filter(|l| l.row == r).map(|l| label_body(l, p.dpi)).collect::<String>())
        .collect();
    let mut i = 0;
    while i < rows.len() {
        let mut n = 1;
        while i + n < rows.len() && rows[i + n] == rows[i] {
            n += 1;
        }
        out.push_str("CLS\r\n");
        out.push_str(&rows[i]);
        out.push_str(&format!("PRINT {n}\r\n"));
        i += n;
    }
    out.into_bytes()
}

/// Settings → Labels "Alignment test": the outline of every cell of one row, numbered, for calibration.
pub fn alignment_test(t: &LabelTemplate, offset_mm: (f64, f64), darkness: u8, speed: u8) -> Vec<u8> {
    let p = layout::layout(t, &[], offset_mm, false);
    let mut out = header(&p, darkness, speed);
    out.push_str("CLS\r\n");
    for (i, (x, y, x2, y2)) in layout::cell_boxes(t, offset_mm).into_iter().enumerate() {
        out.push_str(&format!("BOX {x},{y},{x2},{y2},2\r\n"));
        out.push_str(&format!("TEXT {},{},\"2\",0,1,1,\"{}\"\r\n", x + 8, y + 8, i + 1));
    }
    out.push_str("PRINT 1\r\n");
    out.into_bytes()
}
