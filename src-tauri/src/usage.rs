//! Parses the stdout of `claude -p --output-format json /usage` into a `UsageReading`.
//! The limits are text lines inside the JSON `result` string (see docs/spikes/usage-output.md).

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    pub used_pct: f64,
    /// None while the session is idle: Claude prints no `· resets …` part until you next use it.
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageReading {
    pub session: Limit,
    pub week: Limit,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UsageError {
    NoJson,
    MissingField(&'static str),
    BadTime(String),
}

pub fn parse_usage(raw: &str, fetched_at: DateTime<Utc>) -> Result<UsageReading, UsageError> {
    let (start, end) = (raw.find('{'), raw.rfind('}'));
    let json = match (start, end) {
        (Some(s), Some(e)) if s < e => &raw[s..=e],
        _ => return Err(UsageError::NoJson),
    };
    let value: serde_json::Value = serde_json::from_str(json).map_err(|_| UsageError::NoJson)?;
    let result = value
        .get("result")
        .and_then(|r| r.as_str())
        .ok_or(UsageError::MissingField("result"))?;

    let find = |prefix: &str| {
        result
            .split('\n')
            .find_map(|line| line.trim().strip_prefix(prefix))
    };
    let limit = |rest: Option<&str>, name| {
        parse_limit(rest.ok_or(UsageError::MissingField(name))?, fetched_at)
    };
    Ok(UsageReading {
        session: limit(find("Current session:"), "session")?,
        week: limit(find("Current week (all models):"), "week")?,
        fetched_at,
    })
}

/// `rest` is ` 46% used · resets Oct 28, 8pm (Asia/Kuala_Lumpur)`, or ` 0% used` when idle.
fn parse_limit(rest: &str, fetched_at: DateTime<Utc>) -> Result<Limit, UsageError> {
    let bad = |s: &str| UsageError::BadTime(s.to_string());
    let (pct, reset) = match rest.split_once('·') {
        Some((p, r)) => (p.trim(), Some(r.trim())),
        None => (rest.trim(), None),
    };
    let used_pct = pct
        .strip_suffix("% used")
        .and_then(|n| n.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite())
        .ok_or_else(|| bad(pct))?;
    let resets_at = match reset {
        None => None,
        Some(r) => {
            let r = r.strip_prefix("resets ").ok_or_else(|| bad(r))?;
            Some(parse_reset(r, fetched_at).ok_or_else(|| bad(r))?)
        }
    };
    Ok(Limit { used_pct, resets_at })
}

/// `Mon D, h[:mm]am|pm (IANA zone)`; the year is not in the text, so pick the earliest
/// one that puts the reset no more than a day before `fetched_at`.
fn parse_reset(text: &str, fetched_at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let (when, zone) = text.strip_suffix(')')?.split_once(" (")?;
    let tz: Tz = zone.trim().parse().ok()?;
    let (date, time) = when.split_once(',')?;
    let (mon, day) = date.trim().split_once(' ')?;
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter()
        .position(|m| *m == mon)? as u32
        + 1;
    let day: u32 = day.trim().parse().ok()?;

    let time = time.trim();
    let (clock, pm) = match (time.strip_suffix("am"), time.strip_suffix("pm")) {
        (Some(c), _) => (c, false),
        (_, Some(c)) => (c, true),
        _ => return None,
    };
    let (h, m) = clock.split_once(':').unwrap_or((clock, "0"));
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    if !(1..=12).contains(&h) || m > 59 {
        return None;
    }
    let hour = h % 12 + if pm { 12 } else { 0 };

    let fetched_year = fetched_at.year();
    (fetched_year - 1..=fetched_year + 1).find_map(|year| {
        let local = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, m, 0)?;
        let at = tz.from_local_datetime(&local).earliest()?.with_timezone(&Utc);
        (at >= fetched_at - Duration::days(1)).then_some(at)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde_json::json;

    const FIXTURE: &str = include_str!("../tests/fixtures/usage-wsl.txt");
    const SESSION_LINE: &str = "Current session: 46% used · resets Oct 28, 8pm (Asia/Kuala_Lumpur)";
    const WEEK_LINE: &str =
        "Current week (all models): 72% used · resets Oct 31, 10am (Asia/Kuala_Lumpur)";

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    fn t0() -> DateTime<Utc> {
        at("2026-10-28T18:20:00+08:00")
    }
    fn raw(result: &str) -> String {
        json!({ "is_error": false, "result": result }).to_string()
    }
    fn assert_fixture_values(r: &UsageReading) {
        assert_eq!(r.session.used_pct, 46.0);
        assert_eq!(r.session.resets_at, Some(at("2026-10-28T20:00:00+08:00")));
        assert_eq!(r.week.used_pct, 72.0);
        assert_eq!(r.week.resets_at, Some(at("2026-10-31T10:00:00+08:00")));
        assert_eq!(r.fetched_at, t0());
    }

    #[test]
    fn parses_wsl_fixture() {
        assert_fixture_values(&parse_usage(FIXTURE, t0()).unwrap());
    }

    #[test]
    fn parse_tolerates_noise() {
        let noisy = format!("wsl: banner line\n{FIXTURE}\ntrailing");
        assert_fixture_values(&parse_usage(&noisy, t0()).unwrap());
    }

    #[test]
    fn parses_minutes_in_reset() {
        let r = raw(&format!(
            "Current session: 46% used · resets Oct 28, 8:30pm (Asia/Kuala_Lumpur)\n{WEEK_LINE}"
        ));
        let got = parse_usage(&r, t0()).unwrap();
        assert_eq!(got.session.resets_at, Some(at("2026-10-28T20:30:00+08:00")));
    }

    #[test]
    fn infers_year_across_new_year() {
        let r = raw(
            "Current session: 10% used · resets Jan 2, 9am (UTC)\nCurrent week (all models): 20% used · resets Jan 5, 9am (UTC)",
        );
        let got = parse_usage(&r, at("2026-12-30T12:00:00Z")).unwrap();
        assert_eq!(got.session.resets_at, Some(at("2027-01-02T09:00:00Z")));
        assert_eq!(got.week.resets_at, Some(at("2027-01-05T09:00:00Z")));
    }

    #[test]
    fn missing_week_is_error() {
        let r = raw(SESSION_LINE);
        assert_eq!(
            parse_usage(&r, t0()),
            Err(UsageError::MissingField("week"))
        );
    }

    #[test]
    fn missing_session_is_error() {
        let r = raw(WEEK_LINE);
        assert_eq!(
            parse_usage(&r, t0()),
            Err(UsageError::MissingField("session"))
        );
    }

    #[test]
    fn missing_result_is_error() {
        assert_eq!(
            parse_usage("{\"is_error\":false}", t0()),
            Err(UsageError::MissingField("result"))
        );
    }

    #[test]
    fn bad_percentage_and_time_are_errors() {
        let bad_pct = raw(&format!(
            "Current session: lots% used · resets Oct 28, 8pm (UTC)\n{WEEK_LINE}"
        ));
        assert!(matches!(parse_usage(&bad_pct, t0()), Err(UsageError::BadTime(_))));
        let bad_time = raw(&format!(
            "Current session: 46% used · resets someday (UTC)\n{WEEK_LINE}"
        ));
        assert!(matches!(parse_usage(&bad_time, t0()), Err(UsageError::BadTime(_))));
    }

    #[test]
    fn session_without_reset_is_idle() {
        let r = raw(&format!("Current session: 0% used\n{WEEK_LINE}"));
        let got = parse_usage(&r, t0()).unwrap();
        assert_eq!(got.session, Limit { used_pct: 0.0, resets_at: None });
        assert!(got.week.resets_at.is_some());
    }

    #[test]
    fn garbled_reset_part_is_still_bad_time() {
        let r = raw(&format!("Current session: 0% used · soon\n{WEEK_LINE}"));
        assert!(matches!(parse_usage(&r, t0()), Err(UsageError::BadTime(_))));
    }

    #[test]
    fn empty_is_no_json() {
        assert_eq!(parse_usage("", t0()), Err(UsageError::NoJson));
    }

    #[test]
    fn ignores_sub_limit_week_line() {
        let r = raw(&format!(
            "{SESSION_LINE}\nCurrent week (Sonnet only): 10% used · resets Oct 31, 10am (Asia/Kuala_Lumpur)\n{WEEK_LINE}"
        ));
        assert_eq!(parse_usage(&r, t0()).unwrap().week.used_pct, 72.0);
    }
}
