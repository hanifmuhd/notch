//! Decides each limit's colour level and when a toast fires.
//! Thresholds are 50-99 %, full is always 100 %. Only a window reset re-arms a fired toast.

use crate::usage::{Limit, UsageReading};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LimitKind {
    Session,
    Week,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Normal,
    Alert,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    pub session: u8,
    pub week: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Armed {
    fired_alert: bool,
    fired_full: bool,
    resets_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertState {
    session: Armed,
    week: Armed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertEvent {
    Crossed(LimitKind, f64, DateTime<Utc>),
    Full(LimitKind, DateTime<Utc>),
}

impl Default for Armed {
    // Epoch means the first reading after launch fires once if already over a threshold.
    fn default() -> Self {
        Armed { fired_alert: false, fired_full: false, resets_at: DateTime::<Utc>::UNIX_EPOCH }
    }
}

impl Default for AlertState {
    fn default() -> Self {
        AlertState { session: Armed::default(), week: Armed::default() }
    }
}

/// Missing, unreadable or corrupt files mean nothing has fired yet.
pub fn load(path: &Path) -> AlertState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, s: &AlertState) -> io::Result<()> {
    crate::settings::write_atomic(path, &serde_json::to_string(s).map_err(io::Error::other)?)
}

pub fn level(used_pct: f64, threshold: u8) -> Level {
    if used_pct >= 100.0 {
        Level::Full
    } else if used_pct >= f64::from(threshold) {
        Level::Alert
    } else {
        Level::Normal
    }
}

fn step(armed: &Armed, limit: &Limit, threshold: u8, kind: LimitKind) -> (Armed, Option<AlertEvent>) {
    // Idle: no window is running, so there is nothing to alert on and nothing to re-arm.
    let Some(resets_at) = limit.resets_at else { return (armed.clone(), None) };
    let mut a = armed.clone();
    if resets_at - a.resets_at > Duration::minutes(1) {
        a = Armed { resets_at, ..Armed::default() };
    }
    let used = limit.used_pct;
    // Within a window usage never falls, so this only happens when the threshold was raised.
    if used < f64::from(threshold) {
        a.fired_alert = false;
    }
    let event = if used >= 100.0 && !a.fired_full {
        a.fired_full = true;
        a.fired_alert = true;
        Some(AlertEvent::Full(kind, resets_at))
    } else if used >= f64::from(threshold) && !a.fired_alert {
        a.fired_alert = true;
        Some(AlertEvent::Crossed(kind, used, resets_at))
    } else {
        None
    };
    (a, event)
}

pub fn evaluate(
    state: &AlertState,
    reading: &UsageReading,
    t: &Thresholds,
) -> (AlertState, Vec<AlertEvent>) {
    let (session, s_event) = step(&state.session, &reading.session, t.session, LimitKind::Session);
    let (week, w_event) = step(&state.week, &reading.week, t.week, LimitKind::Week);
    (AlertState { session, week }, s_event.into_iter().chain(w_event).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap()
    }

    fn reading(session: f64, week: f64, session_reset: DateTime<Utc>) -> UsageReading {
        UsageReading {
            session: Limit { used_pct: session, resets_at: Some(session_reset) },
            week: Limit { used_pct: week, resets_at: Some(at(30)) },
            fetched_at: at(1),
        }
    }

    const T: Thresholds = Thresholds { session: 90, week: 90 };

    #[test]
    fn crossing_fires_once() {
        let r = at(2);
        let (s, e) = evaluate(&AlertState::default(), &reading(85.0, 0.0, r), &T);
        assert!(e.is_empty());
        let (s, e) = evaluate(&s, &reading(91.0, 0.0, r), &T);
        assert_eq!(e, vec![AlertEvent::Crossed(LimitKind::Session, 91.0, r)]);
        let (_, e) = evaluate(&s, &reading(93.0, 0.0, r), &T);
        assert!(e.is_empty());
    }

    #[test]
    fn full_fires_once() {
        let r = at(2);
        let (s, _) = evaluate(&AlertState::default(), &reading(99.0, 0.0, r), &T);
        let (s, e) = evaluate(&s, &reading(100.0, 0.0, r), &T);
        assert_eq!(e, vec![AlertEvent::Full(LimitKind::Session, r)]);
        let (_, e) = evaluate(&s, &reading(100.0, 0.0, r), &T);
        assert!(e.is_empty());
    }

    #[test]
    fn full_jump_emits_only_full() {
        let r = at(2);
        let (s, _) = evaluate(&AlertState::default(), &reading(85.0, 0.0, r), &T);
        let (s, e) = evaluate(&s, &reading(100.0, 0.0, r), &T);
        assert_eq!(e, vec![AlertEvent::Full(LimitKind::Session, r)]);
        let (_, e) = evaluate(&s, &reading(100.0, 0.0, r), &T);
        assert!(e.is_empty());
    }

    #[test]
    fn small_reset_drift_does_not_rearm() {
        let r = at(2);
        let (s, _) = evaluate(&AlertState::default(), &reading(100.0, 0.0, r), &T);
        let (_, e) = evaluate(&s, &reading(100.0, 0.0, r + Duration::seconds(30)), &T);
        assert!(e.is_empty());
    }

    #[test]
    fn full_refires_in_new_window() {
        let (s, _) = evaluate(&AlertState::default(), &reading(100.0, 0.0, at(2)), &T);
        let (_, e) = evaluate(&s, &reading(100.0, 0.0, at(3)), &T);
        assert_eq!(e, vec![AlertEvent::Full(LimitKind::Session, at(3))]);
    }

    #[test]
    fn reset_rearms() {
        let (s, e) = evaluate(&AlertState::default(), &reading(92.0, 0.0, at(2)), &T);
        assert_eq!(e, vec![AlertEvent::Crossed(LimitKind::Session, 92.0, at(2))]);
        let (_, e) = evaluate(&s, &reading(92.0, 0.0, at(3)), &T);
        assert_eq!(e, vec![AlertEvent::Crossed(LimitKind::Session, 92.0, at(3))]);
    }

    #[test]
    fn lower_threshold_fires() {
        let r = at(2);
        let (s, e) = evaluate(&AlertState::default(), &reading(70.0, 0.0, r), &T);
        assert!(e.is_empty());
        let low = Thresholds { session: 60, week: 90 };
        let (s, e) = evaluate(&s, &reading(70.0, 0.0, r), &low);
        assert_eq!(e, vec![AlertEvent::Crossed(LimitKind::Session, 70.0, r)]);
        let (_, e) = evaluate(&s, &reading(70.0, 0.0, r), &low);
        assert!(e.is_empty());
    }

    #[test]
    fn raise_threshold_clears_silently() {
        let r = at(2);
        let (s, _) = evaluate(&AlertState::default(), &reading(92.0, 0.0, r), &T);
        let high = Thresholds { session: 95, week: 90 };
        let (_, e) = evaluate(&s, &reading(92.0, 0.0, r), &high);
        assert!(e.is_empty());
        assert_eq!(level(92.0, 95), Level::Normal);
    }

    #[test]
    fn raised_then_crossed_again() {
        let r = at(2);
        let (s, _) = evaluate(&AlertState::default(), &reading(92.0, 0.0, r), &T);
        let high = Thresholds { session: 95, week: 90 };
        let (s, e) = evaluate(&s, &reading(92.0, 0.0, r), &high);
        assert!(e.is_empty());
        let (_, e) = evaluate(&s, &reading(96.0, 0.0, r), &high);
        assert_eq!(e, vec![AlertEvent::Crossed(LimitKind::Session, 96.0, r)]);
    }

    #[test]
    fn over_100_is_full() {
        assert_eq!(level(104.0, 90), Level::Full);
        assert_eq!(level(90.0, 90), Level::Alert);
        assert_eq!(level(f64::NAN, 90), Level::Normal);
    }

    #[test]
    fn week_independent() {
        let r = at(2);
        let (s, e) = evaluate(&AlertState::default(), &reading(95.0, 10.0, r), &T);
        assert_eq!(e, vec![AlertEvent::Crossed(LimitKind::Session, 95.0, r)]);
        let (_, e) = evaluate(&s, &reading(95.0, 91.0, r), &T);
        assert_eq!(e, vec![AlertEvent::Crossed(LimitKind::Week, 91.0, at(30))]);
    }

    #[test]
    fn idle_limit_never_fires() {
        let r = at(2);
        let (s, _) = evaluate(&AlertState::default(), &reading(91.0, 0.0, r), &T);
        let mut idle = reading(95.0, 0.0, r);
        idle.session.resets_at = None;
        let (after, e) = evaluate(&s, &idle, &T);
        assert!(e.is_empty());
        assert_eq!(after.session, s.session);
        let (_, e) = evaluate(&AlertState::default(), &idle, &T);
        assert!(e.is_empty());
    }

    #[test]
    fn level_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Level::Alert).unwrap(), "\"alert\"");
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("notch-alerts-test-{}-{}", std::process::id(), name))
    }

    #[test]
    fn alert_state_round_trips() {
        let dir = test_dir("round_trip");
        let path = dir.join("alerts.json");
        let (s, _) = evaluate(&AlertState::default(), &reading(95.0, 100.0, at(2)), &T);
        assert_ne!(s, AlertState::default());
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_alerts_file_is_default() {
        let dir = test_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("alerts.json");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), AlertState::default());
        assert_eq!(load(&dir.join("missing.json")), AlertState::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
