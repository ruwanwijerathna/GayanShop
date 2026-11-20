//! Generic ESC/POS (doc §7.2, §7.3): a byte builder for 58/80 mm thermal printers, plus the renderers for
//! each print-job kind (`render`). Text is English/Latin (receipts stay English, doc §1.8); anything the
//! printer's code page can't show becomes `?`, never mojibake.

pub mod render;

use crate::error::{AppError, AppResult};

use serde::{Deserialize, Serialize};
use specta::Type;

const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;
const DLE: u8 = 0x10;
const EOT: u8 = 0x04;

/// `DLE EOT 2`: offline cause (cover open, error).
pub const STATUS_OFFLINE: [u8; 3] = [DLE, EOT, 2];
/// `DLE EOT 4`: roll paper sensor.
pub const STATUS_PAPER: [u8; 3] = [DLE, EOT, 4];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaperWidth {
    /// 58 mm, 32 columns (Font A).
    Mm58,
    /// 80 mm, 48 columns (Font A).
    #[default]
    Mm80,
}

impl PaperWidth {
    pub fn columns(self) -> usize {
        match self {
            PaperWidth::Mm58 => 32,
            PaperWidth::Mm80 => 48,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodePage {
    #[default]
    Pc437,
    Wpc1252,
}

impl CodePage {
    /// `ESC t n` table number.
    fn table(self) -> u8 {
        match self {
            CodePage::Pc437 => 0,
            CodePage::Wpc1252 => 16,
        }
    }

    fn encode(self, c: char) -> u8 {
        match (self, u32::from(c)) {
            (_, 0x20..=0x7E) => c as u8,
            (CodePage::Wpc1252, 0xA0..=0xFF) => u8::try_from(u32::from(c)).unwrap_or(b'?'),
            (CodePage::Pc437, _) => match c {
                'Ç' => 0x80,
                'ü' => 0x81,
                'é' => 0x82,
                'â' => 0x83,
                'ä' => 0x84,
                'à' => 0x85,
                'ç' => 0x87,
                'ê' => 0x88,
                'è' => 0x8A,
                'ï' => 0x8B,
                'ô' => 0x93,
                'ö' => 0x94,
                'û' => 0x96,
                'ù' => 0x97,
                'Ü' => 0x9A,
                '£' => 0x9C,
                'ñ' => 0xA4,
                'Ñ' => 0xA5,
                _ => b'?',
            },
            _ => b'?',
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CutMode {
    Full,
    #[default]
    Partial,
    None,
}

/// Which drawer pin the kick pulses (`ESC p m`): pin 2 by default, pin 5 for drawers wired that way.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DrawerPin {
    #[default]
    Pin2,
    Pin5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// How a receipt is laid out on this counter's printer (part of the printer profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub width: PaperWidth,
    pub codepage: CodePage,
    pub cut: CutMode,
    /// Lines fed before the cut, so the last line clears the cutter.
    pub feed_lines: u8,
    /// `GS ( k` QR; `false` falls back to a Code128 barcode of the same data.
    pub qr: bool,
    pub drawer_pin: DrawerPin,
    /// `false`: this counter has no cash drawer on the printer, so nothing sends the drawer kick.
    pub open_drawer: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            width: PaperWidth::Mm80,
            codepage: CodePage::Pc437,
            cut: CutMode::Partial,
            feed_lines: 4,
            qr: true,
            drawer_pin: DrawerPin::Pin2,
            open_drawer: true,
        }
    }
}

/// Widest header image, in dots: the 80 mm printer's printable width (576 dots).
pub const LOGO_MAX_WIDTH: u16 = 576;
/// Tallest header image, in dots: up to 480 dots (60 mm).
pub const LOGO_MAX_HEIGHT: u16 = 480;

/// The receipt's header image (Settings → Receipt printer): a 1-bit bitmap, `width / 8` bytes per row, most
/// significant bit first, `1` = black. The webview converts the picture; Rust checks it before it is stored.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct Logo {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

impl Logo {
    pub fn validate(&self) -> AppResult<()> {
        let bad = |why: &str| Err(AppError::validation("LOGO_INVALID", why.to_owned()));
        if self.width == 0 || self.height == 0 {
            return bad("empty");
        }
        if !self.width.is_multiple_of(8) || self.width > LOGO_MAX_WIDTH || self.height > LOGO_MAX_HEIGHT {
            return bad("size");
        }
        if self.data.len() != usize::from(self.width / 8) * usize::from(self.height) {
            return bad("data");
        }
        Ok(())
    }
}

/// The drawer kick on its own: `ESC p m t1 t2` = `1B 70 00 19 FA` (pin 2, 50 ms on, 500 ms off).
pub fn drawer_kick(pin: DrawerPin) -> Vec<u8> {
    let m = match pin {
        DrawerPin::Pin2 => 0,
        DrawerPin::Pin5 => 1,
    };
    vec![ESC, b'p', m, 0x19, 0xFA]
}

/// Append-only ESC/POS document.
pub struct EscPos {
    buf: Vec<u8>,
    layout: Layout,
}

impl EscPos {
    /// `ESC @` (initialise) and `ESC t n` (code page).
    pub fn new(layout: Layout) -> Self {
        let mut buf = vec![ESC, b'@', ESC, b't', layout.codepage.table()];
        buf.reserve(2048);
        EscPos { buf, layout }
    }

    pub fn columns(&self) -> usize {
        self.layout.width.columns()
    }

    pub fn align(&mut self, a: Align) -> &mut Self {
        let n = match a {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        };
        self.buf.extend_from_slice(&[ESC, b'a', n]);
        self
    }

    pub fn bold(&mut self, on: bool) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, b'E', u8::from(on)]);
        self
    }

