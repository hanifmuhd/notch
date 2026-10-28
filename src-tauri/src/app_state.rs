//! Shared app state behind one mutex: last reading, alert memory, settings and the
//! single-flight flag. Pure logic only, so it is testable without Tauri.

use crate::alerts::{evaluate, AlertEvent, AlertState, Thresholds};
use crate::settings::{self, Settings, SourcePref};
use crate::source::Source;
use crate::usage::UsageReading;
use crate::view::{self, ViewModel};
use chrono::{DateTime, Local};
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

struct Inner {
    reading: Option<UsageReading>,
    last_ok: bool,
    alerts: AlertState,
    settings: Settings,
    settings_path: PathBuf,
    source: Source,
    in_flight: bool,
    /// Last window (HWND as isize) that had focus before the notch; 0 = none.
    prev_foreground: isize,
}

pub struct AppState(Mutex<Inner>);

impl AppState {
    pub fn new(settings: Settings, settings_path: PathBuf) -> Self {
        let source = source_for(&settings.source);
        AppState(Mutex::new(Inner {
            reading: None,
            last_ok: true,
            alerts: AlertState::default(),
            settings,
            settings_path,
            // Auto re-detects on the first refresh; explicit prefs resolve at once.
            source,
            in_flight: false,
            prev_foreground: 0,
        }))
    }

    // A panic mid-refresh must not wedge the app, so a poisoned lock is still usable.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// True if the caller now owns the one refresh slot.
    pub fn try_start_refresh(&self) -> bool {
        let mut s = self.lock();
        !std::mem::replace(&mut s.in_flight, true)
    }

    /// A failed fetch (None) keeps the last reading and marks it stale.
    pub fn finish_refresh(&self, result: Option<UsageReading>) -> Vec<AlertEvent> {
        let mut s = self.lock();
        s.last_ok = result.is_some();
        if result.is_some() {
            s.reading = result;
        }
        s.in_flight = false;
        s.run_alerts()
    }

    pub fn view(&self, now: DateTime<Local>, announce: Option<String>) -> ViewModel {
        let s = self.lock();
        let mut vm = view::build(s.reading.as_ref(), s.last_ok, &s.source, &s.settings, now);
        vm.announce = announce;
        vm.refreshing = s.in_flight;
        vm
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    pub fn source(&self) -> Source {
        self.lock().source.clone()
    }

    pub fn set_source(&self, source: Source) {
        self.lock().source = source;
    }

    /// Clamps, writes the file and swaps the settings under one lock, so saves cannot interleave.
    /// Returns whether the source preference changed, plus any alert the new thresholds fire.
    pub fn save_settings(&self, new: Settings) -> io::Result<(bool, Vec<AlertEvent>)> {
        let new = settings::clamp(new);
        let mut s = self.lock();
        settings::save(&s.settings_path, &new)?;
        let source_changed = s.settings.source != new.source;
        if source_changed {
            s.source = source_for(&new.source);
        }
        s.settings = new;
        Ok((source_changed, s.run_alerts()))
    }

    pub fn prev_foreground(&self) -> isize {
        self.lock().prev_foreground
    }

    pub fn set_prev_foreground(&self, hwnd: isize) {
        self.lock().prev_foreground = hwnd;
    }
}

impl Inner {
    fn run_alerts(&mut self) -> Vec<AlertEvent> {
        let Some(r) = &self.reading else { return vec![] };
        let t = Thresholds { session: self.settings.session_threshold, week: self.settings.week_threshold };
        let (state, events) = evaluate(&self.alerts, r, &t);
        self.alerts = state;
        events
    }
}

/// Auto must probe (slow), so it starts as NotFound and the next refresh detects.
fn source_for(pref: &SourcePref) -> Source {
    match pref {
        SourcePref::Auto => Source::NotFound,
        SourcePref::Windows => Source::Windows,
        SourcePref::Wsl { distro } => Source::Wsl { distro: distro.clone() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::Limit;
    use chrono::{Duration, Utc};

    fn state() -> AppState {
        let s = AppState::new(Settings::default(), std::env::temp_dir().join("notch-app-state-unused.json"));
        s.set_source(Source::Windows);
        s
    }

    fn reading(session: f64) -> UsageReading {
        let now = Utc::now();
        UsageReading {
            session: Limit { used_pct: session, resets_at: now + Duration::hours(2) },
            week: Limit { used_pct: 10.0, resets_at: now + Duration::days(3) },
            fetched_at: now,
        }
    }

    #[test]
    fn single_flight() {
        let s = state();
        assert!(s.try_start_refresh());
        assert!(!s.try_start_refresh());
    }

    #[test]
    fn finish_clears_in_flight() {
        let s = state();
        assert!(s.try_start_refresh());
        s.finish_refresh(None);
        assert!(s.try_start_refresh());
    }

    #[test]
    fn failed_refresh_keeps_last_reading() {
        let s = state();
        s.try_start_refresh();
        s.finish_refresh(Some(reading(40.0)));
        s.try_start_refresh();
        s.finish_refresh(None);
        let vm = s.view(Local::now(), None);
        assert_eq!(vm.session.value_text, "40%");
        assert!(vm.updated_text.starts_with("As of"));
    }

    #[test]
    fn crossing_fires_once() {
        let s = state();
        s.try_start_refresh();
        assert_eq!(s.finish_refresh(Some(reading(95.0))).len(), 1);
        s.try_start_refresh();
        assert!(s.finish_refresh(Some(reading(96.0))).is_empty());
    }

    #[test]
    fn lowering_threshold_fires_on_last_reading_and_saves_clamped() {
        let dir = std::env::temp_dir().join(format!("notch-app-state-{}", std::process::id()));
        let path = dir.join("settings.json");
        let s = AppState::new(Settings::default(), path.clone());
        s.try_start_refresh();
        assert!(s.finish_refresh(Some(reading(60.0))).is_empty());
        let (changed, events) = s
            .save_settings(Settings { session_threshold: 10, ..Settings::default() })
            .unwrap();
        assert!(!changed);
        assert_eq!(events.len(), 1);
        assert_eq!(settings::load(&path).session_threshold, 50);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_pref_change_resets_source() {
        let dir = std::env::temp_dir().join(format!("notch-app-state-src-{}", std::process::id()));
        let s = AppState::new(Settings::default(), dir.join("settings.json"));
        let (changed, _) = s
            .save_settings(Settings { source: SourcePref::Windows, ..Settings::default() })
            .unwrap();
        assert!(changed);
        assert_eq!(s.source(), Source::Windows);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
