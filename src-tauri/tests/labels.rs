//! Block 2.2 / decision #30 — the label engine (doc §7.4): sticker layout in whole printer dots on the 2-up
//! 50 × 25 mm and 3-up 30 × 16 mm templates, barcode-fit rules, the sticker pictures the webview draws checked
//! and stored, the exact TSPL `BITMAP` / ZPL `^GFA` bytes, calibration, and a label job through the label
//! printer's own print worker. The alignment outlines keep golden snapshots
//! (`UPDATE_FIXTURES=1 cargo test --test labels`, then review the bytes).

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Till, till};
use gayanshop_lib::db::repo::print_jobs::{self, PrintKind};
use gayanshop_lib::db::settings;
use gayanshop_lib::hardware::label::barcode::Symbology;
use gayanshop_lib::hardware::label::bitmap::{self, LabelBitmap};
use gayanshop_lib::hardware::label::layout::{self, LabelCell, LabelLanguage, LabelLayout, LabelTemplate};
use gayanshop_lib::hardware::label::{
    self, LABEL_PRINTER, LabelJob, LabelJobItem, LabelPrintItem, LabelPrintJob, LabelPrinterProfile,
    TemplateCalibration, attributes_line,
};
use gayanshop_lib::hardware::printer::worker::{PrintHub, WorkerConfig};
use gayanshop_lib::hardware::printer::{PrintFault, PrinterStatus, PrinterTransport, TransportConfig, profile_key};

fn builtin(id: &str) -> LabelTemplate {
    label::builtin_templates().unwrap().into_iter().find(|t| t.id == id).unwrap()
}

fn cell(id: &str, name: &str, codes: &[&str]) -> LabelCell {
    LabelCell {
        product_id: id.into(),
        store_name: "Sale Center".into(),
        name: name.into(),
        attributes: "Size 38 - Black".into(),
        price: "Rs. 1,250".into(),
        codes: codes.iter().map(|c| (*c).to_owned()).collect(),
    }
}

fn codes(p: &LabelLayout) -> Vec<String> {
    p.warnings.iter().map(|w| w.code.clone()).collect()
}

/// A sticker picture with a recognisable pattern (`seed` tells two pictures apart).
fn picture(width: u16, height: u16, seed: u8) -> LabelBitmap {
    let n = usize::from(width / 8) * usize::from(height);
    LabelBitmap { width, height, data: (0..n).map(|i| u8::try_from(i % 251).unwrap().wrapping_add(seed)).collect() }
}

/// Compare with (or, under UPDATE_FIXTURES, write) a golden file.
fn golden(name: &str, bytes: &[u8]) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/labels").join(name);
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(committed, text, "{name} changed: rerun with UPDATE_FIXTURES=1 and review the printer bytes");
}

fn tspl_profile() -> LabelPrinterProfile {
    LabelPrinterProfile {
        transport: TransportConfig::Network { host: "fake".into(), port: 9100 },
        language: LabelLanguage::Tspl,
        dpi: 203,
        darkness: 8,
        speed: 4,
        offset_x_mm: 0.0,
        offset_y_mm: 0.0,
        auto_print_on_receive: false,
        default_template_id: None,
    }
}

#[test]
fn the_built_in_templates_are_the_owner_confirmed_rolls() {
    let two = builtin("two_up_50x25");
    let three = builtin("three_up_30x16");
    assert_eq!((two.columns, two.label_w_mm, two.label_h_mm, two.dpi), (2, 50.0, 25.0, 203));
    assert_eq!((three.columns, three.label_w_mm, three.label_h_mm, three.dpi), (3, 30.0, 16.0, 203));
    assert_eq!((two.row_gap_mm, two.col_gap_mm, two.left_margin_mm), (3.0, 2.0, 1.5));
    assert_eq!((three.row_gap_mm, three.col_gap_mm, three.left_margin_mm), (3.0, 2.0, 1.5));
    // Roll widths (a margin each side): 105.0 mm and 97.0 mm.
    assert_eq!(layout::mm_text(layout::roll_width_mm(&two)), "105.0");
    assert_eq!(layout::mm_text(layout::roll_width_mm(&three)), "97.0");
}

