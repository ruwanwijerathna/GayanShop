//! Quick-sale grouping key (ARCHITECTURE §3.2). Stored on each custom line at write time.

use unicode_normalization::UnicodeNormalization;

/// NFC, lowercase, trimmed, internal whitespace collapsed. The price is already stripped by
/// the till's parser before the name reaches Rust.
pub fn custom_key(name: &str) -> String {
    let nfc: String = name.nfc().collect();
    nfc.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises() {
        assert_eq!(custom_key("  Blue   Bucket 10L "), "blue bucket 10l");
        assert_eq!(custom_key("BLUE BUCKET 10L"), custom_key("blue bucket 10l"));
        // é composed vs decomposed
        assert_eq!(custom_key("Caf\u{e9}"), custom_key("Cafe\u{301}"));
    }
}
