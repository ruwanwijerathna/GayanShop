//! Business date: the store-local calendar day after the day cutoff (ARCHITECTURE §1.7).

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc, Weekday};
use chrono_tz::Tz;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreCalendar {
    pub timezone: Tz,
    pub day_cutoff: NaiveTime,
    pub week_start: Weekday,
}

impl StoreCalendar {
    /// Owner-confirmed defaults: Asia/Colombo, 04:00 cutoff, weeks start Monday.
    pub fn colombo_default() -> Self {
        Self {
            timezone: chrono_tz::Asia::Colombo,
            day_cutoff: NaiveTime::from_hms_opt(4, 0, 0).expect("valid time"),
            week_start: Weekday::Mon,
        }
    }

    pub fn parse(timezone: &str, day_cutoff: &str, week_start: &str) -> Option<Self> {
        let timezone: Tz = timezone.parse().ok()?;
        let day_cutoff = NaiveTime::parse_from_str(day_cutoff, "%H:%M").ok()?;
        let week_start = match week_start.to_ascii_lowercase().as_str() {
            "monday" => Weekday::Mon,
            "sunday" => Weekday::Sun,
            "saturday" => Weekday::Sat,
            _ => return None,
        };
        Some(Self { timezone, day_cutoff, week_start })
    }

    /// The business date an instant belongs to.
    pub fn business_date(&self, at: DateTime<Utc>) -> NaiveDate {
        let local = at.with_timezone(&self.timezone);
        if local.time() < self.day_cutoff {
            local.date_naive() - Duration::days(1)
        } else {
            local.date_naive()
        }
    }

    /// Inclusive start/end business dates of the week containing `date`.
    pub fn week_of(&self, date: NaiveDate) -> (NaiveDate, NaiveDate) {
        let offset = (7 + date.weekday().num_days_from_monday() as i64
            - self.week_start.num_days_from_monday() as i64)
            % 7;
        let start = date - Duration::days(offset);
        (start, start + Duration::days(6))
    }
}

pub fn format_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn colombo(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> DateTime<Utc> {
        chrono_tz::Asia::Colombo.with_ymd_and_hms(y, m, d, hh, mm, 0).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn sale_after_midnight_belongs_to_previous_day() {
        let cal = StoreCalendar::colombo_default();
        assert_eq!(format_date(cal.business_date(colombo(2026, 11, 9, 1, 30))), "2026-11-08");
        assert_eq!(format_date(cal.business_date(colombo(2026, 11, 9, 3, 59))), "2026-11-08");
        assert_eq!(format_date(cal.business_date(colombo(2026, 11, 9, 4, 0))), "2026-11-09");
        assert_eq!(format_date(cal.business_date(colombo(2026, 11, 8, 23, 59))), "2026-11-08");
    }

    #[test]
    fn utc_instant_is_converted_to_colombo() {
        // 2026-11-08 20:00 UTC = 2026-11-09 01:30 +05:30 ⇒ still business day 8 Nov.
        let cal = StoreCalendar::colombo_default();
        let at = Utc.with_ymd_and_hms(2026, 11, 8, 20, 0, 0).unwrap();
        assert_eq!(format_date(cal.business_date(at)), "2026-11-08");
    }

    #[test]
    fn monday_week() {
        let cal = StoreCalendar::colombo_default();
        let (s, e) = cal.week_of(parse_date("2026-11-08").unwrap()); // a Sunday
        assert_eq!((format_date(s).as_str(), format_date(e).as_str()), ("2026-11-02", "2026-11-08"));
        let (s, _) = cal.week_of(parse_date("2026-11-09").unwrap()); // Monday
        assert_eq!(format_date(s), "2026-11-09");
    }

    #[test]
    fn parses_settings() {
        assert_eq!(StoreCalendar::parse("Asia/Colombo", "04:00", "monday"), Some(StoreCalendar::colombo_default()));
        assert!(StoreCalendar::parse("Mars/Base", "04:00", "monday").is_none());
    }
}
