//! Shared app state behind one mutex: last reading, alert memory, settings and the
//! single-flight flag. Pure logic only, so it is testable without Tauri.

use crate::alerts::{self, evaluate, AlertEvent, AlertState, Thresholds};
use crate::events::{HookEvent, Notice, Sessions};
use crate::settings::{self, Settings, SourcePref};
use crate::source::{detect_with, usage_command, wsl_running, RunError, Runner, Source, CLI_TIMEOUT};
use crate::usage::{looks_logged_out, parse_usage, UsageError, UsageReading};
use crate::view::{self, ViewModel};
use chrono::{DateTime, Local, Utc};
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

struct Inner {
    reading: Option<UsageReading>,
    last_ok: bool,
    /// Claude Code asked for /login on the latest check; cleared by the next good reading.
    logged_out: bool,
    alerts: AlertState,
    alerts_path: PathBuf,
    settings: Settings,
    settings_path: PathBuf,
    /// None = not looked for yet (never shown as "Claude not installed").
    source: Option<Source>,
    in_flight: bool,
    /// Set when a source change arrives mid-refresh; that refresh then runs once more.
    refresh_again: bool,
    /// Last window (HWND as isize) that had focus before the notch; 0 = none.
    prev_foreground: isize,
    /// Claude Code sessions that need you or finished (spec 5.2), from the hook events.
    sessions: Sessions,
}

pub struct AppState(Mutex<Inner>);