    /// `GS ! n`: double width and height, or normal.
    pub fn double(&mut self, on: bool) -> &mut Self {
        self.buf.extend_from_slice(&[GS, b'!', if on { 0x11 } else { 0x00 }]);
        self
    }

    pub fn text(&mut self, s: &str) -> &mut Self {
        let cp = self.layout.codepage;
        self.buf.extend(s.chars().map(|c| if c == '\n' { b'\n' } else { cp.encode(c) }));
        self
    }

    pub fn line(&mut self, s: &str) -> &mut Self {
        self.text(s).text("\n")
    }

    /// Word-wrapped text at the full width (`cols` columns, or half that in double size).
    pub fn wrapped(&mut self, s: &str, cols: usize) -> &mut Self {
        for l in wrap(s, cols) {
            self.line(&l);
        }
        self
    }

    /// `left ......... right` on one line; a long left side wraps above the right-aligned value.
    pub fn pair(&mut self, left: &str, right: &str) -> &mut Self {
        let cols = self.columns();
        let (l, r) = (left.chars().count(), right.chars().count());
        if l + r < cols {
            let pad = " ".repeat(cols - l - r);
            self.line(&format!("{left}{pad}{right}"))
        } else {
            self.wrapped(left, cols);
            let pad = " ".repeat(cols.saturating_sub(r));
            self.line(&format!("{pad}{right}"))
        }
    }

    pub fn rule(&mut self, c: char) -> &mut Self {
        let s: String = std::iter::repeat_n(c, self.columns()).collect();
        self.line(&s)
    }

