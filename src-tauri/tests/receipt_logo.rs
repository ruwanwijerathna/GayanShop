//! The receipt's header image: size/shape validation, the `GS v 0` bytes at the top of a printed receipt, and
//! the device setting that keeps it.

mod common;

use common::till;
use gayanshop_lib::db::settings;
use gayanshop_lib::error::AppError;
use gayanshop_lib::hardware::escpos::render::{self, LOGO_KEY};
use gayanshop_lib::hardware::escpos::{LOGO_MAX_HEIGHT, LOGO_MAX_WIDTH, Layout, Logo};

fn logo(width: u16, height: u16) -> Logo {
    Logo { width, height, data: vec![0xAA; usize::from(width / 8) * usize::from(height)] }
}

fn code(e: AppError) -> String {
    match e {
        AppError::Validation { code, .. } => code,
        other => panic!("expected validation, got {other:?}"),
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[test]
fn only_well_formed_images_within_the_paper_width_are_accepted() {
    assert!(logo(384, 100).validate().is_ok());
    assert!(logo(LOGO_MAX_WIDTH, LOGO_MAX_HEIGHT).validate().is_ok());
    for bad in [
        logo(392, 10),
        logo(380, 10),
        logo(0, 10),
        logo(64, 0),
        logo(64, LOGO_MAX_HEIGHT + 1),
        Logo { width: 64, height: 4, data: vec![0; 31] },
    ] {
        assert_eq!(code(bad.validate().unwrap_err()), "LOGO_INVALID");
    }
}

#[test]
fn the_image_prints_first_as_a_centred_raster_and_a_plain_receipt_has_none() {
    let l = logo(64, 3);
    let tz = chrono_tz::Asia::Colombo;
    let now = "2026-11-16T17:20:00Z";
    let with = render::test_page("Sale Center", "Thanks!", Layout::default(), tz, now, Some(&l));
    let without = render::test_page("Sale Center", "Thanks!", Layout::default(), tz, now, None);
    for page in [&with, &without] {
        assert!(find(page, b"SAMPLE RECEIPT").is_some() && find(page, b"Sample item B").is_some());
        assert!(find(page, b"2026-11-16 22:50").is_some(), "printed in the store's time zone");
        assert!(find(page, b"Thanks!").is_some(), "the store's own footer");
    }
    // GS v 0 m xL xH yL yH: 8 bytes per row, 3 rows.
    let header = [0x1D, b'v', b'0', 0, 8, 0, 3, 0];
    let at = find(&with, &header).expect("raster command present");
    assert_eq!(&with[at + 8..at + 8 + 24], l.data.as_slice());
    assert!(at < find(&with, b"Sale Center").expect("store name"), "image above the store name");
    assert_eq!(&with[at - 3..at], &[0x1B, b'a', 1], "centred");
    assert!(find(&without, &[0x1D, b'v', b'0']).is_none());
}

#[tokio::test]
async fn the_image_is_kept_as_a_device_setting_and_can_be_removed() {
    let t = till().await;
    assert_eq!(render::load_logo(t.pool()).await.unwrap(), None);
    let l = logo(128, 20);
    let mut tx = t.t.db.write().await.unwrap();
    settings::device_set(tx.conn(), LOGO_KEY, &l).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(render::load_logo(t.pool()).await.unwrap(), Some(l));
    let mut tx = t.t.db.write().await.unwrap();
    settings::device_delete(tx.conn(), LOGO_KEY).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(render::load_logo(t.pool()).await.unwrap(), None);
}
