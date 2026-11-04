//! Turns the latest reading and settings into the JSON the webview renders (spec §5.2-5.3).
//! All display times are local; pace maths runs in UTC.

use crate::alerts::{level, AlertEvent, Level, LimitKind};
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
    LoggedOut,
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

/// A reading older than this counts as stale even if the last refresh worked (e.g. after sleep).
const MAX_FRESH_AGE: Duration = Duration::minutes(11);

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

/// Title and body of the toast for one alert event.
pub fn toast_text(e: &AlertEvent, now: DateTime<Local>) -> (String, String) {
    let name = |k: &LimitKind| match k {
        LimitKind::Session => "Session",
        LimitKind::Week => "Week",
    };
    match e {
        AlertEvent::Crossed(k, pct, resets) => {
            (format!("{} at {}%", name(k), pct.round()), reset_text(now, *resets))
        }
        AlertEvent::Full(k, resets) => {
            (format!("{} limit full", name(k)), format!("Back {}", when_text(now, *resets)))
        }
    }
}

/// Tray tooltip: the two numbers, or the app name when there is nothing to show.
pub fn tray_tooltip(vm: &ViewModel) -> String {
    match vm.pill {
        PillState::NoClaude | PillState::NoData | PillState::LoggedOut => "Claude Notch".into(),
        _ => format!("Session {}, week {}", vm.session.value_text, vm.week.value_text),
    }
}

/// Text of the tray's one show/hide item: it names the action a click will do.
pub fn tray_toggle_label(show_notch: bool) -> &'static str {
    if show_notch { "Hide notch" } else { "Show notch" }
}

fn pill_text(name: &str, now: DateTime<Local>, resets_at: Option<DateTime<Utc>>) -> String {
    let Some(resets_at) = resets_at else { return format!("{name} full") };
    match when_text(now, resets_at) {
        w if w == "now" || w.starts_with("in ") => format!("{name} full, back {w}"),
        w => format!("{name} full, back in {w}"),
    }
}

