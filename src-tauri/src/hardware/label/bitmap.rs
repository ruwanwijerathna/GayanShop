//! Sticker pictures (decision #30): the 1-bit image the webview draws for one product's sticker, checked
//! against the template's exact size, stored compactly in the print job, and laid out into the roll's rows.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

/// One sticker as the webview drew it: `width` × `height` dots, row by row, 8 dots per byte with the
/// leftmost dot in the high bit, and a 1 bit for black (the receipt picture's packing).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct LabelBitmap {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

impl LabelBitmap {
    pub fn row_bytes(&self) -> usize {
        usize::from(self.width / 8)
    }

    /// The picture is exactly the sticker the template describes at this printer's dpi.
    pub fn validate(&self, expected: (u16, u16)) -> AppResult<()> {
        let bad = |why: String| Err(AppError::validation("LABEL_IMAGE_INVALID", why));
        if (self.width, self.height) != expected {
            return bad(format!("{}x{} dots, the sticker is {}x{}", self.width, self.height, expected.0, expected.1));
        }
        if self.width == 0 || self.height == 0 || !self.width.is_multiple_of(8) {
            return bad(format!("{}x{} dots", self.width, self.height));
        }
        if self.data.len() != self.row_bytes() * usize::from(self.height) {
            return bad(format!("{} bytes", self.data.len()));
        }
        Ok(())
    }
}

/// How a picture is kept in a `print_jobs` payload: hex, about half the size of a JSON number list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredBitmap {
    pub width: u16,
    pub height: u16,
    pub hex: String,
}

impl From<&LabelBitmap> for StoredBitmap {
    fn from(b: &LabelBitmap) -> Self {
        StoredBitmap { width: b.width, height: b.height, hex: hex::encode(&b.data) }
    }
}

impl StoredBitmap {
    pub fn decode(&self) -> AppResult<LabelBitmap> {
        let data =
            hex::decode(&self.hex).map_err(|e| AppError::internal(format!("label picture in a print job: {e}")))?;
        Ok(LabelBitmap { width: self.width, height: self.height, data })
    }
}

/// The roll's rows: each row lists, per column, the index of the picture printed there. Pictures repeat
/// `copies` times in job order and fill rows left to right; a partial last row is left blank, or filled with
/// copies of the last picture when `fill_last_row`.
pub fn rows(copies: &[(usize, u32)], columns: u8, fill_last_row: bool) -> Vec<Vec<usize>> {
    let cols = usize::from(columns.max(1));
    let mut cells: Vec<usize> =
        copies.iter().flat_map(|&(i, n)| std::iter::repeat_n(i, usize::try_from(n).unwrap_or(0))).collect();
    if fill_last_row && let Some(&last) = cells.last() {
        while !cells.len().is_multiple_of(cols) {
            cells.push(last);
        }
    }
    cells.chunks(cols).map(<[usize]>::to_vec).collect()
}

/// Consecutive identical rows as (row, how many times), so the printer repeats a row instead of receiving it
/// again (`PRINT n` / `^PQn`).
pub fn runs(rows: &[Vec<usize>]) -> Vec<(&[usize], u32)> {
    let mut out: Vec<(&[usize], u32)> = Vec::new();
    for r in rows {
        match out.last_mut() {
            Some((prev, n)) if *prev == r.as_slice() => *n += 1,
            _ => out.push((r.as_slice(), 1)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(w: u16, h: u16) -> LabelBitmap {
        LabelBitmap { width: w, height: h, data: vec![0xA5; usize::from(w / 8) * usize::from(h)] }
    }

    #[test]
    fn a_picture_must_be_exactly_the_sticker() {
        assert!(bitmap(400, 200).validate((400, 200)).is_ok());
        assert!(bitmap(392, 200).validate((400, 200)).is_err());
        let short = LabelBitmap { data: vec![0; 10], ..bitmap(400, 200) };
        assert!(short.validate((400, 200)).is_err());
    }

    #[test]
    fn stored_pictures_round_trip() {
        let b = bitmap(64, 3);
        assert_eq!(StoredBitmap::from(&b).decode().unwrap(), b);
    }

    #[test]
    fn copies_fill_rows_and_identical_rows_collapse() {
        let r = rows(&[(0, 4), (1, 1)], 2, false);
        assert_eq!(r, vec![vec![0, 0], vec![0, 0], vec![1]]);
        assert_eq!(runs(&r), vec![(&[0, 0][..], 2), (&[1][..], 1)]);
        assert_eq!(rows(&[(0, 1), (1, 2)], 3, true), vec![vec![0, 1, 1]]);
        assert_eq!(rows(&[(0, 4)], 3, true), vec![vec![0, 0, 0], vec![0, 0, 0]]);
    }
}
