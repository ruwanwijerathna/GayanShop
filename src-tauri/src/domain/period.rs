//! Report periods (ARCHITECTURE §5.2): day, week (from the store's `week_start`), calendar month and year,
//! all time, and an inclusive custom range, each resolved to inclusive `business_date` bounds. Every period
//! has a previous equivalent for comparison, and trend charts bucket days by day, week or month.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::business_date::{StoreCalendar, format_date, parse_date};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Period {
    Day {
        date: String,
    },
    /// The store week (`week_start`) containing `any_date`.
    Week {
        any_date: String,
    },
    Month {
        year: i32,
        month: u32,
    },
    Year {
        year: i32,
    },
    AllTime,
    /// Inclusive `from..=to`.
    Custom {
        from: String,
        to: String,
    },
}

/// Inclusive business-date bounds; `None` on both sides = all time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DateRange {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PeriodError {
    #[error("invalid date")]
    InvalidDate,
    #[error("the range ends before it starts")]
    Inverted,
}

/// Trend-chart buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum Granularity {
    #[default]
    Day,
    Week,
    Month,
}

fn date(s: &str) -> Result<NaiveDate, PeriodError> {
    parse_date(s).ok_or(PeriodError::InvalidDate)
}

fn month_bounds(year: i32, month: u32) -> Result<(NaiveDate, NaiveDate), PeriodError> {
    let start = NaiveDate::from_ymd_opt(year, month, 1).ok_or(PeriodError::InvalidDate)?;
    let next =
        if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) };
    let next = next.ok_or(PeriodError::InvalidDate)?;
    Ok((start, next - Duration::days(1)))
}

fn year_bounds(year: i32) -> Result<(NaiveDate, NaiveDate), PeriodError> {
    let start = NaiveDate::from_ymd_opt(year, 1, 1).ok_or(PeriodError::InvalidDate)?;
    let end = NaiveDate::from_ymd_opt(year, 12, 31).ok_or(PeriodError::InvalidDate)?;
    Ok((start, end))
}

impl Period {
    /// Inclusive bounds, as dates (`None` = all time).
    pub fn bounds(&self, cal: &StoreCalendar) -> Result<Option<(NaiveDate, NaiveDate)>, PeriodError> {
        Ok(Some(match self {
            Period::Day { date: d } => {
                let d = date(d)?;
                (d, d)
            }
            Period::Week { any_date } => cal.week_of(date(any_date)?),
            Period::Month { year, month } => month_bounds(*year, *month)?,
            Period::Year { year } => year_bounds(*year)?,
            Period::AllTime => return Ok(None),
            Period::Custom { from, to } => {
                let (from, to) = (date(from)?, date(to)?);
                if to < from {
                    return Err(PeriodError::Inverted);
                }
                (from, to)
            }
        }))
    }

    pub fn resolve(&self, cal: &StoreCalendar) -> Result<DateRange, PeriodError> {
        Ok(match self.bounds(cal)? {
            Some((from, to)) => DateRange { from: Some(format_date(from)), to: Some(format_date(to)) },
            None => DateRange { from: None, to: None },
        })
    }

    /// The previous equivalent period (§5.2 comparison): the day, week, month or year before, or the custom
    /// range of the same length ending the day before it starts. All time has none.
    pub fn previous(&self, cal: &StoreCalendar) -> Result<Option<Period>, PeriodError> {
        Ok(Some(match self {
            Period::Day { date: d } => Period::Day { date: format_date(date(d)? - Duration::days(1)) },
            Period::Week { any_date } => {
                let (start, _) = cal.week_of(date(any_date)?);
                Period::Week { any_date: format_date(start - Duration::days(7)) }
            }
            Period::Month { year, month } => {
                month_bounds(*year, *month)?;
                if *month == 1 {
                    Period::Month { year: year - 1, month: 12 }
                } else {
                    Period::Month { year: *year, month: month - 1 }
                }
            }
            Period::Year { year } => {
                year_bounds(*year)?;
                Period::Year { year: year - 1 }
            }
            Period::AllTime => return Ok(None),
            Period::Custom { .. } => {
                let Some((from, to)) = self.bounds(cal)? else { return Ok(None) };
                let len = (to - from).num_days();
                let prev_to = from - Duration::days(1);
                Period::Custom { from: format_date(prev_to - Duration::days(len)), to: format_date(prev_to) }
            }
        }))
    }
}

/// `strftime('%w')` number (Sunday = 0) of the store's first weekday.
fn week_start_w(cal: &StoreCalendar) -> u32 {
    match cal.week_start {
        Weekday::Sun => 0,
        other => other.number_from_monday(),
    }
}

