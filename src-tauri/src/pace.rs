//! Pace tick position and projected run-out time (spec §5.3).

use chrono::{DateTime, Duration, Utc};

pub const SESSION_WINDOW: Duration = Duration::hours(5);
pub const WEEK_WINDOW: Duration = Duration::days(7);

/// Share (0-100) of `window` already passed, given when it resets.
pub fn elapsed_pct(window: Duration, now: DateTime<Utc>, resets_at: DateTime<Utc>) -> f64 {
    let passed = window - (resets_at - now);
    (passed.num_milliseconds() as f64 / window.num_milliseconds() as f64 * 100.0).clamp(0.0, 100.0)
}

/// When usage hits 100 % at the current rate, if that is before the window resets.
pub fn runs_out_at(
    used_pct: f64,
    window: Duration,
    now: DateTime<Utc>,
    resets_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let elapsed = elapsed_pct(window, now, resets_at);
    // No elapsed time means no rate to project from.
    if !used_pct.is_finite() || used_pct <= 0.0 || used_pct <= elapsed || elapsed <= 0.0 {
        return None;
    }
    let elapsed_ms = window.num_milliseconds() as f64 * elapsed / 100.0;
    let left_ms = ((100.0 - used_pct).max(0.0) * elapsed_ms / used_pct).round() as i64;
    let at = now + Duration::milliseconds(left_ms);
    (at < resets_at).then_some(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(d: u32, h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap()
    }

    #[test]
    fn session_elapsed_matches_mockup() {
        let now = at(28, 10, 0);
        let pct = elapsed_pct(SESSION_WINDOW, now, now + Duration::minutes(133));
        assert!((pct - 55.67).abs() < 0.01, "{pct}");
    }

    #[test]
    fn week_elapsed_matches_mockup() {
        // Wed 10:51 -> Fri 09:00 leaves 46 h 9 m of 168 h
        let pct = elapsed_pct(WEEK_WINDOW, at(28, 10, 51), at(30, 9, 0));
        assert!((pct - 72.5).abs() < 0.05, "{pct}");
    }

    #[test]
    fn elapsed_clamps_when_reset_passed() {
        let now = at(28, 10, 0);
        assert_eq!(elapsed_pct(SESSION_WINDOW, now, now - Duration::minutes(1)), 100.0);
        assert_eq!(elapsed_pct(SESSION_WINDOW, now, now + SESSION_WINDOW + Duration::minutes(1)), 0.0);
    }

    #[test]
    fn runs_out_when_ahead() {
        let now = at(28, 10, 0);
        let resets = now + Duration::minutes(150);
        let got = runs_out_at(80.0, SESSION_WINDOW, now, resets);
        assert_eq!(got, Some(now + Duration::seconds(37 * 60 + 30)));
    }

    #[test]
    fn runs_out_none_when_behind() {
        let now = at(28, 10, 0);
        let resets = now + Duration::minutes(133);
        assert_eq!(runs_out_at(42.0, SESSION_WINDOW, now, resets), None);
    }

    #[test]
    fn runs_out_none_after_reset() {
        // reset already passed and used > 100 % gets past the early exits; projection (now) is not before resets_at
        let now = at(28, 10, 0);
        assert_eq!(runs_out_at(120.0, SESSION_WINDOW, now, now - Duration::minutes(1)), None);
    }

    #[test]
    fn runs_out_none_when_no_time_elapsed() {
        let now = at(28, 10, 0);
        // window not started yet (resets_at beyond now + window), and exactly at start
        assert_eq!(runs_out_at(5.0, SESSION_WINDOW, now, now + SESSION_WINDOW + Duration::minutes(1)), None);
        assert_eq!(runs_out_at(5.0, SESSION_WINDOW, now, now + SESSION_WINDOW), None);
    }

    #[test]
    fn runs_out_none_when_not_finite() {
        let now = at(28, 10, 0);
        let resets = now + Duration::minutes(150);
        assert_eq!(runs_out_at(f64::NAN, SESSION_WINDOW, now, resets), None);
        assert_eq!(runs_out_at(f64::INFINITY, SESSION_WINDOW, now, resets), None);
    }

    #[test]
    fn runs_out_none_when_unused() {
        let now = at(28, 10, 0);
        assert_eq!(runs_out_at(0.0, SESSION_WINDOW, now, now + Duration::hours(1)), None);
    }
}