    /// `ESC d n`: print and feed n lines.
    pub fn feed(&mut self, n: u8) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, b'd', n]);
        self
    }

    /// QR (model 2, module 6, error level M) via `GS ( k`, or a Code128 barcode when the profile says the
    /// printer has no QR support.
    pub fn qr(&mut self, data: &str) -> &mut Self {
        let bytes: Vec<u8> = data.chars().map(|c| self.layout.codepage.encode(c)).collect();
        if !self.layout.qr {
            return self.code128(&bytes);
        }
        let store_len = bytes.len() + 3;
        let (pl, ph) = (u8::try_from(store_len % 256).unwrap_or(0), u8::try_from(store_len / 256).unwrap_or(0));
        self.buf.extend_from_slice(&[GS, b'(', b'k', 4, 0, 49, 65, 50, 0]); // model 2
        self.buf.extend_from_slice(&[GS, b'(', b'k', 3, 0, 49, 67, 6]); // module size
        self.buf.extend_from_slice(&[GS, b'(', b'k', 3, 0, 49, 69, 49]); // error correction M
        self.buf.extend_from_slice(&[GS, b'(', b'k', pl, ph, 49, 80, 48]); // store
        self.buf.extend_from_slice(&bytes);
        self.buf.extend_from_slice(&[GS, b'(', b'k', 3, 0, 49, 81, 48]); // print
        self.text("\n")
    }

    /// `GS v 0`: the header image as a raster bit image, centred, then a blank line under it.
    pub fn logo(&mut self, logo: &Logo) -> &mut Self {
        let bytes_per_row = logo.width / 8;
        let [xl, xh] = bytes_per_row.to_le_bytes();
        let [yl, yh] = logo.height.to_le_bytes();
        self.align(Align::Center);
        self.buf.extend_from_slice(&[GS, b'v', b'0', 0, xl, xh, yl, yh]);
        self.buf.extend_from_slice(&logo.data);
        self.text("\n")
    }

    /// Code128 (code set B) with HRI below: `GS h`, `GS w`, `GS H`, `GS k 73`.
    fn code128(&mut self, data: &[u8]) -> &mut Self {
        let body: Vec<u8> = [b"{B".as_slice(), data].concat();
        let n = u8::try_from(body.len()).unwrap_or(u8::MAX);
        self.buf.extend_from_slice(&[GS, b'h', 80, GS, b'w', 2, GS, b'H', 2, GS, b'k', 73, n]);
        self.buf.extend_from_slice(&body[..usize::from(n)]);
        self.text("\n")
    }

    pub fn kick(&mut self) -> &mut Self {
        self.buf.extend(drawer_kick(self.layout.drawer_pin));
        self
    }

    /// Feed so the last line clears the cutter, then `GS V` (full `0` / partial `1`).
    pub fn cut(&mut self) -> &mut Self {
        self.feed(self.layout.feed_lines);
        match self.layout.cut {
            CutMode::Full => self.buf.extend_from_slice(&[GS, b'V', 0]),
            CutMode::Partial => self.buf.extend_from_slice(&[GS, b'V', 1]),
            CutMode::None => {}
        }
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Greedy word wrap; words longer than a line are split.
pub fn wrap(s: &str, cols: usize) -> Vec<String> {
    let cols = cols.max(1);
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > cols {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(word.drain(..cols).collect());
        }
        let word: String = word.into_iter().collect();
        let need = if cur.is_empty() { word.chars().count() } else { cur.chars().count() + 1 + word.chars().count() };
        if need > cols && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn kick_is_the_documented_pulse() {
        assert_eq!(drawer_kick(DrawerPin::Pin2), vec![0x1B, 0x70, 0x00, 0x19, 0xFA]);
        assert_eq!(drawer_kick(DrawerPin::Pin5), vec![0x1B, 0x70, 0x01, 0x19, 0xFA]);
    }

    #[test]
    fn init_codepage_cut_and_qr() {
        let mut p = EscPos::new(Layout { codepage: CodePage::Wpc1252, cut: CutMode::Full, ..Layout::default() });
        p.qr("abc").cut();
        let b = p.finish();
        assert_eq!(&b[..5], &[0x1B, b'@', 0x1B, b't', 16]);
        assert!(contains(&b, &[0x1D, b'(', b'k', 6, 0, 49, 80, 48, b'a', b'b', b'c']));
        assert!(contains(&b, &[0x1D, b'(', b'k', 3, 0, 49, 81, 48]));
        assert!(b.ends_with(&[0x1B, b'd', 4, 0x1D, b'V', 0]));
    }

    #[test]
    fn code128_fallback_without_qr() {
        let mut p = EscPos::new(Layout { qr: false, cut: CutMode::None, ..Layout::default() });
        p.qr("T01-1");
        let b = p.finish();
        assert!(contains(&b, &[0x1D, b'k', 73, 7, b'{', b'B', b'T', b'0', b'1', b'-', b'1']));
        assert!(!contains(&b, &[0x1D, b'(', b'k']));
    }

    #[test]
    fn unprintable_text_becomes_question_marks() {
        let mut p = EscPos::new(Layout::default());
        p.text("é බ£");
        assert!(p.finish().ends_with(&[0x82, b' ', b'?', 0x9C]));
        let mut w = EscPos::new(Layout { codepage: CodePage::Wpc1252, ..Layout::default() });
        w.text("é");
        assert!(w.finish().ends_with(&[0xE9]));
    }

    #[test]
    fn pairs_and_wrapping_fit_the_paper() {
        let mut p = EscPos::new(Layout { width: PaperWidth::Mm58, ..Layout::default() });
        p.pair("Total", "Rs 10.00");
        let b = p.finish();
        let line = format!("Total{}Rs 10.00\n", " ".repeat(32 - 5 - 8));
        assert!(contains(&b, line.as_bytes()));
        assert_eq!(wrap("All sales are final. No refunds", 12), vec!["All sales", "are final.", "No refunds"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }
}
