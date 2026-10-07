//! UTC calendar dates for project-record decisions and review deadlines.
//!
//! Only whole days matter here, so a date is a day count since 1970-01-01 (proleptic Gregorian),
//! converted with Howard Hinnant's civil-date algorithms.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer};

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    days: i64,
}

const SECONDS_PER_DAY: u64 = 86_400;

impl Date {
    /// The date for a year, month (1-12) and day, or `None` when it does not exist.
    #[must_use]
    pub fn from_ymd(year: i64, month: u32, day: u32) -> Option<Self> {
        if !(1..=12).contains(&month) || day == 0 {
            return None;
        }
        let date = Self {
            days: days_from_civil(year, month, day),
        };
        (date.ymd() == (year, month, day)).then_some(date)
    }

    /// Parses `YYYY-MM-DD` exactly.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return None;
        }
        let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
        if !(digits(year) && digits(month) && digits(day)) {
            return None;
        }
        Self::from_ymd(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
    }

    /// Today in UTC, from the system clock.
    #[must_use]
    pub fn today() -> Self {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Self {
            days: i64::try_from(seconds / SECONDS_PER_DAY).unwrap_or(i64::MAX),
        }
    }

    /// The date `days` later (earlier when negative).
    #[must_use]
    pub fn add_days(self, days: i64) -> Self {
        Self {
            days: self.days.saturating_add(days),
        }
    }

    /// Days from `self` to `other` (negative when `other` is earlier).
    #[must_use]
    pub fn days_until(self, other: Self) -> i64 {
        other.days.saturating_sub(self.days)
    }

    /// Year, month and day.
    #[must_use]
    pub fn ymd(self) -> (i64, u32, u32) {
        civil_from_days(self.days)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.ymd();
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// Accepts a TOML local date (`2026-10-07`) or a string in the same form; rejects times and
/// offsets, since a review deadline is a whole UTC day.
impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = toml::Value::deserialize(deserializer)?;
        let text = match &value {
            toml::Value::Datetime(datetime)
                if datetime.time.is_none() && datetime.offset.is_none() =>
            {
                datetime.to_string()
            }
            toml::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("expected a date as YYYY-MM-DD, found `{text}`"))
        })
    }
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}
