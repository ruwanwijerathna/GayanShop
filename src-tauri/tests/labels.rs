//! Block 2.2 — the label engine (doc §7.4): barcode-fit rules on the 2-up 50 × 25 mm and 3-up 30 × 16 mm templates,
//! golden TSPL / ZPL byte snapshots, multi-column rows, calibration, and a label job through the label
//! printer's own print worker. Regenerate the snapshots with `UPDATE_FIXTURES=1 cargo test --test labels`
//! (and review the diff: these bytes go to a real printer).

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Till, till};
use gayanshop_lib::db::repo::print_jobs::{self, PrintKind};
use gayanshop_lib::db::settings;
use gayanshop_lib::hardware::label::barcode::Symbology;
use gayanshop_lib::hardware::label::layout::LabelLanguage;
use gayanshop_lib::hardware::label::layout::{self, LabelCell, LabelPreview, LabelTemplate};
use gayanshop_lib::hardware::label::{
    self, LABEL_PRINTER, LabelJob, LabelJobItem, LabelPrinterProfile, TemplateCalibration, attributes_line,
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

fn codes(p: &LabelPreview) -> Vec<String> {
    p.warnings.iter().map(|w| w.code.clone()).collect()
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
fn ean13_fits_a_2x1_label_at_a_three_dot_module() {
    let p =
        layout::layout(&builtin("two_up_50x25"), &[cell("p1", "Ladies Sandal", &["2000123456782"])], (0.0, 0.0), false);
    let b = p.labels[0].barcode.as_ref().unwrap();
    assert_eq!((b.symbology, b.module_dots, b.fits), (Symbology::Ean13, 3, true));
    assert_eq!(b.data, "2000123456782");
    assert!(p.warnings.is_empty());
}

#[test]
fn a_13_digit_code128_fits_a_2x1_label_at_two_dots() {
    // Not a valid EAN (bad check digit) ⇒ Code128: 143 modules with quiet zones ≈ 35.8 mm at 2 dots.
    let p = layout::layout(&builtin("two_up_50x25"), &[cell("p1", "Thing", &["2000123456781"])], (0.0, 0.0), false);
    let b = p.labels[0].barcode.as_ref().unwrap();
    assert_eq!((b.symbology, b.module_dots, b.fits), (Symbology::Code128, 2, true));
    assert_eq!(b.widths.iter().map(|w| u32::from(*w)).sum::<u32>() + 2 * layout::QUIET_MODULES, 143);
}

#[test]
fn the_8_digit_in_store_barcode_prints_as_code128_c_on_both_rolls() {
    // START C, 21, 00, 04, 23, check, STOP = 6 × 11 + 13 = 79 modules, 99 with the quiet zones.
    for (id, module) in [("three_up_30x16", 2), ("two_up_50x25", 4)] {
        let p = layout::layout(&builtin(id), &[cell("p1", "Ear Stud", &["21000423"])], (0.0, 0.0), false);
        let b = p.labels[0].barcode.as_ref().unwrap();
        assert_eq!(
            (b.symbology, b.data.as_str(), b.module_dots, b.fits),
            (Symbology::Code128, "21000423", module, true)
        );
        assert_eq!(b.widths.iter().map(|w| u32::from(*w)).sum::<u32>() + 2 * layout::QUIET_MODULES, 99);
        assert!(p.warnings.is_empty(), "{id}: {:?}", codes(&p));
    }
}

#[test]
fn codes_that_cannot_fit_print_flagged_and_300_dpi_allows_8_digits() {
    let t = builtin("three_up_30x16");
    let ean_only = layout::layout(&t, &[cell("p1", "Ear Stud", &["2000123456782"])], (0.0, 0.0), false);
    assert_eq!(codes(&ean_only), vec!["BARCODE_TOO_LONG"]);
    let b = ean_only.labels[0].barcode.as_ref().unwrap();
    assert_eq!((b.module_dots, b.fits), (2, false), "still printed, at the smallest scannable module");

    let seven = layout::layout(&t, &[cell("p1", "Ring", &["1045231"])], (0.0, 0.0), false);
    assert!(seven.warnings.is_empty(), "7 digits fit the 28 mm barcode box at 203 dpi: {:?}", codes(&seven));
    let mut t300 = t.clone();
    t300.dpi = 300;
    let eight = layout::layout(&t300, &[cell("p1", "Ring", &["10452311"])], (0.0, 0.0), false);
    assert!(eight.warnings.is_empty(), "8 digits fit at 300 dpi: {:?}", codes(&eight));

    let none = layout::layout(&t, &[cell("p1", "Ring", &[])], (0.0, 0.0), false);
    assert_eq!(codes(&none), vec!["NO_CODE"]);
}

#[test]
fn names_wrap_and_unprintable_text_is_flagged() {
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
    let p = layout::layout(
        &with_name,
        &[
            cell("p1", "Ladies Leather Sandal Comfort Fit Wide Black Extra Soft Sole With Strap", &["104523"]),
            cell("p2", "නිල් බාල්දිය", &["104524"]),
        ],
        (0.0, 0.0),
        false,
    );
    let name = p.labels[0].texts.iter().find(|t| t.kind == layout::ElementKind::ProductName).unwrap();
    assert_eq!(name.lines.len(), 2);
    assert!(name.lines[1].ends_with("..."));
    assert_eq!(codes(&p), vec!["TEXT_NOT_PRINTABLE"]);
    assert_eq!(p.warnings[0].product_id, "p2");
    let si = p.labels[1].texts.iter().find(|t| t.kind == layout::ElementKind::ProductName).unwrap();
    assert!(si.lines[0].chars().all(|c| c == '?' || c == ' '));
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
        assert!(price.font_size >= 4, "{id}: the price is the largest text");
        assert!(price.bold);
        assert!(store.font_size < price.font_size);

        let p = layout::layout(&t, &[cell("p1", "Sandal", &["104523"])], (0.0, 0.0), false);
        assert!(p.warnings.is_empty(), "{id}: {:?}", codes(&p));
        let texts: Vec<&str> = p.labels[0].texts.iter().map(|x| x.lines[0].as_str()).collect();
        assert_eq!(texts, vec!["Sale Center", "Rs. 1,250"], "{id}");
    }
}

#[test]
fn attributes_put_size_first() {
    assert_eq!(attributes_line(r#"{"colour":"Black","size":"38","brand":"Bata"}"#), "Size 38 - Bata - Black");
    assert_eq!(attributes_line("{}"), "");
}

#[test]
fn cells_fill_rows_left_to_right_and_the_last_row_can_be_filled() {
    let t = builtin("two_up_50x25");
    let cells = vec![cell("a", "A", &["104523"]), cell("a", "A", &["104523"]), cell("b", "B", &["104524"])];
    let p = layout::layout(&t, &cells, (0.0, 0.0), false);
    assert_eq!((p.rows, p.labels.len()), (2, 3));
    assert_eq!(p.labels.iter().map(|l| (l.row, l.col)).collect::<Vec<_>>(), vec![(0, 0), (0, 1), (1, 0)]);
    // Column 2 starts after the label and the gap: 1.5 + 50.0 + 2.0 = 53.5 mm.
    assert_eq!(layout::mm_text(p.labels[1].x_mm), "53.5");
    let filled = layout::layout(&t, &cells, (0.0, 0.0), true);
    assert_eq!(filled.labels.len(), 4);
    assert_eq!(filled.labels[3].product_id, "b");
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

fn sample_job() -> (LabelTemplate, Vec<LabelCell>) {
    let sandal = cell("p1", "Ladies Sandal Leather Comfort Fit", &["2000123456782"]);
    let stud =
        LabelCell { attributes: String::new(), price: "Rs. 450".into(), ..cell("p2", "Silver Ear Stud", &["104523"]) };
    // Four sandals (two identical rows ⇒ PRINT 2) and one stud.
    let mut cells = vec![sandal.clone(), sandal.clone(), sandal.clone(), sandal];
    cells.push(stud);
    (builtin("two_up_50x25"), cells)
}

#[test]
fn tspl_golden_two_up() {
    let (t, cells) = sample_job();
    let p = layout::layout(&t, &cells, (0.0, 0.0), false);
    let bytes = label::emit(&p, &tspl_profile());
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("SIZE 105.0 mm,25.0 mm\r\nGAP 3.0 mm,0 mm\r\nDIRECTION 1\r\nREFERENCE 0,0\r\n"));
    assert!(text.contains("PRINT 2\r\n"), "identical rows collapse");
    assert_eq!(text.matches("CLS\r\n").count(), 2);
    assert!(text.contains("BARCODE") && text.contains("\"EAN13\"") && text.contains("\"200012345678\""));
    assert!(text.contains("\"Rs. 1,250\"") && text.contains("\"Sale Center\""));
    golden("two_up_50x25.tspl", &bytes);
}

#[test]
fn tspl_golden_three_up() {
    let t = builtin("three_up_30x16");
    let cells = vec![cell("p1", "Silver Ear Stud Round 6mm", &["21000423"]); 3];
    let p = layout::layout(&t, &cells, (0.0, 0.0), false);
    let bytes = label::emit(&p, &tspl_profile());
    assert!(String::from_utf8(bytes.clone()).unwrap().contains("\"128\""));
    golden("three_up_30x16.tspl", &bytes);
}

#[test]
fn zpl_golden_two_up() {
    let (t, cells) = sample_job();
    let p = layout::layout(&t, &cells, (0.0, 0.0), false);
    let zpl = LabelPrinterProfile { language: LabelLanguage::Zpl, ..tspl_profile() };
    let bytes = label::emit(&p, &zpl);
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("~SD16^XA^PW839^LL200^LH0,0"), "{}", &text[..40]);
    assert!(text.contains("^PQ2\r\n^XZ"), "identical rows collapse");
    assert!(text.contains("^BEN,") && text.contains("^BY3"));
    golden("two_up_50x25.zpl", &bytes);
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
fn calibration_offsets_shift_everything() {
    let (t, cells) = sample_job();
    let p0 = layout::layout(&t, &cells, (0.0, 0.0), false);
    let p1 = layout::layout(&t, &cells, (1.0, 0.5), false);
    assert_eq!(layout::mm_text(p1.labels[0].x_mm - p0.labels[0].x_mm), "1.0");
    let (b0, b1) = (p0.labels[0].barcode.as_ref().unwrap(), p1.labels[0].barcode.as_ref().unwrap());
    assert_eq!(layout::mm_text(b1.y_mm - b0.y_mm), "0.5");
}

#[tokio::test]
async fn the_test_label_is_a_sample_sticker_row_and_the_alignment_job_stays_an_outline() {
    let t = till().await;
    let sample = label::sample_preview(t.pool(), "three_up_30x16").await.unwrap();
    assert_eq!(sample.labels.len(), 3, "one full row of the roll");
    assert!(sample.warnings.is_empty());
    let texts: Vec<_> = sample.labels[0].texts.iter().map(|x| x.lines[0].clone()).collect();
    assert_eq!(texts, vec!["Sale Center", "Rs. 1,250"], "the sticker name, not the store's own");

    let profile = tspl_profile();
    let bytes = label::render_job(
        t.pool(),
        "test",
        &serde_json::json!({"template_id": "three_up_30x16", "sample": true}),
        &profile,
    )
    .await
    .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"Rs. 1,250\"") && text.contains("BARCODE") && text.contains("\"21000010\""));
    let outline = label::render_job(t.pool(), "test", &serde_json::json!({"template_id": "three_up_30x16"}), &profile)
        .await
        .unwrap();
    assert!(!String::from_utf8(outline).unwrap().contains("BARCODE"));
}

#[test]
fn a_300_dpi_head_gets_larger_font_cells_so_text_is_centred_and_fitted_correctly() {
    assert_eq!(layout::font_dots(2, 203), (12, 20));
    assert_eq!(layout::font_dots(2, 300), (16, 28));
    let t = LabelTemplate { dpi: 300, ..builtin("two_up_50x25") };
    let p = layout::layout(&t, &[cell("p1", "Sandal", &["104523"])], (0.0, 0.0), false);
    let store = &p.labels[0].texts[0];
    let (cw, _) = layout::font_dots(store.font_size, 300);
    // Fixed pitch; bold is the same line struck again one dot to the right.
    let text_w = i64::try_from(store.lines[0].chars().count()).unwrap() * cw + i64::from(store.bold);
    let expected = layout::dots(store.x_mm, 300) + (layout::dots(store.w_mm, 300) - text_w) / 2;
    assert_eq!(layout::text_x_dots(store, &store.lines[0], 300), expected);
}

/// The `x` of every `TEXT` line of a TSPL job, with its font and text.
fn tspl_texts(bytes: &[u8]) -> Vec<(i64, u8, String)> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .filter_map(|l| l.strip_prefix("TEXT "))
        .map(|rest| {
            let parts: Vec<&str> = rest.splitn(7, ',').collect();
            let font = parts[2].trim_matches('"').parse().unwrap();
            (parts[0].parse().unwrap(), font, parts[6].trim_matches('"').to_owned())
        })
        .collect()
}