#[test]
fn a_sticker_picture_is_the_label_in_dots_rounded_up_to_whole_bytes() {
    assert_eq!(layout::sticker_size(&builtin("two_up_50x25")), (400, 200));
    assert_eq!(layout::sticker_size(&builtin("three_up_30x16")), (240, 128));
    // 50 mm at 300 dpi is 591 dots: 592, the 8-dot byte boundary; the extra column stays white.
    let t300 = LabelTemplate { dpi: 300, ..builtin("two_up_50x25") };
    assert_eq!(layout::sticker_size(&t300), (592, 295));
    let p = layout::layout(&t300, &[cell("p1", "Sandal", &["21000423"])]);
    assert_eq!((p.width, p.height, p.dpi), (592, 295, 300));
    let price = p.stickers[0].texts.iter().find(|x| x.kind == layout::ElementKind::Price).unwrap();
    assert_eq!(price.font_px, 71, "the same 6 mm price, in 300 dpi dots");
}

#[test]
fn ean13_fits_a_2x1_label_at_a_three_dot_module() {
    let p = layout::layout(&builtin("two_up_50x25"), &[cell("p1", "Ladies Sandal", &["2000123456782"])]);
    let b = p.stickers[0].barcode.as_ref().unwrap();
    assert_eq!((b.symbology, b.module, b.fits), (Symbology::Ean13, 3, true));
    assert_eq!(b.data, "2000123456782");
    assert!(p.warnings.is_empty());
}

#[test]
fn a_13_digit_code128_fits_a_2x1_label_at_two_dots() {
    // Not a valid EAN (bad check digit) ⇒ Code128: 143 modules with quiet zones ≈ 35.8 mm at 2 dots.
    let p = layout::layout(&builtin("two_up_50x25"), &[cell("p1", "Thing", &["2000123456781"])]);
    let b = p.stickers[0].barcode.as_ref().unwrap();
    assert_eq!((b.symbology, b.module, b.fits), (Symbology::Code128, 2, true));
    assert_eq!(b.widths.iter().map(|w| u32::from(*w)).sum::<u32>() + 2 * layout::QUIET_MODULES, 143);
}

#[test]
fn the_8_digit_in_store_barcode_prints_as_code128_c_on_both_rolls() {
    // START C, 21, 00, 04, 23, check, STOP = 6 × 11 + 13 = 79 modules, 99 with the quiet zones.
    for (id, module) in [("three_up_30x16", 2), ("two_up_50x25", 4)] {
        let p = layout::layout(&builtin(id), &[cell("p1", "Ear Stud", &["21000423"])]);
        let b = p.stickers[0].barcode.as_ref().unwrap();
        assert_eq!((b.symbology, b.data.as_str(), b.module, b.fits), (Symbology::Code128, "21000423", module, true));
        assert_eq!(b.widths.iter().map(|w| u32::from(*w)).sum::<u32>() + 2 * layout::QUIET_MODULES, 99);
        assert!(p.warnings.is_empty(), "{id}: {:?}", codes(&p));
    }
}

#[test]
fn the_bars_sit_on_whole_dots_centred_in_their_box() {
    let t = builtin("two_up_50x25");
    let p = layout::layout(&t, &[cell("p1", "Ear Stud", &["21000423"])]);
    let b = p.stickers[0].barcode.as_ref().unwrap();
    let bars = b.widths.iter().map(|w| u16::from(*w)).sum::<u16>() * u16::from(b.module);
    assert_eq!(bars, 316, "79 modules × 4 dots");
    assert_eq!(b.x, (400 - 316) / 2, "centred in the 50 mm (400-dot) box");
    let el = t.elements.iter().find(|e| e.kind == layout::ElementKind::Barcode).unwrap();
    let y = u16::try_from(layout::dots(el.y_mm, 203)).unwrap();
    let box_h = u16::try_from(layout::dots(el.h_mm, 203)).unwrap();
    assert_eq!((b.y, b.bar_h, b.hri_px), (y, box_h - 16, 16), "the bars, then 2 mm (16 dots) of digits");
}

