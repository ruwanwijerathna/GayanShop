//! ZPL (Zebra and compatibles), doc §7.4: per row `^XA ^PW ^LL ^LH0,0`, the fields per column (`^FO`, `^A0`,
//! `^BY` + `^BC` / `^BE`), `^PQ n` for identical consecutive rows, `^XZ`.

use super::barcode::Symbology;
use super::layout::{self, LabelPreview, LabelTemplate, PlacedLabel};

/// `~SD` sets absolute darkness 0–30 (twice the TSPL `DENSITY` 0–15 the profile stores).
fn row_open(p: &LabelPreview, darkness: u8) -> String {
    format!(
        "~SD{:02}^XA^PW{}^LL{}^LH0,0\r\n",
        darkness.min(15) * 2,
        layout::dots(p.roll_width_mm, p.dpi),
        layout::dots(p.label_h_mm, p.dpi),
    )
}

fn label_body(l: &PlacedLabel, dpi: u16, label_w_mm: f64) -> String {
    let mut out = String::new();
    for t in &l.texts {
        let (cw, ch) = layout::font_dots(t.font_size, dpi);
        let w = if t.bold { cw + cw / 4 } else { cw };
        let box_w = layout::dots(t.w_mm, dpi);
        let box_x = layout::dots(t.x_mm, dpi);
        let justify = match t.align {
            layout::Align::Left => 'L',
            layout::Align::Center => 'C',
            layout::Align::Right => 'R',
        };
        for (i, line) in t.lines.iter().enumerate() {
            let y = layout::text_y_dots(t, i, dpi);
            // `^A0` is proportional, so its width is the printer's to measure: every line is aligned inside its
            // box by a one-line `^FB` rather than by an estimate.
            out.push_str(&format!("^FO{box_x},{y}^A0N,{ch},{w}^FB{box_w},1,0,{justify}^FD{line}^FS\r\n"));
        }
    }
    if let Some(b) = &l.barcode {
        let (x, y, h) = layout::barcode_dots(b, dpi);
        let m = b.module_dots;
        match b.symbology {
            Symbology::Ean13 => {
                out.push_str(&format!("^FO{x},{y}^BY{m}^BEN,{h},Y,N^FD{}^FS\r\n", b.data.get(..12).unwrap_or(&b.data)));
            }
            Symbology::Code128 => {
                out.push_str(&format!("^FO{x},{y}^BY{m}^BCN,{h},N,N,N,A^FD{}^FS\r\n", b.data));
                let (cw, ch) = (layout::dots(1.8, dpi), layout::dots(2.5, dpi));
                let text_y = y + h + layout::dots(0.5, dpi);
                let col_x = layout::dots(l.x_mm, dpi);
                let label_w = layout::dots(label_w_mm, dpi);
                out.push_str(&format!("^FO{col_x},{text_y}^A0N,{ch},{cw}^FB{label_w},1,0,C^FD{}^FS\r\n", b.data));
            }
        }
    }
    out
}

/// ZPL bytes for a laid-out job (each printed row is one ZPL label as wide as the roll).
pub fn emit(p: &LabelPreview, darkness: u8) -> Vec<u8> {
    let rows: Vec<String> = (0..p.rows)
        .map(|r| p.labels.iter().filter(|l| l.row == r).map(|l| label_body(l, p.dpi, p.label_w_mm)).collect::<String>())
        .collect();
    let mut out = String::new();
    let mut i = 0;
    while i < rows.len() {
        let mut n = 1;
        while i + n < rows.len() && rows[i + n] == rows[i] {
            n += 1;
        }
        out.push_str(&row_open(p, darkness));
        out.push_str(&rows[i]);
        out.push_str(&format!("^PQ{n}\r\n^XZ\r\n"));
        i += n;
    }
    out.into_bytes()
}

/// The alignment test: every cell's outline (`^GB`) and its number.
pub fn alignment_test(t: &LabelTemplate, offset_mm: (f64, f64), darkness: u8) -> Vec<u8> {
    let p = layout::layout(t, &[], offset_mm, false);
    let mut out = row_open(&p, darkness);
    for (i, (x, y, x2, y2)) in layout::cell_boxes(t, offset_mm).into_iter().enumerate() {
        out.push_str(&format!("^FO{x},{y}^GB{},{},2^FS\r\n", x2 - x, y2 - y));
        out.push_str(&format!("^FO{},{}^A0N,20,12^FD{}^FS\r\n", x + 8, y + 8, i + 1));
    }
    out.push_str("^PQ1\r\n^XZ\r\n");
    out.into_bytes()
}