#[test]
fn centred_tspl_text_prints_centred_on_its_sticker_like_the_preview() {
    for id in ["two_up_50x25", "three_up_30x16"] {
        let t = builtin(id);
        let cells = vec![cell("p1", "Sandal", &["104523"]); usize::from(t.columns)];
        let p = layout::layout(&t, &cells, (0.0, 0.0), false);
        let printed = tspl_texts(&label::emit(&p, &tspl_profile()));
        for l in &p.labels {
            for text in &l.texts {
                let line = &text.lines[0];
                let (x, font, _) = printed
                    .iter()
                    .find(|(x, _, s)| s == line && *x == layout::text_x_dots(text, line, 203))
                    .cloned()
                    .unwrap_or_else(|| panic!("{id}: {line} not printed at its laid-out x"));
                // TSPL fonts are fixed pitch: each character is the font cell wide, bold adds the 1-dot restrike.
                let (cw, _) = layout::font_dots(font, 203);
                let width = i64::try_from(line.chars().count()).unwrap() * cw + i64::from(text.bold);
                let centre_twice = 2 * x + width;
                let box_centre_twice = 2 * layout::dots(text.x_mm, 203) + layout::dots(text.w_mm, 203);
                assert!(
                    (centre_twice - box_centre_twice).abs() <= 2,
                    "{id}: {line} is centred at {centre_twice}/2, its sticker at {box_centre_twice}/2"
                );
            }
        }
    }
}