/// The bucket a business date falls in: the date itself, its week's first day, or `YYYY-MM`.
pub fn bucket_of(d: NaiveDate, g: Granularity, cal: &StoreCalendar) -> String {
    match g {
        Granularity::Day => format_date(d),
        Granularity::Week => format_date(cal.week_of(d).0),
        Granularity::Month => format!("{:04}-{:02}", d.year(), d.month()),
    }
}

/// The SQL expression computing [`bucket_of`] for a `business_date` column. `col` is always one of our own
/// column names and the week offset is a small integer from the store calendar, never user input.
pub fn bucket_sql(col: &str, g: Granularity, cal: &StoreCalendar) -> String {
    match g {
        Granularity::Day => col.to_owned(),
        Granularity::Week => format!(
            "date({col}, '-' || ((CAST(strftime('%w', {col}) AS INTEGER) - {} + 7) % 7) || ' days')",
            week_start_w(cal)
        ),
        Granularity::Month => format!("substr({col}, 1, 7)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal() -> StoreCalendar {
        StoreCalendar::colombo_default()
    }

    fn range(p: Period) -> (Option<String>, Option<String>) {
        let r = p.resolve(&cal()).unwrap();
        (r.from, r.to)
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_owned())
    }

    #[test]
    fn every_period_resolves_to_inclusive_bounds() {
        assert_eq!(range(Period::Day { date: "2026-11-08".into() }), (s("2026-11-08"), s("2026-11-08")));
        // 2026-11-08 is a Sunday; the Monday week runs 2–8 November.
        assert_eq!(range(Period::Week { any_date: "2026-11-08".into() }), (s("2026-11-02"), s("2026-11-08")));
        assert_eq!(range(Period::Month { year: 2026, month: 2 }), (s("2026-02-01"), s("2026-02-28")));
        assert_eq!(range(Period::Month { year: 2028, month: 2 }), (s("2028-02-01"), s("2028-02-29")));
        assert_eq!(range(Period::Month { year: 2026, month: 12 }), (s("2026-12-01"), s("2026-12-31")));
        assert_eq!(range(Period::Year { year: 2026 }), (s("2026-01-01"), s("2026-12-31")));
        assert_eq!(range(Period::AllTime), (None, None));
        assert_eq!(
            range(Period::Custom { from: "2026-10-30".into(), to: "2026-11-02".into() }),
            (s("2026-10-30"), s("2026-11-02"))
        );
    }

    #[test]
    fn invalid_periods_are_refused() {
        let c = cal();
        assert_eq!(Period::Day { date: "2026-13-01".into() }.resolve(&c), Err(PeriodError::InvalidDate));
        assert_eq!(Period::Month { year: 2026, month: 0 }.resolve(&c), Err(PeriodError::InvalidDate));
        assert_eq!(
            Period::Custom { from: "2026-11-02".into(), to: "2026-11-01".into() }.resolve(&c),
            Err(PeriodError::Inverted)
        );
    }

    #[test]
    fn previous_periods_are_the_equivalent_one_before() {
        let c = cal();
        let prev = |p: Period| p.previous(&c).unwrap();
        assert_eq!(prev(Period::Day { date: "2026-03-01".into() }), Some(Period::Day { date: "2026-02-28".into() }));
        assert_eq!(
            prev(Period::Week { any_date: "2026-11-05".into() }).unwrap().resolve(&c).unwrap().from,
            s("2026-10-26")
        );
        assert_eq!(prev(Period::Month { year: 2026, month: 1 }), Some(Period::Month { year: 2025, month: 12 }));
        assert_eq!(prev(Period::Year { year: 2026 }), Some(Period::Year { year: 2025 }));
        assert_eq!(prev(Period::AllTime), None);
        assert_eq!(
            prev(Period::Custom { from: "2026-11-05".into(), to: "2026-11-07".into() }),
            Some(Period::Custom { from: "2026-11-02".into(), to: "2026-11-04".into() })
        );
    }

    #[test]
    fn buckets_follow_the_store_week() {
        let c = cal();
        let d = parse_date("2026-11-08").unwrap();
        assert_eq!(bucket_of(d, Granularity::Day, &c), "2026-11-08");
        assert_eq!(bucket_of(d, Granularity::Week, &c), "2026-11-02");
        assert_eq!(bucket_of(d, Granularity::Month, &c), "2026-11");
        let sunday = StoreCalendar { week_start: Weekday::Sun, ..c };
        assert_eq!(bucket_of(d, Granularity::Week, &sunday), "2026-11-08");
        assert!(bucket_sql("o.business_date", Granularity::Week, &sunday).contains("- 0 + 7"));
        assert!(bucket_sql("o.business_date", Granularity::Week, &cal()).contains("- 1 + 7"));
    }
}