impl AppState {
    pub fn new(settings: Settings, settings_path: PathBuf) -> Self {
        let source = source_for(&settings.source);
        let alerts_path = settings_path.with_file_name("alerts.json");
        AppState(Mutex::new(Inner {
            reading: None,
            last_ok: true,
            logged_out: false,
            alerts: alerts::load(&alerts_path),
            alerts_path,
            settings,
            settings_path,
            // Auto re-detects on the first refresh; explicit prefs resolve at once.
            source,
            in_flight: false,
            refresh_again: false,
            prev_foreground: 0,
            sessions: Sessions::default(),
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

    /// A failed or logged-out fetch keeps the last reading and marks it stale.
    pub fn finish_refresh(&self, result: Fetched) -> Vec<AlertEvent> {
        let mut s = self.lock();
        s.last_ok = matches!(result, Fetched::Reading(_));
        match result {
            Fetched::Reading(r) => {
                s.reading = Some(r);
                s.logged_out = false;
            }
            Fetched::LoggedOut => s.logged_out = true,
            Fetched::Failed => {}
        }
        s.in_flight = false;
        s.run_alerts()
    }

    pub fn view(&self, now: DateTime<Local>, announce: Option<String>) -> ViewModel {
        let s = self.lock();
        let mut vm = view::build(s.reading.as_ref(), s.last_ok, s.source.as_ref(), &s.settings, s.logged_out, now);
        vm.announce = announce;
        vm.refreshing = s.in_flight;
        vm.sessions = s.sessions.rows();
        vm.attention = s.sessions.attention();
        vm.pod = s.sessions.pod();
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

    /// Where the hooks drop event files: `events` beside settings.json.
    pub fn events_dir(&self) -> PathBuf {
        self.lock().settings_path.with_file_name("events")
    }

    /// Applies hook events as `events::take_files` returns them, read at `now`; returns the
    /// toasts to show.
    pub fn apply_events(&self, events: Vec<(HookEvent, DateTime<Utc>, bool)>, now: DateTime<Utc>) -> Vec<Notice> {
        let mut s = self.lock();
        let finished_on = s.settings.task_finished;
        events.into_iter().filter_map(|(e, at, quiet)| s.sessions.apply(e, at, now, quiet, finished_on)).collect()
    }

    /// Clears finished sessions older than 30 min; true if any went.
    pub fn expire_sessions(&self, now: DateTime<Utc>) -> bool {
        self.lock().sessions.expire(now)
    }

    /// The panel was opened and closed again.
    pub fn sessions_seen(&self) {
        self.lock().sessions.seen()
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
        if state != self.alerts {
            // Losing the memory only risks one repeat toast after a restart, so never fatal.
            if let Err(e) = alerts::save(&self.alerts_path, &state) {
                eprintln!("alerts: save failed: {e}");
            }
            self.alerts = state;
        }
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

/// Outcome of one usage check.
pub enum Fetched {
    Reading(UsageReading),
    LoggedOut,
    /// No reading: the call failed, WSL is stopped and Start WSL is off, or Claude is not found yet.
    Failed,
}

/// Runs the CLI (blocking, 6-9 s on WSL).
pub fn fetch(state: &AppState, runner: &dyn Runner) -> Fetched {
    let (settings, mut source) = state.snapshot();
    // Undetected and NotFound are both retried: Claude may have been installed or WSL started.
    if matches!(source, None | Some(Source::NotFound)) {
        let found = detect_with(runner, &settings.source, settings.start_wsl);
        if !state.set_detected(&settings.source, found.clone()) {
            return Fetched::Failed; // preference changed while detecting; refresh_again follows
        }
        source = found;
    }
    let Some(source) = source.filter(|s| *s != Source::NotFound) else { return Fetched::Failed };
    if let Source::Wsl { distro } = &source {
        if !settings.start_wsl && !wsl_running(runner, distro) {
            return Fetched::Failed;
        }
    }
    let (program, args) = usage_command(&source);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match runner.run(&program, &args, CLI_TIMEOUT) {
        Ok(raw) => match parse_usage(&raw, Utc::now()) {
            Ok(r) => Fetched::Reading(r),
            Err(UsageError::LoggedOut) => Fetched::LoggedOut,
            Err(_) => Fetched::Failed,
        },
        Err(RunError::Exit(_, stderr, stdout))
            if looks_logged_out(&stderr) || parse_usage(&stdout, Utc::now()) == Err(UsageError::LoggedOut) =>
        {
            Fetched::LoggedOut
        }
        Err(_) => Fetched::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::Limit;
    use chrono::Duration;
    use std::cell::RefCell;
    use std::time::Duration as StdDuration;

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("notch-app-state-{}-{}", std::process::id(), name))
    }

    /// An AppState in its own folder (alerts.json lands there), removed on drop, so no test
    /// sees another's fired alerts even though tests run in parallel under one pid.
    struct TestState(AppState, std::path::PathBuf);

    impl std::ops::Deref for TestState {
        type Target = AppState;
        fn deref(&self) -> &AppState {
            &self.0
        }
    }

    impl Drop for TestState {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }

    fn test_state(settings: Settings) -> TestState {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = scratch_dir(&format!("t{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        TestState(AppState::new(settings, dir.join("settings.json")), dir)
    }

    fn state() -> TestState {
        let s = test_state(Settings::default());
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
    fn session_events_follow_the_task_finished_switch() {
        use crate::events::{HookEvent, Kind};
        let stop = || vec![(HookEvent { session_id: "a".into(), project: "own".into(), kind: Kind::Finished }, Utc::now(), false)];
        let off = test_state(Settings { task_finished: false, ..Settings::default() });
        assert!(off.apply_events(stop(), Utc::now()).is_empty());
        let on = state();
        assert_eq!(on.apply_events(stop(), Utc::now()).len(), 1);
        assert!(!on.expire_sessions(Utc::now()));
        on.sessions_seen();
        assert!(on.lock().sessions.rows().is_empty());
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
        s.finish_refresh(Fetched::Failed);
        assert!(s.try_start_refresh());
    }

    #[test]
    fn failed_refresh_keeps_last_reading() {
        let s = state();
        s.try_start_refresh();
        s.finish_refresh(Fetched::Reading(reading(40.0)));
        s.try_start_refresh();
        s.finish_refresh(Fetched::Failed);
        let vm = s.view(Local::now(), None);
        assert_eq!(vm.session.value_text, "40%");
        assert!(vm.updated_text.starts_with("As of"));
    }

    #[test]
    fn crossing_fires_once() {
        let s = state();
        s.try_start_refresh();
        assert_eq!(s.finish_refresh(Fetched::Reading(reading(95.0))).len(), 1);
        s.try_start_refresh();
        assert!(s.finish_refresh(Fetched::Reading(reading(96.0))).is_empty());
    }

    #[test]
    fn lowering_threshold_fires_on_last_reading_and_saves_clamped() {
        let dir = std::env::temp_dir().join(format!("notch-app-state-{}", std::process::id()));
        let path = dir.join("settings.json");
        let s = AppState::new(Settings::default(), path.clone());
        s.try_start_refresh();
        assert!(s.finish_refresh(Fetched::Reading(reading(60.0))).is_empty());
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
        Err(RunError::Exit(1, String::new(), String::new()))
    }

    fn no_distro_running() -> FakeRunner {
        FakeRunner {
            map: vec![("where.exe claude", no_claude_on_windows()), ("wsl.exe -l --running -q", Ok(String::new()))],
            calls: RefCell::new(vec![]),
        }
    }

    #[test]
    fn exit_with_login_text_is_logged_out() {
        let r = FakeRunner {
            map: vec![(
                "cmd.exe /d /c claude -p --settings {\"disableAllHooks\":true} --output-format json /usage",
                Err(RunError::Exit(1, "Please run /login".into(), String::new())),
            )],
            calls: RefCell::new(vec![]),
        };
        let s = state();
        assert!(matches!(fetch(&s, &r), Fetched::LoggedOut));
    }

    #[test]
    fn exit_with_login_json_on_stdout_is_logged_out() {
        let out = r#"{"is_error":true,"result":"Invalid API key · Please run /login"}"#;
        let r = FakeRunner {
            map: vec![(
                "cmd.exe /d /c claude -p --settings {\"disableAllHooks\":true} --output-format json /usage",
                Err(RunError::Exit(1, String::new(), out.into())),
            )],
            calls: RefCell::new(vec![]),
        };
        assert!(matches!(fetch(&state(), &r), Fetched::LoggedOut));
        let other = RunError::Exit(1, "boom".into(), r#"{"is_error":true,"result":"rate limited"}"#.into());
        let r = FakeRunner { map: vec![(r.map[0].0, Err(other))], calls: RefCell::new(vec![]) };
        assert!(matches!(fetch(&state(), &r), Fetched::Failed));
    }

    #[test]
    fn logged_out_keeps_old_reading_until_a_reading_clears_it() {
        let s = state();
        s.try_start_refresh();
        s.finish_refresh(Fetched::Reading(reading(40.0)));
        s.try_start_refresh();
        s.finish_refresh(Fetched::LoggedOut);
        let vm = s.view(Local::now(), None);
        assert_eq!(vm.pill, crate::view::PillState::LoggedOut);
        assert_eq!(vm.session.value_text, "40%");
        // A plain failure does not hide the hint.
        s.try_start_refresh();
        s.finish_refresh(Fetched::Failed);
        assert_eq!(s.view(Local::now(), None).pill, crate::view::PillState::LoggedOut);
        s.try_start_refresh();
        s.finish_refresh(Fetched::Reading(reading(41.0)));
        assert_eq!(s.view(Local::now(), None).pill, crate::view::PillState::Normal);
    }

    #[test]
    fn fresh_auto_state_shows_no_data() {
        let s = test_state(Settings::default());
        assert_eq!(s.view(Local::now(), None).pill, crate::view::PillState::NoData);
    }

    #[test]
    fn stopped_wsl_with_start_wsl_off_is_not_booted() {
        let r = no_distro_running();
        let s = test_state(Settings { start_wsl: false, ..Settings::default() });
        s.try_start_refresh();
        assert!(matches!(fetch(&s, &r), Fetched::Failed));
        s.finish_refresh(Fetched::Failed);
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
        let s = test_state(settings);
        s.try_start_refresh();
        s.finish_refresh(Fetched::Reading(reading(40.0)));
        s.try_start_refresh();
        assert!(matches!(fetch(&s, &r), Fetched::Failed));
        s.finish_refresh(Fetched::Failed);
        assert!(r.calls.borrow().iter().all(|c| !c.contains(" -d ")));
        let vm = s.view(Local::now(), None);
        assert_eq!(vm.pill, crate::view::PillState::Stale);
        assert_eq!(vm.session.value_text, "40%");
    }

    #[test]
    fn source_change_mid_detect_not_overwritten() {
        let s = test_state(Settings::default());
        let (old, _) = s.snapshot();
        // Detection for Auto is running; the user switches to Windows meanwhile.
        s.save_settings(Settings { source: SourcePref::Windows, ..Settings::default() }).unwrap();
        assert!(!s.set_detected(&old.source, Some(Source::Wsl { distro: "Ubuntu".into() })));
        assert_eq!(s.snapshot().1, Some(Source::Windows));
    }

    #[test]
    fn refresh_again_after_busy() {
        let dir = std::env::temp_dir().join(format!("notch-app-state-again-{}", std::process::id()));
        let s = AppState::new(Settings::default(), dir.join("settings.json"));
        assert!(s.try_start_refresh());
        s.save_settings(Settings { source: SourcePref::Windows, ..Settings::default() }).unwrap();
        s.finish_refresh(Fetched::Failed);
        assert!(s.take_refresh_again());
        assert!(!s.take_refresh_again());
        // With nothing running, the caller's own refresh_now handles it.
        s.save_settings(Settings { source: SourcePref::Auto, ..Settings::default() }).unwrap();
        assert!(!s.take_refresh_again());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restart_does_not_refire() {
        let dir = scratch_dir("restart");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let r = reading(92.0);
        let first = AppState::new(Settings::default(), path.clone());
        first.try_start_refresh();
        assert_eq!(first.finish_refresh(Fetched::Reading(r.clone())).len(), 1);
        let second = AppState::new(Settings::default(), path);
        second.try_start_refresh();
        assert!(second.finish_refresh(Fetched::Reading(r)).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn view_carries_sessions() {
        use crate::events::{HookEvent, Kind};
        let s = state();
        let ev = |id: &str, kind| (HookEvent { session_id: id.into(), project: "own".into(), kind }, Utc::now(), false);
        s.apply_events(vec![ev("a", Kind::NeedsYou("m".into())), ev("b", Kind::Finished)], Utc::now());
        let vm = s.view(Local::now(), None);
        assert_eq!(vm.attention, 2);
        assert_eq!(vm.sessions.len(), 2);
        assert_eq!(vm.sessions[0].status, "finished"); // newest first
        assert_eq!(vm.sessions[1].status, "needs_you");
        assert_eq!(vm.flash, None);
        assert_eq!(vm.pod.map(|p| p.status), Some("needs_you"));
    }
}
