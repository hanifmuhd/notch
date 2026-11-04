//! Shared app state behind one mutex: last reading, alert memory, settings and the
//! single-flight flag. Pure logic only, so it is testable without Tauri.

use crate::alerts::{evaluate, AlertEvent, AlertState, Thresholds};
use crate::settings::{self, Settings, SourcePref};
use crate::source::{detect_with, usage_command, wsl_running, Runner, Source, CLI_TIMEOUT};
use crate::usage::{parse_usage, UsageReading};
use crate::view::{self, ViewModel};
use chrono::{DateTime, Local, Utc};
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

struct Inner {
    reading: Option<UsageReading>,
    last_ok: bool,
    alerts: AlertState,
    settings: Settings,
    settings_path: PathBuf,
    /// None = not looked for yet (never shown as "Claude not installed").
    source: Option<Source>,
    in_flight: bool,
    /// Set when a source change arrives mid-refresh; that refresh then runs once more.
    refresh_again: bool,
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
            refresh_again: false,
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
        let mut vm = view::build(s.reading.as_ref(), s.last_ok, s.source.as_ref(), &s.settings, now);
        vm.announce = announce;
        vm.refreshing = s.in_flight;
        vm
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Settings and source read together so one refresh sees a consistent pair.
    pub fn snapshot(&self) -> (Settings, Option<Source>) {
        let s = self.lock();
        (s.settings.clone(), s.source.clone())
    }

    /// Stores a detection result only if the source preference it was made for is still current.
    pub fn set_detected(&self, pref: &SourcePref, found: Option<Source>) -> bool {
        let mut s = self.lock();
        let current = s.settings.source == *pref;
        if current {
            s.source = found;
        }
        current
    }