#[test]
fn font_steps_above_five_print_and_measure_as_tspl_font_five() {
    assert_eq!(layout::font_dots(5, 203), (32, 48));
    assert_eq!(layout::font_dots(6, 203), layout::font_dots(5, 203));
    assert_eq!(layout::tspl_font(6), 5);
    assert_eq!(layout::tspl_font(0), 1);
    // The 50 x 25 price (step 6, bold) is font "5" and has room for "Rs. 12,500": 400 dots / 32 = 12 chars.
    let t = builtin("two_up_50x25");
    let c = LabelCell { price: "Rs. 12,500".into(), ..cell("p1", "Sandal", &["104523"]) };
    let p = layout::layout(&t, &[c], (0.0, 0.0), false);
    assert!(p.warnings.is_empty(), "{:?}", codes(&p));
    let printed = tspl_texts(&label::emit(&p, &tspl_profile()));
    assert!(printed.iter().any(|(_, f, s)| *f == 5 && s == "Rs. 12,500"), "{printed:?}");
}

#[test]
fn zpl_aligns_every_text_line_inside_its_box() {
    let t = builtin("two_up_50x25");
    let p = layout::layout(&t, &[cell("p1", "Sandal", &["104523"])], (0.0, 0.0), false);
    let zpl = LabelPrinterProfile { language: LabelLanguage::Zpl, ..tspl_profile() };
    let text = String::from_utf8(label::emit(&p, &zpl)).unwrap();
    for line in ["Sale Center", "Rs. 1,250"] {
        assert!(text.contains(&format!(",1,0,C^FD{line}^FS")), "{line}: {text}");
    }
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

#[tokio::test]
async fn a_job_previews_from_products_and_validates_copies() {
    let t = till().await;
    let p = product_with_codes(&t, "Ladies Sandal", 125_000, Some("2000123456782")).await;
    let job = LabelJob {
        template_id: "two_up_50x25".into(),
        items: vec![LabelJobItem { product_id: p.clone(), copies: 3 }],
        fill_last_row: false,
    };
    let preview = label::preview(t.pool(), &t.s.store_id, &job).await.unwrap();
    assert_eq!((preview.rows, preview.labels.len()), (2, 3));
    let price = preview.labels[0].texts.iter().find(|x| x.kind == layout::ElementKind::Price).unwrap();
    assert_eq!(price.lines, vec!["Rs. 1,250"]);

    for (copies, code) in [(0, "LABEL_COPIES_INVALID"), (1_000, "LABEL_COPIES_INVALID")] {
        let bad = LabelJob { items: vec![LabelJobItem { product_id: p.clone(), copies }], ..job.clone() };
        assert_eq!(validation(label::preview(t.pool(), &t.s.store_id, &bad).await.unwrap_err()), code);
    }
    let empty = LabelJob { items: vec![], ..job.clone() };
    assert_eq!(validation(label::preview(t.pool(), &t.s.store_id, &empty).await.unwrap_err()), "LABEL_JOB_EMPTY");
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
    assert_eq!(p.labels[0].barcode.as_ref().map(|b| b.data.as_str()), Some("21000423"));
    assert!(p.warnings.is_empty(), "{:?}", codes(&p));

    // A product with only a SKU prints no bars at all and says so; its SKU is never printed.
    let bare = product_with_codes(&t, "Hair Clip", 15_000, None).await;
    let sku: String =
        sqlx::query_scalar("SELECT sku FROM products WHERE id = ?1").bind(&bare).fetch_one(t.pool()).await.unwrap();
    let p = label::preview(t.pool(), &t.s.store_id, &job(&bare)).await.unwrap();
    assert!(p.labels[0].barcode.is_none());
    assert_eq!(codes(&p), vec!["NO_CODE"]);
    let payload = serde_json::json!({
        "template_id": "three_up_30x16",
        "items": [{ "product_id": bare, "copies": 1 }],
        "store_id": t.s.store_id,
    });
    let bytes =
        String::from_utf8(label::render_job(t.pool(), "labels", &payload, &tspl_profile()).await.unwrap()).unwrap();
    assert!(!bytes.contains("BARCODE") && !bytes.contains(&sku), "{bytes}");
    assert!(bytes.contains("\"Rs. 150\""), "the sticker still prints its shop name and price: {bytes}");
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
    {
        let mut tx = t.t.db.write().await.unwrap();
        settings::device_set(tx.conn(), &profile_key(LABEL_PRINTER), &tspl_profile()).await.unwrap();
        let job = LabelJob {
            template_id: "two_up_50x25".into(),
            items: vec![LabelJobItem { product_id: p, copies: 2 }],
            fill_last_row: false,
        };
        let mut payload = serde_json::to_value(&job).unwrap();
        payload["store_id"] = serde_json::json!(t.s.store_id);
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
    let all = sent.iter().map(|b| String::from_utf8_lossy(b).into_owned()).collect::<Vec<_>>();
    assert!(all.iter().any(|s| s.contains("\"EAN13\"") && s.contains("PRINT 1")), "{all:?}");
    assert!(all.iter().any(|s| s.matches("BOX ").count() == 3), "the alignment test");
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
