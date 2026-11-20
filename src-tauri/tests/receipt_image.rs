//! A receipt drawn by the webview (the Sinhala template) printed as a picture: validation, the banded
//! `GS v 0` bytes, the cut, and the queued job being rendered from its payload.

mod common;

use common::till;
use gayanshop_lib::error::AppError;
use gayanshop_lib::hardware::escpos::render;
use gayanshop_lib::hardware::escpos::{Layout, RECEIPT_IMAGE_MAX_HEIGHT, ReceiptImage};
use serde_json::json;

fn picture(width: u16, height: u16) -> ReceiptImage {
    ReceiptImage { width, height, data: vec![0x55; usize::from(width / 8) * usize::from(height)] }
}

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected validation, got {other:?}"),
    }
}

fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    hay.windows(needle.len()).enumerate().filter(|(_, w)| *w == needle).map(|(i, _)| i).collect()
}

#[test]
fn only_well_formed_pictures_within_the_paper_are_accepted() {
    assert!(picture(576, 900).validate().is_ok());
    assert!(picture(576, RECEIPT_IMAGE_MAX_HEIGHT).validate().is_ok());
    for bad in [
        picture(584, 10),
        picture(380, 10),
        picture(0, 10),
        picture(64, 0),
        picture(64, RECEIPT_IMAGE_MAX_HEIGHT + 1),
        ReceiptImage { width: 64, height: 4, data: vec![0; 31] },
    ] {
        assert_eq!(code(bad.validate().unwrap_err()), "RECEIPT_IMAGE_INVALID");
    }
}

#[test]
fn a_short_picture_is_one_raster_followed_by_the_cut() {
    let image = picture(64, 3);
    let bytes = render::receipt_image(&image, Layout::default());
    let header = [0x1D, b'v', b'0', 0, 8, 0, 3, 0];
    let at = find_all(&bytes, &header);
    assert_eq!(at.len(), 1, "one band");
    let start = at[0] + header.len();
    assert_eq!(&bytes[start..start + 24], image.data.as_slice());
    assert!(find_all(&bytes[start + 24..], &[0x1D, b'V']).len() == 1, "cut after the picture");
}

#[test]
fn a_tall_picture_goes_out_in_bands_that_add_up_to_every_row_in_order() {
    let rows: u16 = 600;
    let width: u16 = 64;
    let mut image = picture(width, rows);
    for (i, b) in image.data.iter_mut().enumerate() {
        *b = u8::try_from(i % 251).unwrap_or(0);
    }
    let bytes = render::receipt_image(&image, Layout::default());
    let marks = find_all(&bytes, &[0x1D, b'v', b'0', 0, 8, 0]);
    assert_eq!(marks.len(), 3, "256 + 256 + 88 rows");
    let mut seen = Vec::new();
    let mut total_rows: usize = 0;
    for at in marks {
        let band_rows = usize::from(bytes[at + 6]) + usize::from(bytes[at + 7]) * 256;
        total_rows += band_rows;
        seen.extend_from_slice(&bytes[at + 8..at + 8 + band_rows * 8]);
    }
    assert_eq!(total_rows, usize::from(rows));
    assert_eq!(seen, image.data, "bands carry the picture unchanged");
}

#[tokio::test]
async fn a_queued_receipt_job_with_an_image_prints_the_picture_and_a_bad_one_is_refused() {
    let t = till().await;
    let tz = chrono_tz::Asia::Colombo;
    let now = "2026-11-21T10:00:00Z";
    let image = picture(576, 40);
    let payload = json!({ "image": image });
    let bytes = render::job(t.pool(), "receipt", &payload, Layout::default(), tz, now).await.unwrap();
    assert_eq!(bytes, render::receipt_image(&image, Layout::default()));

    let bad = json!({ "image": { "width": 12, "height": 2, "data": [0, 0] } });
    let err = render::job(t.pool(), "receipt", &bad, Layout::default(), tz, now).await.unwrap_err();
    assert_eq!(code(err), "RECEIPT_IMAGE_INVALID");

    let garbled = json!({ "image": "not a picture" });
    assert!(render::job(t.pool(), "receipt", &garbled, Layout::default(), tz, now).await.is_err());
}
