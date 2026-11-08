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
    fn ids_are_time_ordered_v7() {
        let a = new_id();
        let b = new_id();
        assert_eq!(uuid::Uuid::parse_str(&a).unwrap().get_version_num(), 7);
        assert!(a < b);
    }
}
