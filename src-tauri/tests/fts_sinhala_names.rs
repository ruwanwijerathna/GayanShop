//! Block 1.1 — 20 real Sinhala product names against both FTS5 tokenizers (doc §13.4, "Sinhala FTS is
//! unverified"). Result, recorded in the doc: unicode61 splits Sinhala at every vowel sign and virama, so it
//! indexes bare consonants and confuses different words; trigram matches exact substrings. The till's
//! Sinhala search therefore uses `products_fts_si` (trigram), with a LIKE fallback for 1–2 characters.

mod common;

use common::{META, exec, open};
use gayanshop_lib::db::repo::products;
use sqlx::SqlitePool;

/// (Sinhala display name, printable English name, a substring from inside a word).
const NAMES: [(&str, &str, &str); 20] = [
    ("සබන් කැටය", "Soap Bar", "බන්"),
    ("දත් බුරුසුව", "Toothbrush", "රුසු"),
    ("දත් බෙහෙත්", "Toothpaste", "හෙත්"),
    ("ප්ලාස්ටික් බාල්දිය", "Plastic Bucket", "ස්ටික්"),
    ("නිල් පාට බේසම", "Blue Basin", "බේසම"),
    ("තේ කෝප්පය", "Tea Cup", "ප්පය"),
    ("වීදුරු පිඟාන", "Glass Plate", "දුරු"),
    ("මේස හැන්ද", "Table Spoon", "හැන්ද"),
    ("ළමා පාවහන්", "Kids Shoes", "වහන්"),
    ("රබර් සෙරෙප්පු", "Rubber Slippers", "රෙප්"),
    ("කළු කුඩය", "Black Umbrella", "කුඩ"),
    ("පාසල් බෑගය", "School Bag", "සල්"),
    ("ළමා ඇඳුම් කට්ටලය", "Kids Clothing Set", "ඇඳුම්"),
    ("කපු තුවාය", "Cotton Towel", "තුවා"),
    ("ඇඳ ඇතිරිල්ල", "Bed Sheet", "තිරි"),
    ("ජල බෝතලය", "Water Bottle", "බෝතල"),
    ("කුස්සි පිහිය", "Kitchen Knife", "පිහි"),
    ("ඉටිපන්දම් පැකට්ටුව", "Candle Pack", "පන්දම්"),
    ("ගිනිපෙට්ටිය", "Matchbox", "පෙට්ටි"),
    ("ශ්‍රී ලංකා තේ කොළ", "Sri Lanka Tea Leaves", "ලංකා"),
];

/// Sinhala dependent vowel signs and the virama (al-lakuna): Unicode marks, not letters.
fn is_sinhala_mark(c: char) -> bool {
    matches!(c, '\u{0DCA}' | '\u{0DCF}'..='\u{0DDF}' | '\u{0DF2}' | '\u{0DF3}')
}

async fn seed(pool: &SqlitePool) {
    for (i, (si, en, _)) in NAMES.iter().enumerate() {
        exec(pool, &format!("INSERT INTO products (id, store_id, name, name_si, price, created_at, updated_at, updated_by_device) VALUES ('p{i:02}', 's', '{en}', '{si}', 10000, {META})")).await;
    }
    exec(pool, "CREATE VIRTUAL TABLE si_unicode61 USING fts5(id UNINDEXED, name_si, tokenize = 'unicode61')").await;
    exec(pool, "INSERT INTO si_unicode61 (id, name_si) SELECT id, name_si FROM products").await;
}

async fn ids(pool: &SqlitePool, sql: &str, q: &str) -> Vec<String> {
    sqlx::query_scalar(sql).bind(q).fetch_all(pool).await.unwrap()
}