    /// True once if a source change arrived while a refresh was running.
    pub fn take_refresh_again(&self) -> bool {
        std::mem::take(&mut self.lock().refresh_again)
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
            // The running refresh may be using the old source; the caller's own
            // refresh_now would be dropped by single flight.
            s.refresh_again |= s.in_flight;
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

/// Auto must probe (slow), so it starts undetected and the next refresh detects.
fn source_for(pref: &SourcePref) -> Option<Source> {
    match pref {
        SourcePref::Auto => None,
        SourcePref::Windows => Some(Source::Windows),
        SourcePref::Wsl { distro } => Some(Source::Wsl { distro: distro.clone() }),
    }
}

/// Runs the CLI (blocking, 6-9 s on WSL). None means no reading: the call failed, WSL is
/// stopped and Start WSL is off, or Claude is not found yet.
pub fn fetch(state: &AppState, runner: &dyn Runner) -> Option<UsageReading> {
    let (settings, mut source) = state.snapshot();
    // Undetected and NotFound are both retried: Claude may have been installed or WSL started.
    if matches!(source, None | Some(Source::NotFound)) {
        let found = detect_with(runner, &settings.source, settings.start_wsl);
        if !state.set_detected(&settings.source, found.clone()) {
            return None; // preference changed while detecting; refresh_again follows
        }
        source = found;
    }
    let source = source.filter(|s| *s != Source::NotFound)?;
    if let Source::Wsl { distro } = &source {
        if !settings.start_wsl && !wsl_running(runner, distro) {
            return None;
        }
    }
    let (program, args) = usage_command(&source);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let raw = runner.run(&program, &args, CLI_TIMEOUT).ok()?;
    parse_usage(&raw, Utc::now()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::Limit;
    use crate::source::RunError;
    use chrono::Duration;
    use std::cell::RefCell;
    use std::time::Duration as StdDuration;

    fn state() -> AppState {
        let s = AppState::new(Settings::default(), std::env::temp_dir().join("notch-app-state-unused.json"));
        s.lock().source = Some(Source::Windows);
        s
    }

    fn reading(session: f64) -> UsageReading {
        let now = Utc::now();
        UsageReading {
            session: Limit { used_pct: session, resets_at: Some(now + Duration::hours(2)) },
            week: Limit { used_pct: 10.0, resets_at: Some(now + Duration::days(3)) },
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
        assert_eq!(s.snapshot().1, Some(Source::Windows));
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct FakeRunner {
        map: Vec<(&'static str, Result<String, RunError>)>,
        calls: RefCell<Vec<String>>,
    }

    impl Runner for FakeRunner {
        fn run(&self, program: &str, args: &[&str], _t: StdDuration) -> Result<String, RunError> {
            let key = format!("{} {}", program, args.join(" "));
            self.calls.borrow_mut().push(key.clone());
            self.map
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or(Err(RunError::Spawn("unknown".into())))
        }
    }

    fn no_claude_on_windows() -> Result<String, RunError> {
        Err(RunError::Exit(1, String::new()))
    }

    fn no_distro_running() -> FakeRunner {
        FakeRunner {
            map: vec![("where.exe claude", no_claude_on_windows()), ("wsl.exe -l --running -q", Ok(String::new()))],
            calls: RefCell::new(vec![]),
        }
    }

    #[test]
    fn fresh_auto_state_shows_no_data() {
        let s = AppState::new(Settings::default(), std::env::temp_dir().join("notch-app-state-unused.json"));
        assert_eq!(s.view(Local::now(), None).pill, crate::view::PillState::NoData);
    }

    #[test]
    fn stopped_wsl_with_start_wsl_off_is_not_booted() {
        let r = no_distro_running();
        let s = AppState::new(
            Settings { start_wsl: false, ..Settings::default() },
            std::env::temp_dir().join("notch-app-state-unused.json"),
        );
        s.try_start_refresh();
        assert!(fetch(&s, &r).is_none());
        s.finish_refresh(None);
        assert!(r.calls.borrow().iter().all(|c| !c.contains(" -d ") && !c.contains("-l -q")));
        // Still undetected, so the pill says "No reading yet", not "Install Claude Code".
        assert_eq!(s.view(Local::now(), None).pill, crate::view::PillState::NoData);
        assert_eq!(s.snapshot().1, None);
    }

    #[test]
    fn explicit_wsl_stopped_with_start_wsl_off_goes_stale() {
        let r = no_distro_running();
        let settings = Settings {
            start_wsl: false,
            source: SourcePref::Wsl { distro: "Ubuntu".into() },
            ..Settings::default()
        };
        let s = AppState::new(settings, std::env::temp_dir().join("notch-app-state-unused.json"));
        s.try_start_refresh();
        s.finish_refresh(Some(reading(40.0)));
        s.try_start_refresh();
        assert!(fetch(&s, &r).is_none());
        s.finish_refresh(None);
        assert!(r.calls.borrow().iter().all(|c| !c.contains(" -d ")));
        let vm = s.view(Local::now(), None);
        assert_eq!(vm.pill, crate::view::PillState::Stale);
        assert_eq!(vm.session.value_text, "40%");
    }

    #[test]
    fn source_change_mid_detect_not_overwritten() {
        let s = AppState::new(Settings::default(), std::env::temp_dir().join("notch-app-state-mid.json"));
        let (old, _) = s.snapshot();
        // Detection for Auto is running; the user switches to Windows meanwhile.
        s.save_settings(Settings { source: SourcePref::Windows, ..Settings::default() }).unwrap();
        assert!(!s.set_detected(&old.source, Some(Source::Wsl { distro: "Ubuntu".into() })));
        assert_eq!(s.snapshot().1, Some(Source::Windows));
        let _ = std::fs::remove_file(std::env::temp_dir().join("notch-app-state-mid.json"));
    }

    #[test]
    fn refresh_again_after_busy() {
        let dir = std::env::temp_dir().join(format!("notch-app-state-again-{}", std::process::id()));
        let s = AppState::new(Settings::default(), dir.join("settings.json"));
        assert!(s.try_start_refresh());
        s.save_settings(Settings { source: SourcePref::Windows, ..Settings::default() }).unwrap();
        s.finish_refresh(None);
        assert!(s.take_refresh_again());
        assert!(!s.take_refresh_again());
        // With nothing running, the caller's own refresh_now handles it.
        s.save_settings(Settings { source: SourcePref::Auto, ..Settings::default() }).unwrap();
        assert!(!s.take_refresh_again());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
