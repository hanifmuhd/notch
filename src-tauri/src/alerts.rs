//! Decides each limit's colour level and when a toast fires.
//! Thresholds are 50-99 %, full is always 100 %. Only a window reset re-arms a fired toast.

use crate::usage::{Limit, UsageReading};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

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

#[derive(Debug, Clone, PartialEq)]
struct Armed {
    fired_alert: bool,
    fired_full: bool,
    resets_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
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
    let mut a = armed.clone();
    if limit.resets_at - a.resets_at > Duration::minutes(1) {
        a = Armed { resets_at: limit.resets_at, ..Armed::default() };
    }
    let used = limit.used_pct;
    // Within a window usage never falls, so this only happens when the threshold was raised.
    if used < f64::from(threshold) {
        a.fired_alert = false;
    }
    let event = if used >= 100.0 && !a.fired_full {
        a.fired_full = true;
        a.fired_alert = true;
        Some(AlertEvent::Full(kind, limit.resets_at))
    } else if used >= f64::from(threshold) && !a.fired_alert {
        a.fired_alert = true;
        Some(AlertEvent::Crossed(kind, used, limit.resets_at))
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
            session: Limit { used_pct: session, resets_at: session_reset },
            week: Limit { used_pct: week, resets_at: at(30) },
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
    fn level_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Level::Alert).unwrap(), "\"alert\"");
    }
}