#[tokio::test]
async fn unicode61_fragments_sinhala_words() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    exec(p, "CREATE VIRTUAL TABLE si_vocab USING fts5vocab(si_unicode61, 'row')").await;
    let terms: Vec<String> = sqlx::query_scalar("SELECT term FROM si_vocab").fetch_all(p).await.unwrap();
    assert!(!terms.is_empty());
    // Every vowel sign and virama was treated as a separator: no indexed term keeps one…
    assert!(terms.iter().all(|t| !t.chars().any(is_sinhala_mark)), "unexpected mark in {terms:?}");
    // …so no whole word survives as a term.
    for word in ["බාල්දිය", "කුඩය", "තුවාය", "බෝතලය"] {
        assert!(!terms.iter().any(|t| t == word), "{word} unexpectedly indexed whole");
    }
    // Consequence: words that differ only in their vowel signs collide under unicode61. "බෝල" (ball) and
    // "බිල" (bill) both match "බාල්දිය" (bucket); "කපා" (cut) matches "කපු" (cotton). Trigram matches none.
    for (query, wrongly) in [("බෝල", "p03"), ("බිල", "p03"), ("කපා", "p13")] {
        let q = format!("\"{query}\"");
        let u61 = ids(p, "SELECT id FROM si_unicode61 WHERE si_unicode61 MATCH ?1", &q).await;
        assert_eq!(u61, vec![wrongly], "unicode61 conflation for {query}");
        let tri = ids(p, "SELECT id FROM products_fts_si WHERE products_fts_si MATCH ?1", &q).await;
        assert!(tri.is_empty(), "trigram must not match {query}: {tri:?}");
    }
}

#[tokio::test]
async fn trigram_finds_every_name_by_an_inner_substring() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    for (i, (si, _, sub)) in NAMES.iter().enumerate() {
        assert!(si.contains(sub), "fixture: {sub} not inside {si}");
        assert!(sub.chars().count() >= 3, "fixture: trigram needs 3+ characters: {sub}");
        let id = format!("p{i:02}");
        let hits = ids(p, "SELECT id FROM products_fts_si WHERE products_fts_si MATCH ?1", &format!("\"{sub}\"")).await;
        assert!(hits.contains(&id), "trigram missed {si} for {sub}: {hits:?}");
        let whole = ids(p, "SELECT id FROM products_fts_si WHERE products_fts_si MATCH ?1", &format!("\"{si}\"")).await;
        assert_eq!(whole, vec![id.clone()], "full name {si}");
    }
}

#[tokio::test]
async fn till_search_uses_trigram_for_sinhala_and_like_for_short_queries() {
    let t = open().await;
    let p = t.pool();
    seed(p).await;
    for (i, (si, _, sub)) in NAMES.iter().enumerate() {
        let hits = products::search(p, "s", sub, 20).await.unwrap();
        assert!(hits.iter().any(|h| h.id == format!("p{i:02}")), "search({sub}) missed {si}");
        assert!(hits.iter().all(|h| h.name_si.as_deref().is_some_and(|n| n.contains(sub))), "false hit for {sub}");
    }
    // 1–2 character queries can't use trigram MATCH; the LIKE fallback still finds them.
    let short = products::search(p, "s", "ළමා", 20).await.unwrap();
    assert_eq!(short.len(), 2);
    let two = products::search(p, "s", "කු", 20).await.unwrap();
    let mut two_ids: Vec<&str> = two.iter().map(|h| h.id.as_str()).collect();
    two_ids.sort_unstable();
    assert_eq!(two_ids, vec!["p10", "p16"]);
    // The zero-width joiner in ශ්‍රී is part of the name; the same typed form finds it.
    assert_eq!(products::search(p, "s", "ශ්‍රී ලංකා", 20).await.unwrap()[0].id, "p19");
    // Inactive and deleted products never appear.
    exec(p, "UPDATE products SET is_active = 0 WHERE id = 'p03'").await;
    exec(p, &format!("UPDATE products SET deleted_at = '{}' WHERE id = 'p04'", common::NOW)).await;
    assert!(products::search(p, "s", "බාල්දිය", 20).await.unwrap().is_empty());
    assert!(products::search(p, "s", "බේසම", 20).await.unwrap().is_empty());
}
