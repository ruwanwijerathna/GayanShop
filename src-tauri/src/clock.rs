//! Injectable clock so lockouts, approval TTLs and business dates are deterministic in tests.

use std::sync::Mutex;

use chrono::{DateTime, Duration, SecondsFormat, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that only moves when told to (tests and fault-injection drills).
pub struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    pub fn new(at: DateTime<Utc>) -> Self {
        Self(Mutex::new(at))
    }

    pub fn advance(&self, by: Duration) {
        // A poisoned lock still holds a valid timestamp; recover it rather than panic.
        let mut t = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *t += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// ISO-8601 UTC with millisecond precision, e.g. `2026-11-08T09:30:00.000Z`.
pub fn iso(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

/// Time-ordered UUIDv7 for every locally created row.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Namespace for deterministic ids (UUIDv5). Fixed forever: changing it would split converged rows.
const STABLE_NAMESPACE: uuid::Uuid = uuid::Uuid::from_u128(0x6a5f_0e2c_4b1d_4c8e_9d3a_1f20_7b44_c0de);

/// Deterministic UUIDv5 of `(kind, store_id, key)`, e.g. a customer from their phone (doc §3.8): two
/// offline counters registering the same key create the same row id, which then merges by LWW.
pub fn stable_id(kind: &str, store_id: &str, key: &str) -> String {
    uuid::Uuid::new_v5(&STABLE_NAMESPACE, format!("{kind}\u{1f}{store_id}\u{1f}{key}").as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn manual_clock_advances() {
        let c = ManualClock::new(Utc.with_ymd_and_hms(2026, 11, 8, 9, 0, 0).unwrap());
        c.advance(Duration::seconds(61));
        assert_eq!(iso(c.now()), "2026-11-08T09:01:01.000Z");
        assert_eq!(parse_iso("2026-11-08T09:01:01.000Z"), Some(c.now()));
    }

    #[test]
    fn stable_ids_are_deterministic_and_scoped() {
        let a = stable_id("customer", "s1", "+94771234567");
        assert_eq!(a, stable_id("customer", "s1", "+94771234567"));
        assert_ne!(a, stable_id("customer", "s2", "+94771234567"));
        assert_ne!(a, stable_id("quick_sale", "s1", "+94771234567"));
        assert_eq!(uuid::Uuid::parse_str(&a).unwrap().get_version_num(), 5);
    }

    #[test]
    fn ids_are_time_ordered_v7() {
        let a = new_id();
        let b = new_id();
        assert_eq!(uuid::Uuid::parse_str(&a).unwrap().get_version_num(), 7);
        assert!(a < b);
    }
}
