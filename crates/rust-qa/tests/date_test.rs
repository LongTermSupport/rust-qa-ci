//! Calendar dates for record review deadlines: parsing, arithmetic and TOML input.
#![cfg(test)]

use rust_qa::date::Date;

fn date(text: &str) -> Date {
    Date::parse(text).unwrap()
}

#[test]
fn parses_and_prints_iso_dates() {
    assert_eq!(date("2026-10-07").to_string(), "2026-10-07");
    assert_eq!(date("2024-02-29").to_string(), "2024-02-29");
}

#[test]
fn rejects_impossible_or_malformed_dates() {
    for text in [
        "2026-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-04-31",
        "2026-4-1",
        "26-04-01",
        "2026/04/01",
        "2026-04-01T00:00:00",
        "",
    ] {
        assert!(Date::parse(text).is_none(), "{text} was accepted");
    }
}

#[test]
fn day_arithmetic_crosses_months_years_and_leap_days() {
    assert_eq!(date("2026-12-31").add_days(1), date("2027-01-01"));
    assert_eq!(date("2024-02-28").add_days(1), date("2024-02-29"));
    assert_eq!(date("2024-03-01").add_days(-1), date("2024-02-29"));
    assert_eq!(date("1970-01-01").add_days(0), date("1970-01-01"));
    assert_eq!(date("2026-01-01").days_until(date("2026-12-31")), 364);
    assert_eq!(date("2026-12-31").days_until(date("2026-01-01")), -364);
}

#[test]
fn dates_order_chronologically() {
    assert!(date("2026-01-31") < date("2026-02-01"));
    assert!(date("1999-12-31") < date("2000-01-01"));
}

#[test]
fn today_is_a_plausible_utc_date() {
    let today = Date::today();
    assert!(today > date("2025-01-01"));
    assert!(today < date("2200-01-01"));
}

#[derive(serde::Deserialize)]
struct Holder {
    when: Date,
}

#[test]
fn deserialises_from_a_toml_date_or_a_string() {
    let bare: Holder = toml::from_str("when = 2026-10-07\n").unwrap();
    assert_eq!(bare.when, date("2026-10-07"));
    let quoted: Holder = toml::from_str("when = \"2026-10-07\"\n").unwrap();
    assert_eq!(quoted.when, date("2026-10-07"));
}

#[test]
fn rejects_toml_datetimes_with_a_time_or_offset() {
    assert!(toml::from_str::<Holder>("when = 2026-10-07T10:00:00\n").is_err());
    assert!(toml::from_str::<Holder>("when = 2026-10-07T10:00:00Z\n").is_err());
    assert!(toml::from_str::<Holder>("when = \"soon\"\n").is_err());
}