#[test]
fn codes_that_cannot_fit_print_flagged_and_300_dpi_allows_8_digits() {
    let t = builtin("three_up_30x16");
    let ean_only = layout::layout(&t, &[cell("p1", "Ear Stud", &["2000123456782"])]);
    assert_eq!(codes(&ean_only), vec!["BARCODE_TOO_LONG"]);
    let b = ean_only.stickers[0].barcode.as_ref().unwrap();
    assert_eq!((b.module, b.fits), (2, false), "still printed, at the smallest scannable module");

    let seven = layout::layout(&t, &[cell("p1", "Ring", &["1045231"])]);
    assert!(seven.warnings.is_empty(), "7 digits fit the 30 mm barcode box at 203 dpi: {:?}", codes(&seven));
    let t300 = LabelTemplate { dpi: 300, ..t.clone() };
    let eight = layout::layout(&t300, &[cell("p1", "Ring", &["10452311"])]);
    assert!(eight.warnings.is_empty(), "8 digits fit at 300 dpi: {:?}", codes(&eight));

    let none = layout::layout(&t, &[cell("p1", "Ring", &[])]);
    assert_eq!(codes(&none), vec!["NO_CODE"]);
    assert!(none.stickers[0].barcode.is_none());
}

#[test]
fn the_stickers_read_shop_name_then_barcode_then_a_large_price() {
    use layout::ElementKind::{Barcode, Price, StoreName};
    for id in ["two_up_50x25", "three_up_30x16"] {
        let t = builtin(id);
        let kinds: Vec<_> = t.elements.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![StoreName, Barcode, Price], "{id}");
        let ys: Vec<f64> = t.elements.iter().map(|e| e.y_mm).collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "{id}: top to bottom");
        let (store, bar, price) = (&t.elements[0], &t.elements[1], &t.elements[2]);
        assert!(store.y_mm + store.h_mm <= bar.y_mm && bar.y_mm + bar.h_mm <= price.y_mm, "{id}: no overlap");
        assert!(price.y_mm + price.h_mm <= t.label_h_mm, "{id}: price inside the label");
        assert!(price.bold);
        assert!(store.font_size < price.font_size);

        let p = layout::layout(&t, &[cell("p1", "Sandal", &["104523"])]);
        assert!(p.warnings.is_empty(), "{id}: {:?}", codes(&p));
        let texts: Vec<&str> = p.stickers[0].texts.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts, vec!["Sale Center", "Rs. 1,250"], "{id}");
        let (s, pr) = (&p.stickers[0].texts[0], &p.stickers[0].texts[1]);
        assert!(pr.font_px > s.font_px, "{id}: the price is the largest text");
        assert!(pr.y >= s.y + s.h && u32::from(pr.y) + u32::from(pr.h) <= u32::from(p.height), "{id}");
    }
}

#[test]
fn text_boxes_are_whole_dots_from_the_stickers_corner_and_keep_the_whole_text() {
    let mut with_name = builtin("two_up_50x25");
    with_name.elements.push(layout::LabelElement {
        kind: layout::ElementKind::ProductName,
        x_mm: 2.0,
        y_mm: 4.5,
        w_mm: 46.0,
        h_mm: 6.0,
        font_size: 2,
        bold: true,
        align: layout::Align::Left,
        max_lines: Some(2),
    });
    let long = "Ladies Leather Sandal Comfort Fit Wide Black Extra Soft Sole With Strap";
    let p = layout::layout(&with_name, &[cell("p1", long, &["21000423"]), cell("p2", "නිල් බාල්දිය", &["21000430"])]);
    assert!(p.warnings.is_empty(), "nothing is cut or replaced in Rust: {:?}", codes(&p));
    let name = p.stickers[0].texts.iter().find(|t| t.kind == layout::ElementKind::ProductName).unwrap();
    assert_eq!((name.text.as_str(), name.max_lines), (long, 2), "the template wraps and ellipsises it");
    assert_eq!((name.x, name.y, name.w, name.font_px), (16, 36, 368, 20));
    let si = p.stickers[1].texts.iter().find(|t| t.kind == layout::ElementKind::ProductName).unwrap();
    assert_eq!(si.text, "නිල් බාල්දිය", "drawn by the webview, so Sinhala prints");
    let store = &p.stickers[0].texts[0];
    assert_eq!((store.x, store.w, store.align), (0, 400, layout::Align::Center), "relative to the sticker");
}