fn limit_view(limit: &Limit, threshold: u8, window: Duration, now: DateTime<Local>) -> LimitView {
    let now_utc = now.with_timezone(&Utc);
    let lvl = level(limit.used_pct, threshold);
    let pace = limit.resets_at.map_or(0.0, |r| elapsed_pct(window, now_utc, r));
    let clamped = limit.used_pct.clamp(0.0, 100.0);
    // Only a Full limit may read 100 %; 99.5 rounds up but is still Alert.
    let shown = if lvl == Level::Full { clamped.round() } else { clamped.round().min(99.0) };
    let runs_out_text = match (lvl, limit.resets_at) {
        (Level::Full, _) | (_, None) => None,
        (_, Some(r)) => runs_out_at(limit.used_pct, window, now_utc, r).map(|t| {
            format!("Runs out around {}", t.with_timezone(&Local).format("%H:%M"))
        }),
    };
    LimitView {
        value_text: format!("{}%", shown),
        used_pct: clamped,
        pace_pct: pace,
        level: lvl,
        reset_text: limit.resets_at.map_or("Starts when you next use Claude".into(), |r| reset_text(now, r)),
        pace_text: if limit.resets_at.is_some() { format!("{}% of time passed", pace.round()) } else { String::new() },
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

/// `source` is None while Claude has not been looked for yet; that is not "not installed".
pub fn build(
    reading: Option<&UsageReading>,
    last_ok: bool,
    source: Option<&Source>,
    settings: &Settings,
    logged_out: bool,
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
        (Some(Source::NotFound), _) => (
            PillState::NoClaude,
            None,
            "Install Claude Code to see your limits".to_string(),
        ),
        (_, _) if logged_out => (
            PillState::LoggedOut,
            None,
            "Log in to Claude Code: run claude, then /login".to_string(),
        ),
        (_, None) => (PillState::NoData, None, "No reading yet".to_string()),
        (_, Some(r)) => {
            let last_ok = last_ok && now.with_timezone(&Utc) - r.fetched_at <= MAX_FRESH_AGE;
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
            session: Limit { used_pct: s_used, resets_at: Some(utc(now) + Duration::minutes(133)) },
            week: Limit { used_pct: w_used, resets_at: Some(utc(local(30, 9, 0))) },
            fetched_at: utc(local(28, 10, 51)),
        }
    }

    #[test]
    fn idle_limit_view() {
        let now = local(28, 10, 0);
        let lv = limit_view(&Limit { used_pct: 0.0, resets_at: None }, 90, SESSION_WINDOW, now);
        assert_eq!(lv.reset_text, "Starts when you next use Claude");
        assert_eq!(lv.pace_text, "");
        assert_eq!(lv.pace_pct, 0.0);
        assert_eq!(lv.runs_out_text, None);
        assert_eq!(lv.level, Level::Normal);
    }

    #[test]
    fn full_without_reset_pill_has_no_time() {
        let now = local(28, 10, 0);
        let mut r = reading(100.0, 10.0, now);
        r.session.resets_at = None;
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!(vm.pill, PillState::Full);
        assert_eq!(vm.pill_text.as_deref(), Some("Session full"));
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
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!(vm.session.pace_text, "56% of time passed");
        assert_eq!(vm.session.value_text, "10%");
    }

    #[test]
    fn runs_out_text() {
        let now = local(28, 10, 0);
        let r = reading(80.0, 10.0, now);
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        let t = runs_out_at(80.0, SESSION_WINDOW, utc(now), r.session.resets_at.unwrap()).unwrap();
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
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!(vm.session.level, Level::Full);
        assert_eq!(vm.session.runs_out_text, None);
    }

    #[test]
    fn stale_state_and_text() {
        let now = local(28, 11, 0);
        let r = reading(10.0, 10.0, now);
        let vm = build(Some(&r), false, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!(vm.pill, PillState::Stale);
        assert_eq!(vm.updated_text, "As of 10:51");
        let ok = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!((ok.pill, ok.updated_text.as_str()), (PillState::Normal, "Updated 10:51"));
    }

    #[test]
    fn old_reading_is_stale() {
        let now = local(28, 11, 3); // reading() was fetched at 10:51
        let r = reading(10.0, 10.0, now);
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!((vm.pill, vm.updated_text.as_str()), (PillState::Stale, "As of 10:51"));
    }

    #[test]
    fn recent_reading_not_stale() {
        let now = local(28, 10, 55);
        let r = reading(10.0, 10.0, now);
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!((vm.pill, vm.updated_text.as_str()), (PillState::Normal, "Updated 10:51"));
    }

    #[test]
    fn full_beats_alert_beats_stale() {
        let now = local(28, 11, 0);
        let r = reading(100.0, 95.0, now);
        let vm = build(Some(&r), false, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!(vm.pill, PillState::Full);
        let r = reading(10.0, 95.0, now);
        let vm = build(Some(&r), false, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!(vm.pill, PillState::Alert);
    }

    #[test]
    fn pill_text_full_session() {
        let now = local(28, 10, 0);
        let mut r = reading(100.0, 100.0, now);
        r.session.resets_at = Some(utc(now) + Duration::minutes(80));
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!(vm.pill_text.as_deref(), Some("Session full, back in 1h 20m"));
    }

    #[test]
    fn pill_text_full_week() {
        let now = local(28, 10, 51);
        let r = reading(10.0, 100.0, now);
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!(vm.pill_text.as_deref(), Some("Week full, back in Fri 09:00"));
    }

    #[test]
    fn no_claude() {
        let vm = build(None, true, Some(&Source::NotFound), &Settings::default(), false, local(28, 10, 0));
        assert_eq!(vm.pill, PillState::NoClaude);
        assert_eq!(vm.updated_text, "Install Claude Code to see your limits");
    }

    #[test]
    fn logged_out_pill() {
        let now = local(28, 10, 0);
        let r = reading(10.0, 10.0, now);
        let vm = build(Some(&r), true, Some(&Source::Windows), &Settings::default(), true, now);
        assert_eq!(vm.pill, PillState::LoggedOut);
        assert_eq!(vm.updated_text, "Log in to Claude Code: run claude, then /login");
        assert_eq!(serde_json::to_string(&vm.pill).unwrap(), "\"logged_out\"");
        assert_eq!(tray_tooltip(&vm), "Claude Notch");
        // Not installed still wins.
        let vm = build(None, true, Some(&Source::NotFound), &Settings::default(), true, now);
        assert_eq!(vm.pill, PillState::NoClaude);
    }

    #[test]
    fn no_data() {
        let vm = build(None, true, Some(&Source::Windows), &Settings::default(), false, local(28, 10, 0));
        assert_eq!(vm.pill, PillState::NoData);
        assert_eq!(vm.session.value_text, "–");
        assert_eq!(vm.week.value_text, "–");
        assert_eq!(vm.updated_text, "No reading yet");
        assert!(vm.announce.is_none() && !vm.refreshing);
    }

    fn session_value(used: f64) -> String {
        let now = local(28, 10, 0);
        let r = reading(used, 10.0, now);
        build(Some(&r), true, Some(&Source::Windows), &Settings::default(), false, now).session.value_text
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
        let vm = build(Some(&r), false, Some(&Source::Windows), &Settings::default(), false, now);
        assert_eq!((vm.pill, vm.updated_text.as_str()), (PillState::Alert, "As of 10:51"));
    }

    #[test]
    fn pill_state_json() {
        assert_eq!(serde_json::to_string(&PillState::NoClaude).unwrap(), "\"no_claude\"");
    }

    #[test]
    fn toast_text_copy() {
        let now = local(28, 10, 0);
        let soon = utc(now) + Duration::minutes(90);
        assert_eq!(
            toast_text(&AlertEvent::Crossed(LimitKind::Session, 91.0, soon), now),
            ("Session at 91%".to_string(), "Resets in 1h 30m".to_string())
        );
        assert_eq!(
            toast_text(&AlertEvent::Full(LimitKind::Week, utc(local(30, 9, 0))), now),
            ("Week limit full".to_string(), "Back Fri 09:00".to_string())
        );
        assert_eq!(
            toast_text(&AlertEvent::Full(LimitKind::Session, soon), now).1,
            "Back in 1h 30m"
        );
        assert_eq!(
            toast_text(&AlertEvent::Full(LimitKind::Session, utc(now) - Duration::minutes(1)), now).1,
            "Back now"
        );
    }

    #[test]
    fn undetected_source_is_not_no_claude() {
        let now = local(28, 10, 0);
        let vm = build(None, true, None, &Settings::default(), false, now);
        assert_eq!((vm.pill, vm.updated_text.as_str()), (PillState::NoData, "No reading yet"));
        let r = reading(10.0, 10.0, now);
        assert_eq!(build(Some(&r), true, None, &Settings::default(), false, now).pill, PillState::Normal);
    }

    #[test]
    fn tray_tooltip_shows_numbers_or_the_name() {
        let now = local(28, 10, 0);
        let r = reading(42.0, 68.0, now);
        let build_with = |r: Option<&UsageReading>, src: Source| build(r, true, Some(&src), &Settings::default(), false, now);
        assert_eq!(tray_tooltip(&build_with(Some(&r), Source::Windows)), "Session 42%, week 68%");
        assert_eq!(tray_tooltip(&build_with(None, Source::Windows)), "Claude Notch");
        assert_eq!(tray_tooltip(&build_with(None, Source::NotFound)), "Claude Notch");
    }

    #[test]
    fn tray_toggle_label_names_the_next_action() {
        assert_eq!(tray_toggle_label(true), "Hide notch");
        assert_eq!(tray_toggle_label(false), "Show notch");
    }
}
