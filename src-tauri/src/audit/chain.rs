//! Per-device SHA-256 hash chain over audit rows (ARCHITECTURE §3.6).
//!
//! The hashed bytes are built explicitly, never from serde's map ordering (a transitive
//! `preserve_order` feature could silently change it): a JSON array of the fields in a fixed
//! order, with `metadata` serialised with recursively sorted object keys.

use serde_json::Value;
use sha2::{Digest, Sha256};

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The fields covered by the hash, in their fixed order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainFields<'a> {
    pub id: &'a str,
    pub store_id: &'a str,
    pub device_id: &'a str,
    pub device_seq: i64,
    pub staff_id: Option<&'a str>,
    pub approved_by: Option<&'a str>,
    pub action: &'a str,
    pub entity_type: Option<&'a str>,
    pub entity_id: Option<&'a str>,
    /// Canonical JSON text (see [`canonical_json`]).
    pub metadata: &'a str,
    pub business_date: &'a str,
    pub created_at: &'a str,
}

/// Deterministic JSON: object keys sorted (byte order), no whitespace.
pub fn canonical_json(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            out.push_str(&serde_json::to_string(v).expect("scalar serialises"));
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).expect("key serialises"));
                out.push(':');
                write_canonical(&map[k], out);
            }
            out.push('}');
        }
    }
}

fn opt(s: Option<&str>) -> Value {
    s.map(|x| Value::String(x.to_owned())).unwrap_or(Value::Null)
}

/// `hex(SHA256(prev_hash ‖ canonical_fields))`.
pub fn compute_hash(prev_hash: &str, f: &ChainFields<'_>) -> String {
    let metadata: Value = serde_json::from_str(f.metadata).unwrap_or(Value::String(f.metadata.to_owned()));
    let fields = Value::Array(vec![
        Value::String(f.id.to_owned()),
        Value::String(f.store_id.to_owned()),
        Value::String(f.device_id.to_owned()),
        Value::from(f.device_seq),
        opt(f.staff_id),
        opt(f.approved_by),
        Value::String(f.action.to_owned()),
        opt(f.entity_type),
        opt(f.entity_id),
        metadata,
        Value::String(f.business_date.to_owned()),
        Value::String(f.created_at.to_owned()),
    ]);
    let mut h = Sha256::new();
    h.update(prev_hash.as_bytes());
    h.update(canonical_json(&fields).as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample<'a>(metadata: &'a str) -> ChainFields<'a> {
        ChainFields {
            id: "0193f000-0000-7000-8000-000000000001",
            store_id: "store-1",
            device_id: "device-1",
            device_seq: 1,
            staff_id: Some("staff-1"),
            approved_by: None,
            action: "AUTH_LOGIN",
            entity_type: Some("staff"),
            entity_id: Some("staff-1"),
            metadata,
            business_date: "2026-11-08",
            created_at: "2026-11-08T09:30:00.000Z",
        }
    }

    #[test]
    fn canonical_sorts_keys_recursively() {
        let a = json!({"b": 1, "a": {"y": [1, {"d": 2, "c": 3}], "x": null}});
        assert_eq!(canonical_json(&a), r#"{"a":{"x":null,"y":[1,{"c":3,"d":2}]},"b":1}"#);
    }

    #[test]
    fn hash_ignores_metadata_formatting_but_not_content() {
        let h1 = compute_hash(GENESIS_HASH, &sample(r#"{"b":1,"a":2}"#));
        let h2 = compute_hash(GENESIS_HASH, &sample("{ \"a\": 2,\n \"b\": 1 }"));
        let h3 = compute_hash(GENESIS_HASH, &sample(r#"{"a":2,"b":3}"#));
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn golden_hash() {
        // Locks the canonical encoding. If this changes, every stored chain breaks — don't.
        let h = compute_hash(GENESIS_HASH, &sample(r#"{"method":"pin"}"#));
        assert_eq!(h.len(), 64);
        let mut sha = Sha256::new();
        sha.update(GENESIS_HASH.as_bytes());
        sha.update(
            br#"["0193f000-0000-7000-8000-000000000001","store-1","device-1",1,"staff-1",null,"AUTH_LOGIN","staff","staff-1",{"method":"pin"},"2026-11-08","2026-11-08T09:30:00.000Z"]"#,
        );
        assert_eq!(h, hex::encode(sha.finalize()));
    }

    #[test]
    fn chain_depends_on_prev_hash() {
        let a = compute_hash(GENESIS_HASH, &sample("{}"));
        let b = compute_hash(&a, &sample("{}"));
        assert_ne!(a, b);
    }
}