#[test]
fn attributes_put_size_first() {
    assert_eq!(attributes_line(r#"{"colour":"Black","size":"38"}"#), "Size 38 - Black");
    assert_eq!(attributes_line("{}"), "");
}

/// The exact TSPL `BITMAP` command for a picture: TSPL prints a 0 bit black, so every byte is inverted.
fn tspl_bitmap(x: i64, y: i64, b: &LabelBitmap) -> Vec<u8> {
    let mut out = format!("BITMAP {x},{y},{},{},0,", b.width / 8, b.height).into_bytes();
    out.extend(b.data.iter().map(|byte| !byte));
    out.extend(b"\r\n");
    out
}

const TSPL_HEADER_TWO_UP: &str =
    "SIZE 105.0 mm,25.0 mm\r\nGAP 3.0 mm,0 mm\r\nDIRECTION 1\r\nREFERENCE 0,0\r\nDENSITY 8\r\nSPEED 4\r\n";

#[test]
fn tspl_places_each_picture_on_its_column_inverted_and_repeats_identical_rows() {
    let t = builtin("two_up_50x25");
    let (a, b) = (picture(400, 200, 0), picture(400, 200, 7));
    // Four of the first product (two identical rows ⇒ PRINT 2), one of the second.
    let rows = bitmap::rows(&[(0, 4), (1, 1)], t.columns, false);
    let bytes = label::emit(&t, &tspl_profile(), &[a.clone(), b.clone()], &rows);
    // Column 1 starts after the margin, the first label and the gap: 1.5 + 50 + 2 = 53.5 mm = 428 dots.
    let mut expected = TSPL_HEADER_TWO_UP.as_bytes().to_vec();
    expected.extend(b"CLS\r\n");
    expected.extend(tspl_bitmap(12, 0, &a));
    expected.extend(tspl_bitmap(428, 0, &a));
    expected.extend(b"PRINT 2\r\nCLS\r\n");
    expected.extend(tspl_bitmap(12, 0, &b));
    expected.extend(b"PRINT 1\r\n");
    assert_eq!(bytes, expected);
    assert!(!bytes.windows(5).any(|w| w == b"TEXT "), "no printer fonts anywhere");

    // "Fill the last row" repeats the last picture into the empty column.
    let filled = label::emit(&t, &tspl_profile(), &[a.clone(), b.clone()], &bitmap::rows(&[(0, 1), (1, 2)], 2, true));
    let mut expected = TSPL_HEADER_TWO_UP.as_bytes().to_vec();
    expected.extend(b"CLS\r\n");
    expected.extend(tspl_bitmap(12, 0, &a));
    expected.extend(tspl_bitmap(428, 0, &b));
    expected.extend(b"PRINT 1\r\nCLS\r\n");
    expected.extend(tspl_bitmap(12, 0, &b));
    expected.extend(tspl_bitmap(428, 0, &b));
    expected.extend(b"PRINT 1\r\n");
    assert_eq!(filled, expected);
}

#[test]
fn zpl_places_each_picture_as_a_graphic_field_without_inverting() {
    let t = builtin("three_up_30x16");
    let a = picture(240, 128, 3);
    let zpl = LabelPrinterProfile { language: LabelLanguage::Zpl, ..tspl_profile() };
    let text =
        String::from_utf8(label::emit(&t, &zpl, std::slice::from_ref(&a), &bitmap::rows(&[(0, 6)], 3, false))).unwrap();
    let field = |x: i64| format!("^FO{x},0^GFA,3840,3840,30,{}^FS\r\n", hex::encode_upper(&a.data));
    // Columns at 1.5, 33.5 and 65.5 mm: 12, 268 and 523 dots.
    let expected = format!("~SD16^XA^PW775^LL128^LH0,0\r\n{}{}{}^PQ2\r\n^XZ\r\n", field(12), field(268), field(523));
    assert_eq!(text, expected);
}

#[test]
fn calibration_offsets_shift_every_picture_and_never_past_the_edge() {
    let t = builtin("two_up_50x25");
    let a = picture(400, 200, 1);
    let one = bitmap::rows(&[(0, 1)], 2, false);
    let shifted = LabelPrinterProfile { offset_x_mm: 1.0, offset_y_mm: 0.5, ..tspl_profile() };
    let bytes = label::emit(&t, &shifted, std::slice::from_ref(&a), &one);
    let mut expected = TSPL_HEADER_TWO_UP.as_bytes().to_vec();
    expected.extend(b"CLS\r\n");
    expected.extend(tspl_bitmap(20, 4, &a));
    expected.extend(b"PRINT 1\r\n");
    assert_eq!(bytes, expected, "1.5 + 1.0 mm = 20 dots across, 0.5 mm = 4 dots down");
    let pulled = LabelPrinterProfile { offset_x_mm: -3.0, offset_y_mm: -1.0, ..tspl_profile() };
    let bytes = label::emit(&t, &pulled, std::slice::from_ref(&a), &one);
    assert!(bytes.windows(13).any(|w| w == b"BITMAP 0,0,50"), "clamped to the roll's edge");
}

#[test]
fn the_alignment_test_outlines_every_cell() {
    let t = builtin("three_up_30x16");
    let tspl = String::from_utf8(label::tspl::alignment_test(&t, (0.0, 0.0), 8, 4)).unwrap();
    assert_eq!(tspl.matches("BOX ").count(), 3);
    golden("align_three_up.tspl", tspl.as_bytes());
    let zpl = String::from_utf8(label::zpl::alignment_test(&t, (0.0, 0.0), 8)).unwrap();
    assert_eq!(zpl.matches("^GB").count(), 3);
    golden("align_three_up.zpl", zpl.as_bytes());
}

#[test]
fn a_saved_default_sticker_size_round_trips_and_old_profiles_have_none() {
    let with = LabelPrinterProfile { default_template_id: Some("three_up_30x16".into()), ..tspl_profile() };
    assert!(with.validate().is_ok());
    let json = serde_json::to_value(&with).unwrap();
    assert_eq!(serde_json::from_value::<LabelPrinterProfile>(json).unwrap(), with);
    let mut old = serde_json::to_value(tspl_profile()).unwrap();
    old.as_object_mut().unwrap().remove("default_template_id");
    assert_eq!(serde_json::from_value::<LabelPrinterProfile>(old).unwrap().default_template_id, None);
    let blank = LabelPrinterProfile { default_template_id: Some("  ".into()), ..tspl_profile() };
    assert_eq!(validation(blank.validate().unwrap_err()), "LABEL_PROFILE_INVALID");
}

#[test]
fn the_barcode_boxes_are_short_so_the_price_stays_the_big_thing() {
    for (id, bars_mm) in [("two_up_50x25", 4.5), ("three_up_30x16", 2.5)] {
        let t = builtin(id);
        let bar = t.elements.iter().find(|e| e.kind == layout::ElementKind::Barcode).unwrap();
        assert!((bar.h_mm - 2.0 - bars_mm).abs() < 1e-9, "{id}: bars are {bars_mm} mm tall plus the 2 mm digits");
    }
}

#[tokio::test]
async fn templates_calibrate_per_device_and_reset_to_default() {
    let t = till().await;
    let cal = TemplateCalibration { col_gap_mm: 3.0, row_gap_mm: 2.5, left_margin_mm: 1.0 };
    let mut tx = t.t.db.write().await.unwrap();
    let saved = label::calibrate(tx.conn(), t.pool(), "two_up_50x25", Some(&cal)).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!((saved.col_gap_mm, saved.row_gap_mm, saved.left_margin_mm), (3.0, 2.5, 1.0));
    assert_eq!(label::template(t.pool(), "two_up_50x25").await.unwrap().col_gap_mm, 3.0);

    let bad = TemplateCalibration { col_gap_mm: 12.0, ..cal };
    let mut tx = t.t.db.write().await.unwrap();
    assert!(label::calibrate(tx.conn(), t.pool(), "two_up_50x25", Some(&bad)).await.is_err());
    let reset = label::calibrate(tx.conn(), t.pool(), "two_up_50x25", None).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(reset, builtin("two_up_50x25"));
}

fn print_job(template_id: &str, items: Vec<(String, i64, LabelBitmap)>) -> LabelPrintJob {
    LabelPrintJob {
        template_id: template_id.into(),
        items: items
            .into_iter()
            .map(|(product_id, copies, image)| LabelPrintItem { product_id, copies, image })
            .collect(),
        fill_last_row: false,
    }
}

#[tokio::test]
async fn a_job_lays_out_one_sticker_per_product_and_checks_every_picture() {
    let t = till().await;
    let sandal = product_with_codes(&t, "Ladies Sandal", 125_000, Some("2000123456782")).await;
    let stud = product_with_codes(&t, "Ear Stud", 45_000, Some("21000423")).await;
    let job = LabelJob {
        template_id: "two_up_50x25".into(),
        items: vec![
            LabelJobItem { product_id: sandal.clone(), copies: 3 },
            LabelJobItem { product_id: stud.clone(), copies: 1 },
        ],
        fill_last_row: false,
    };
    let laid = label::preview(t.pool(), &t.s.store_id, &job).await.unwrap();
    assert_eq!((laid.width, laid.height, laid.stickers.len()), (400, 200, 2), "one sticker per product, not per copy");
    let price = laid.stickers[0].texts.iter().find(|x| x.kind == layout::ElementKind::Price).unwrap();
    assert_eq!(price.text, "Rs. 1,250");
    assert_eq!(laid.stickers[1].barcode.as_ref().map(|b| b.data.as_str()), Some("21000423"));

    let good = print_job(
        "two_up_50x25",
        vec![(sandal.clone(), 3, picture(400, 200, 0)), (stud.clone(), 1, picture(400, 200, 9))],
    );
    let (payload, labels, _) = label::prepare_print(t.pool(), &t.s.store_id, &good).await.unwrap();
    assert_eq!(labels, 4);
    assert_eq!(payload["dpi"], 203);
    assert_eq!(payload["items"][0]["image"]["hex"].as_str().unwrap().len(), 20_000, "10,000 bytes as hex");

    let wrong_size = print_job("two_up_50x25", vec![(sandal.clone(), 1, picture(240, 128, 0))]);
    let e = label::prepare_print(t.pool(), &t.s.store_id, &wrong_size).await.unwrap_err();
    assert_eq!(validation(e), "LABEL_IMAGE_INVALID", "a 30 x 16 picture never goes on a 50 x 25 roll");
    let truncated = LabelBitmap { data: vec![0; 10], ..picture(400, 200, 0) };
    let e =
        label::prepare_print(t.pool(), &t.s.store_id, &print_job("two_up_50x25", vec![(sandal.clone(), 1, truncated)]))
            .await
            .unwrap_err();
    assert_eq!(validation(e), "LABEL_IMAGE_INVALID");
    for (copies, code) in [(0, "LABEL_COPIES_INVALID"), (1_000, "LABEL_COPIES_INVALID")] {
        let bad = print_job("two_up_50x25", vec![(sandal.clone(), copies, picture(400, 200, 0))]);
        assert_eq!(validation(label::prepare_print(t.pool(), &t.s.store_id, &bad).await.unwrap_err()), code);
    }
    let ghost = print_job("two_up_50x25", vec![("nope".into(), 1, picture(400, 200, 0))]);
    assert_eq!(
        validation(label::prepare_print(t.pool(), &t.s.store_id, &ghost).await.unwrap_err()),
        "LABEL_PRODUCT_INVALID"
    );
    let empty = print_job("two_up_50x25", vec![]);
    assert_eq!(validation(label::prepare_print(t.pool(), &t.s.store_id, &empty).await.unwrap_err()), "LABEL_JOB_EMPTY");
}

#[tokio::test]
async fn a_stored_job_prints_its_pictures_and_refuses_after_a_dpi_change() {
    let t = till().await;
    let sandal = product_with_codes(&t, "Ladies Sandal", 125_000, Some("21000423")).await;
    let a = picture(400, 200, 5);
    let job = print_job("two_up_50x25", vec![(sandal, 2, a.clone())]);
    let (payload, _, _) = label::prepare_print(t.pool(), &t.s.store_id, &job).await.unwrap();
    let bytes = label::render_job(t.pool(), "labels", &payload, &tspl_profile()).await.unwrap();
    let mut expected = TSPL_HEADER_TWO_UP.as_bytes().to_vec();
    expected.extend(b"CLS\r\n");
    expected.extend(tspl_bitmap(12, 0, &a));
    expected.extend(tspl_bitmap(428, 0, &a));
    expected.extend(b"PRINT 1\r\n");
    assert_eq!(bytes, expected, "the picture as drawn, on both columns");

    let at_300 = LabelPrinterProfile { dpi: 300, ..tspl_profile() };
    let e = label::render_job(t.pool(), "labels", &payload, &at_300).await.unwrap_err();
    assert_eq!(validation(e), "LABEL_DPI_CHANGED", "drawn for 203 dpi: fail rather than print at the wrong size");
}

#[tokio::test]
async fn the_test_label_is_the_drawn_sample_across_a_row_and_the_alignment_job_stays_an_outline() {
    let t = till().await;
    let sample = label::sample_preview(t.pool(), "three_up_30x16").await.unwrap();
    assert_eq!((sample.stickers.len(), sample.width, sample.height), (1, 240, 128));
    assert!(sample.warnings.is_empty());
    let texts: Vec<_> = sample.stickers[0].texts.iter().map(|x| x.text.clone()).collect();
    assert_eq!(texts, vec!["Sale Center", "Rs. 1,250"], "the sticker name, not the store's own");
    assert_eq!(sample.stickers[0].barcode.as_ref().map(|b| b.data.as_str()), Some("21000010"));

    let wrong = label::prepare_sample(t.pool(), "three_up_30x16", &picture(400, 200, 0)).await.unwrap_err();
    assert_eq!(validation(wrong), "LABEL_IMAGE_INVALID");
    let a = picture(240, 128, 2);
    let payload = label::prepare_sample(t.pool(), "three_up_30x16", &a).await.unwrap();
    let bytes = label::render_job(t.pool(), "test", &payload, &tspl_profile()).await.unwrap();
    let mut expected =
        b"SIZE 97.0 mm,16.0 mm\r\nGAP 3.0 mm,0 mm\r\nDIRECTION 1\r\nREFERENCE 0,0\r\nDENSITY 8\r\nSPEED 4\r\nCLS\r\n"
            .to_vec();
    for x in [12, 268, 523] {
        expected.extend(tspl_bitmap(x, 0, &a));
    }
    expected.extend(b"PRINT 1\r\n");
    assert_eq!(bytes, expected, "one full row of the drawn sample");

    let outline =
        label::render_job(t.pool(), "test", &serde_json::json!({"template_id": "three_up_30x16"}), &tspl_profile())
            .await
            .unwrap();
    let outline = String::from_utf8(outline).unwrap();
    assert!(outline.contains("BOX ") && !outline.contains("BITMAP"));
}

#[tokio::test]
async fn labels_print_only_the_barcode_never_the_sku() {
    let t = till().await;
    let job = |id: &str| LabelJob {
        template_id: "three_up_30x16".into(),
        items: vec![LabelJobItem { product_id: id.to_owned(), copies: 1 }],
        fill_last_row: false,
    };

    // A product with a barcode prints that barcode.
    let coded = product_with_codes(&t, "Ear Stud", 45_000, Some("21000423")).await;
    let p = label::preview(t.pool(), &t.s.store_id, &job(&coded)).await.unwrap();
    assert_eq!(p.stickers[0].barcode.as_ref().map(|b| b.data.as_str()), Some("21000423"));
    assert!(p.warnings.is_empty(), "{:?}", codes(&p));

    // A product with only a SKU gets no bars at all and says so; its SKU is nowhere on the sticker.
    let bare = product_with_codes(&t, "Hair Clip", 15_000, None).await;
    let sku: String =
        sqlx::query_scalar("SELECT sku FROM products WHERE id = ?1").bind(&bare).fetch_one(t.pool()).await.unwrap();
    let p = label::preview(t.pool(), &t.s.store_id, &job(&bare)).await.unwrap();
    assert!(p.stickers[0].barcode.is_none());
    assert_eq!(codes(&p), vec!["NO_CODE"]);
    assert!(p.stickers[0].texts.iter().all(|x| !x.text.contains(&sku)), "{:?}", p.stickers[0].texts);
    let texts: Vec<_> = p.stickers[0].texts.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, vec!["Sale Center", "Rs. 150"], "the sticker still shows its shop name and price");
}

fn validation(e: gayanshop_lib::error::AppError) -> String {
    match e {
        gayanshop_lib::error::AppError::Validation { code, .. } => code,
        other => panic!("expected validation, got {other:?}"),
    }
}

async fn product_with_codes(t: &Till, name: &str, price: i64, barcode: Option<&str>) -> String {
    let id = t.product(name, price, true, 0, None).await;
    common::exec(
        t.pool(),
        &format!(
            "UPDATE products SET barcode = {}, attributes = '{{\"size\":\"38\",\"colour\":\"Black\"}}' WHERE id = '{id}'",
            barcode.map_or("NULL".to_owned(), |b| format!("'{b}'"))
        ),
    )
    .await;
    id
}

/// Records what reached the label printer; panics if anyone asks it for ESC/POS status.
struct LabelFake {
    sent: Mutex<Vec<Vec<u8>>>,
}

impl PrinterTransport for LabelFake {
    fn send(&self, bytes: &[u8]) -> Result<(), PrintFault> {
        self.sent.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }

    fn status(&self) -> Result<Option<PrinterStatus>, PrintFault> {
        panic!("a TSPL/ZPL printer must never be sent DLE EOT");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_label_job_prints_through_the_label_printers_own_queue() {
    let t = till().await;
    let p = product_with_codes(&t, "Ladies Sandal", 125_000, Some("2000123456782")).await;
    let a = picture(400, 200, 4);
    {
        let (payload, _, _) =
            label::prepare_print(t.pool(), &t.s.store_id, &print_job("two_up_50x25", vec![(p, 2, a.clone())]))
                .await
                .unwrap();
        let mut tx = t.t.db.write().await.unwrap();
        settings::device_set(tx.conn(), &profile_key(LABEL_PRINTER), &tspl_profile()).await.unwrap();
        print_jobs::enqueue_for(tx.conn(), &t.ctx(), LABEL_PRINTER, PrintKind::Labels, None, &payload).await.unwrap();
        print_jobs::enqueue_for(
            tx.conn(),
            &t.ctx(),
            LABEL_PRINTER,
            PrintKind::Test,
            None,
            &serde_json::json!({"template_id": "three_up_30x16"}),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let fake = Arc::new(LabelFake { sent: Mutex::new(Vec::new()) });
    let f = fake.clone();
    // Jobs committed before the worker started are "from before a restart": released here as "Print now".
    let hub = PrintHub::start(
        t.t.db.clone(),
        LABEL_PRINTER,
        Arc::new(move |_| f.clone() as Arc<dyn PrinterTransport>),
        Arc::new(gayanshop_lib::clock::SystemClock),
        chrono_tz::Asia::Colombo,
        WorkerConfig { io_timeout: Duration::from_millis(500), ..WorkerConfig::default() },
        Arc::new(|| {}),
    )
    .await
    .unwrap();
    let held = hub.take_held().unwrap();
    print_jobs::requeue(&t.t.db, &t.ctx(), &held).await.unwrap();
    hub.notify();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while t.count("SELECT COUNT(*) FROM print_jobs WHERE printer_key = 'label' AND status = 'done'").await < 2 {
        assert!(std::time::Instant::now() < deadline, "label jobs didn't print");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let sent = fake.sent.lock().unwrap().clone();
    let picture_bytes = tspl_bitmap(12, 0, &a);
    assert!(sent.iter().any(|s| s.windows(picture_bytes.len()).any(|w| w == picture_bytes.as_slice())), "the picture");
    assert!(sent.iter().any(|s| String::from_utf8_lossy(s).matches("BOX ").count() == 3), "the alignment test");
    assert_eq!(t.count("SELECT COUNT(*) FROM print_jobs WHERE printer_key = 'receipt'").await, 0, "receipts untouched");
}

#[test]
fn a_store_seeded_with_the_old_rolls_moves_to_the_new_ones_and_keeps_calibration_and_custom_rolls() {
    let builtins = label::builtin_templates().unwrap();
    let old_two = LabelTemplate { id: "two_up_2x1".into(), ..builtin("two_up_50x25") };
    let old_three = LabelTemplate { id: "three_up_1x1".into(), ..builtin("three_up_30x16") };
    let calibrated = LabelTemplate { col_gap_mm: 3.5, ..builtin("three_up_30x16") };
    let custom = LabelTemplate { id: "mine".into(), ..builtin("two_up_50x25") };

    let migrated = label::reconcile(vec![old_two, old_three, custom.clone()], builtins.clone());
    let ids: Vec<&str> = migrated.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["two_up_50x25", "three_up_30x16", "mine"]);
    assert_eq!(migrated[2], custom);

    let kept = label::reconcile(vec![calibrated.clone(), builtin("two_up_50x25")], builtins.clone());
    assert_eq!(kept.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["two_up_50x25", "three_up_30x16"]);
    assert_eq!(kept[1].col_gap_mm, 3.5, "a calibrated current roll is not overwritten");
    let stale = LabelTemplate { elements: vec![], col_gap_mm: 3.5, ..builtin("three_up_30x16") };
    let refreshed = label::reconcile(vec![stale], builtins.clone());
    assert_eq!(refreshed[1].col_gap_mm, 3.5, "calibration survives");
    assert_eq!(refreshed[1].elements, builtin("three_up_30x16").elements, "the sticker design is refreshed");
    assert_eq!(label::reconcile(builtins.clone(), builtins.clone()), builtins);
}
