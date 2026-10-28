//! Turns the latest reading and settings into the JSON the webview renders (spec §5.2-5.3).
//! All display times are local; pace maths runs in UTC.

use crate::alerts::{level, Level};
use crate::pace::{elapsed_pct, runs_out_at, SESSION_WINDOW, WEEK_WINDOW};
use crate::settings::Settings;
use crate::source::Source;
use crate::usage::{Limit, UsageReading};
use chrono::{DateTime, Duration, Local, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PillState {
    NoClaude,
    NoData,
    Normal,
    Alert,
    Full,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LimitView {
    pub value_text: String,
    pub used_pct: f64,
    pub pace_pct: f64,
    pub level: Level,
    pub reset_text: String,
    pub pace_text: String,
    pub runs_out_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewModel {
    pub pill: PillState,
    pub pill_text: Option<String>,
    pub session: LimitView,
    pub week: LimitView,
    pub updated_text: String,
    pub announce: Option<String>,
    pub refreshing: bool,
}

/// "now", "in 2h 13m" / "in 45m" (rounded up, within 24 h), or "Fri 09:00" beyond that.
fn when_text(now: DateTime<Local>, resets_at: DateTime<Utc>) -> String {
    let ahead = resets_at - now.with_timezone(&Utc);
    if ahead <= Duration::zero() {
        return "now".into();
    }
    if ahead > Duration::hours(24) {
        return resets_at.with_timezone(&Local).format("%a %H:%M").to_string();
    }
    let mins = (ahead.num_seconds() + 59) / 60;
    match (mins / 60, mins % 60) {
        (0, m) => format!("in {m}m"),
        (h, m) => format!("in {h}h {m}m"),
    }
}

pub fn reset_text(now: DateTime<Local>, resets_at: DateTime<Utc>) -> String {
    format!("Resets {}", when_text(now, resets_at))
}

fn pill_text(name: &str, now: DateTime<Local>, resets_at: DateTime<Utc>) -> String {
    match when_text(now, resets_at) {
        w if w == "now" || w.starts_with("in ") => format!("{name} full, back {w}"),
        w => format!("{name} full, back in {w}"),
    }
}

fn limit_view(limit: &Limit, threshold: u8, window: Duration, now: DateTime<Local>) -> LimitView {
    let now_utc = now.with_timezone(&Utc);
    let lvl = level(limit.used_pct, threshold);
    let pace = elapsed_pct(window, now_utc, limit.resets_at);
    let clamped = limit.used_pct.clamp(0.0, 100.0);
    // Only a Full limit may read 100 %; 99.5 rounds up but is still Alert.
    let shown = if lvl == Level::Full { clamped.round() } else { clamped.round().min(99.0) };
    let runs_out_text = match lvl {
        Level::Full => None,
        _ => runs_out_at(limit.used_pct, window, now_utc, limit.resets_at).map(|t| {
            format!("Runs out around {}", t.with_timezone(&Local).format("%H:%M"))
        }),
    };
    LimitView {
        value_text: format!("{}%", shown),
        used_pct: clamped,
        pace_pct: pace,
        level: lvl,
        reset_text: reset_text(now, limit.resets_at),
        pace_text: format!("{}% of time passed", pace.round()),
        runs_out_text,
    }
}

fn empty_limit() -> LimitView {
    LimitView {
        value_text: "–".into(),
        used_pct: 0.0,
        pace_pct: 0.0,
        level: Level::Normal,
        reset_text: String::new(),
        pace_text: String::new(),
        runs_out_text: None,
    }
}

pub fn build(
    reading: Option<&UsageReading>,
    last_ok: bool,
    source: &Source,
    settings: &Settings,
    now: DateTime<Local>,
) -> ViewModel {
    let (session, week) = match reading {
        Some(r) => (
            limit_view(&r.session, settings.session_threshold, SESSION_WINDOW, now),
            limit_view(&r.week, settings.week_threshold, WEEK_WINDOW, now),
        ),
        None => (empty_limit(), empty_limit()),
    };
    let (pill, pill_text, updated_text) = match (source, reading) {
        (Source::NotFound, _) => (
            PillState::NoClaude,
            None,
            "Install Claude Code to see your limits".to_string(),
        ),
        (_, None) => (PillState::NoData, None, "No reading yet".to_string()),
        (_, Some(r)) => {
            let at = r.fetched_at.with_timezone(&Local).format("%H:%M");
            let updated = if last_ok { format!("Updated {at}") } else { format!("As of {at}") };
            let full = if session.level == Level::Full {
                Some(pill_text("Session", now, r.session.resets_at))
            } else if week.level == Level::Full {
                Some(pill_text("Week", now, r.week.resets_at))
            } else {
                None
            };
            if full.is_some() {
                (PillState::Full, full, updated)
            } else if session.level == Level::Alert || week.level == Level::Alert {
                (PillState::Alert, None, updated)
            } else if !last_ok {
                (PillState::Stale, None, updated)
            } else {
                (PillState::Normal, None, updated)
            }
        }
    };
    ViewModel { pill, pill_text, session, week, updated_text, announce: None, refreshing: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    fn local(d: u32, h: u32, m: u32) -> DateTime<Local> {
        let naive = NaiveDate::from_ymd_opt(2026, 10, d).unwrap().and_hms_opt(h, m, 0).unwrap();
        Local.from_local_datetime(&naive).single().unwrap()
    }

    fn utc(t: DateTime<Local>) -> DateTime<Utc> {
        t.with_timezone(&Utc)
    }

    fn reading(s_used: f64, w_used: f64, now: DateTime<Local>) -> UsageReading {
        UsageReading {
            session: Limit { used_pct: s_used, resets_at: utc(now) + Duration::minutes(133) },
            week: Limit { used_pct: w_used, resets_at: utc(local(30, 9, 0)) },
            fetched_at: utc(local(28, 10, 51)),
        }
    }

    #[test]
    fn reset_text_session() {
        let now = local(28, 10, 0);
        assert_eq!(reset_text(now, utc(now) + Duration::minutes(133)), "Resets in 2h 13m");
        assert_eq!(reset_text(now, utc(now) + Duration::minutes(45)), "Resets in 45m");
    }

    #[test]
    fn reset_text_week() {
        assert_eq!(reset_text(local(28, 10, 51), utc(local(30, 9, 0))), "Resets Fri 09:00");
    }

    #[test]
    fn reset_text_past_is_now() {
        let now = local(28, 10, 0);
        assert_eq!(reset_text(now, utc(now) - Duration::minutes(1)), "Resets now");
    }

    #[test]
    fn minutes_round_up() {
        let now = local(28, 10, 0);
        assert_eq!(reset_text(now, utc(now) + Duration::seconds(30)), "Resets in 1m");
        assert_eq!(reset_text(now, utc(now) + Duration::seconds(2 * 3600 + 12 * 60 + 30)), "Resets in 2h 13m");
    }

    #[test]
    fn pace_text() {
        let now = local(28, 10, 0);
        let r = reading(10.0, 10.0, now);
        // 133 min left of 300 -> 55.67 % passed
        let vm = build(Some(&r), true, &Source::Windows, &Settings::default(), now);
        assert_eq!(vm.session.pace_text, "56% of time passed");
        assert_eq!(vm.session.value_text, "10%");
    }

    #[test]
    fn runs_out_text() {
        let now = local(28, 10, 0);
        let r = reading(80.0, 10.0, now);
        let vm = build(Some(&r), true, &Source::Windows, &Settings::default(), now);
        let t = runs_out_at(80.0, SESSION_WINDOW, utc(now), r.session.resets_at).unwrap();
        let want = format!("Runs out around {}", t.with_timezone(&Local).format("%H:%M"));
        assert_eq!(vm.session.runs_out_text, Some(want));
        // fixed example: the formatter itself
        let at = local(28, 15, 40).with_timezone(&Local).format("%H:%M").to_string();
        assert_eq!(format!("Runs out around {at}"), "Runs out around 15:40");
    }

    #[test]
    fn runs_out_hidden_when_full() {
        let now = local(28, 10, 0);
        let r = reading(100.0, 10.0, now);
        let vm = build(Some(&r), true, &Source::Windows, &Settings::default(), now);
        assert_eq!(vm.session.level, Level::Full);
        assert_eq!(vm.session.runs_out_text, None);
    }

    #[test]
    fn stale_state_and_text() {
        let now = local(28, 11, 0);
        let r = reading(10.0, 10.0, now);
        let vm = build(Some(&r), false, &Source::Windows, &Settings::default(), now);
        assert_eq!(vm.pill, PillState::Stale);
        assert_eq!(vm.updated_text, "As of 10:51");
        let ok = build(Some(&r), true, &Source::Windows, &Settings::default(), now);
        assert_eq!((ok.pill, ok.updated_text.as_str()), (PillState::Normal, "Updated 10:51"));
    }

    #[test]
    fn full_beats_alert_beats_stale() {
        let now = local(28, 11, 0);
        let r = reading(100.0, 95.0, now);
        let vm = build(Some(&r), false, &Source::Windows, &Settings::default(), now);
        assert_eq!(vm.pill, PillState::Full);
        let r = reading(10.0, 95.0, now);
        let vm = build(Some(&r), false, &Source::Windows, &Settings::default(), now);
        assert_eq!(vm.pill, PillState::Alert);
    }

    #[test]
    fn pill_text_full_session() {
        let now = local(28, 10, 0);
        let mut r = reading(100.0, 100.0, now);
        r.session.resets_at = utc(now) + Duration::minutes(80);
        let vm = build(Some(&r), true, &Source::Windows, &Settings::default(), now);
        assert_eq!(vm.pill_text.as_deref(), Some("Session full, back in 1h 20m"));
    }

    #[test]
    fn pill_text_full_week() {
        let now = local(28, 10, 51);
        let r = reading(10.0, 100.0, now);
        let vm = build(Some(&r), true, &Source::Windows, &Settings::default(), now);
        assert_eq!(vm.pill_text.as_deref(), Some("Week full, back in Fri 09:00"));
    }

    #[test]
    fn no_claude() {
        let vm = build(None, true, &Source::NotFound, &Settings::default(), local(28, 10, 0));
        assert_eq!(vm.pill, PillState::NoClaude);
        assert_eq!(vm.updated_text, "Install Claude Code to see your limits");
    }

    #[test]
    fn no_data() {
        let vm = build(None, true, &Source::Windows, &Settings::default(), local(28, 10, 0));
        assert_eq!(vm.pill, PillState::NoData);
        assert_eq!(vm.session.value_text, "–");
        assert_eq!(vm.week.value_text, "–");
        assert_eq!(vm.updated_text, "No reading yet");
        assert!(vm.announce.is_none() && !vm.refreshing);
    }

    fn session_value(used: f64) -> String {
        let now = local(28, 10, 0);
        let r = reading(used, 10.0, now);
        build(Some(&r), true, &Source::Windows, &Settings::default(), now).session.value_text
    }

    #[test]
    fn value_text_clamped() {
        assert_eq!(session_value(150.0), "100%");
        assert_eq!(session_value(-5.0), "0%");
        assert_eq!(session_value(-0.4), "0%");
    }

    #[test]
    fn value_text_never_100_below_full() {
        assert_eq!(session_value(99.5), "99%");
        assert_eq!(session_value(99.6), "99%");
        assert_eq!(session_value(100.0), "100%");
    }

    #[test]
    fn stale_text_even_when_alert() {
        let now = local(28, 11, 0);
        let r = reading(10.0, 95.0, now);
        let vm = build(Some(&r), false, &Source::Windows, &Settings::default(), now);
        assert_eq!((vm.pill, vm.updated_text.as_str()), (PillState::Alert, "As of 10:51"));
    }

    #[test]
    fn pill_state_json() {
        assert_eq!(serde_json::to_string(&PillState::NoClaude).unwrap(), "\"no_claude\"");
    }
}
