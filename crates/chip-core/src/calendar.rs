//! Reporting intervals are half-open UTC instants resolved from civil dates in one IANA zone.
use crate::model::Filter;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub(crate) struct Range {
    pub as_of: DateTime<Utc>,
    pub timezone: String,
    pub period: String,
    pub week_start: &'static str,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub from_local: Option<String>,
    pub to_local: Option<String>,
    pub comparison: Option<&'static str>,
    pub previous_from: Option<DateTime<Utc>>,
    pub previous_to: Option<DateTime<Utc>>,
    pub prior_complete_to: Option<DateTime<Utc>>,
}
fn instant(local: NaiveDateTime, tz: Tz) -> Result<DateTime<Utc>> {
    // Earliest occurrence at a repeated wall time. Advance through a skipped wall time.
    for minutes in 0..=1440 {
        if let Some(t) = tz
            .from_local_datetime(&(local + Duration::minutes(minutes)))
            .earliest()
        {
            return Ok(t.with_timezone(&Utc));
        }
    }
    bail!("Unsupported timezone transition")
}
fn midnight(day: NaiveDate, tz: Tz) -> Result<DateTime<Utc>> {
    instant(day.and_hms_opt(0, 0, 0).unwrap(), tz)
}
pub(crate) fn bound(s: Option<&str>, tz: Tz, end: bool) -> Result<Option<DateTime<Utc>>> {
    s.map(|s| {
        if let Ok(day) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            midnight(
                if end {
                    day.succ_opt().context("Invalid date")?
                } else {
                    day
                },
                tz,
            )
        } else {
            Ok(DateTime::parse_from_rfc3339(s)
                .context("Use ISO timestamps or YYYY-MM-DD dates")?
                .with_timezone(&Utc))
        }
    })
    .transpose()
}
pub(crate) fn resolve(f: &Filter, tz: Tz, now: DateTime<Utc>) -> Result<Range> {
    let local = now.with_timezone(&tz);
    let day = local.date_naive();
    let period = f
        .period
        .as_deref()
        .unwrap_or(if f.from.is_some() || f.to.is_some() {
            "custom"
        } else {
            "all"
        });
    let (from, to, previous_from, previous_to, complete_to, comparison) = match period {
        "today" | "week" | "month" | "year" => {
            let start = match period {
                "today" => day,
                "week" => day - Duration::days(day.weekday().num_days_from_monday() as i64),
                "month" => day.with_day(1).unwrap(),
                _ => NaiveDate::from_ymd_opt(day.year(), 1, 1).unwrap(),
            };
            let prior = match period {
                "today" => start - Duration::days(1),
                "week" => start - Duration::days(7),
                "month" => (start - Duration::days(1)).with_day(1).unwrap(),
                _ => NaiveDate::from_ymd_opt(start.year() - 1, 1, 1).unwrap(),
            };
            let start_utc = midnight(start, tz)?;
            let prior_utc = midnight(prior, tz)?;
            // Match civil progress, not elapsed UTC hours (DST days differ in length).
            // Shorter previous months/years are capped at their complete end.
            let civil = prior.and_time(local.time()) + Duration::days((day - start).num_days());
            let prior_end = instant(civil, tz)?.min(start_utc);
            (
                Some(start_utc),
                Some(now),
                Some(prior_utc),
                Some(prior_end),
                Some(start_utc),
                Some("Prior calendar period to the same civil progress (capped at its end)"),
            )
        }
        "7" | "30" | "90" => {
            let span = Duration::days(period.parse::<i64>()?);
            (
                Some(now - span),
                Some(now),
                Some(now - span - span),
                Some(now - span),
                None,
                Some("Previous equal elapsed duration"),
            )
        }
        "all" | "custom" => {
            let from = bound(f.from.as_deref(), tz, false)?;
            let to = bound(f.to.as_deref(), tz, true)?.or_else(|| from.map(|_| now));
            let previous = from.map(|a| (a - (to.unwrap_or(now) - a), a));
            (
                from,
                to,
                previous.map(|p| p.0),
                previous.map(|p| p.1),
                None,
                previous.map(|_| "Previous equal elapsed duration"),
            )
        }
        _ => bail!("Unsupported reporting period"),
    };
    if from.zip(to).is_some_and(|(a, b)| {
        a > b || (a == b && !matches!(period, "today" | "week" | "month" | "year"))
    }) {
        bail!("Date range must end after it starts");
    }
    Ok(Range {
        as_of: now,
        timezone: tz.to_string(),
        period: period.into(),
        week_start: "Monday",
        from,
        to,
        from_local: from.map(|t| t.with_timezone(&tz).to_rfc3339()),
        to_local: to.map(|t| t.with_timezone(&tz).to_rfc3339()),
        comparison,
        previous_from,
        previous_to,
        prior_complete_to: complete_to,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn range(period: &str, timezone: &str, now: &str) -> Range {
        resolve(
            &Filter {
                period: Some(period.into()),
                ..Filter::default()
            },
            timezone.parse().unwrap(),
            DateTime::parse_from_rfc3339(now)
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap()
    }
    #[test]
    fn civil_rollovers_and_leap_day() {
        for (period, now, start) in [
            ("today", "2024-03-01T00:30:00Z", "2024-02-29T06:00:00Z"),
            ("month", "2024-03-01T06:30:00Z", "2024-03-01T06:00:00Z"),
            ("year", "2027-01-01T06:30:00Z", "2027-01-01T06:00:00Z"),
            ("week", "2027-01-04T06:30:00Z", "2027-01-04T06:00:00Z"),
        ] {
            let r = range(period, "America/Chicago", now);
            assert_eq!(
                r.from.unwrap(),
                DateTime::parse_from_rfc3339(start).unwrap()
            );
            assert!(r.previous_to.unwrap() <= r.prior_complete_to.unwrap());
        }
        let r = range("month", "America/Chicago", "2024-03-31T17:00:00Z");
        assert_eq!(r.previous_to, r.from); // February is already complete.
    }
    #[test]
    fn exact_rollover_is_an_empty_to_date_interval() {
        for (period, now) in [
            ("today", "2027-01-01T06:00:00Z"),
            ("month", "2027-01-01T06:00:00Z"),
            ("year", "2027-01-01T06:00:00Z"),
            ("week", "2027-01-04T06:00:00Z"),
        ] {
            let r = range(period, "America/Chicago", now);
            assert_eq!(r.from, r.to);
            assert_eq!(r.previous_from, r.previous_to);
        }
    }
    #[test]
    fn dst_and_non_us_zone() {
        for (now, start, previous_end) in [
            (
                "2026-03-08T17:00:00Z",
                "2026-03-08T06:00:00Z",
                "2026-03-07T18:00:00Z",
            ),
            (
                "2026-11-01T18:00:00Z",
                "2026-11-01T05:00:00Z",
                "2026-10-31T17:00:00Z",
            ),
        ] {
            let r = range("today", "America/Chicago", now);
            assert_eq!(
                r.from.unwrap(),
                DateTime::parse_from_rfc3339(start).unwrap()
            );
            assert_eq!(
                r.previous_to.unwrap(),
                DateTime::parse_from_rfc3339(previous_end).unwrap()
            );
        }
        let r = range("today", "Asia/Kathmandu", "2026-12-31T19:00:00Z");
        assert_eq!(r.from_local.unwrap(), "2027-01-01T00:00:00+05:45");
    }
}
