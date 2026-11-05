#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod app_state;
mod events;
mod fullscreen;
mod hit;
mod hooks;
mod monitors;
mod pace;
mod placement;
mod prayer;
mod prayer_zones;
mod settings;
mod source;
mod taskbar;
mod tray;
mod update;
mod usage;
mod view;

use alerts::AlertEvent;
use app_state::AppState;
use chrono::{DateTime, Local, Utc};
use serde::Serialize;
use settings::Settings;
use source::SystemRunner;
use placement::{notch_origin, pick_monitor, Rect};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri_plugin_notification::NotificationExt;
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, State, WindowEvent};
use view::ViewModel;

const REFRESH_EVERY: Duration = Duration::from_secs(300);
/// How often the refresh loop checks the wall clock; a plain 300 s sleep may not count suspend time.
const REFRESH_CHECK: Duration = Duration::from_secs(15);
/// The notch window never resizes (spec 4, approach A): room for the open panel, the pod and
/// the glow. CSS animates the shape inside; the pointer loop passes clicks through elsewhere.
const WINDOW: (f64, f64) = (860.0, 560.0);
const MONITOR_POLL: Duration = Duration::from_secs(2);
const EVENTS_POLL: Duration = Duration::from_secs(1);

/// Physical rect of the notch window and its scale, set by `place_window`, read by the pointer loop.
static WINDOW_RECT: Mutex<Option<(Rect, f64)>> = Mutex::new(None);
/// Where the notch shows right now (spec 5.5): top, or in the taskbar full or rings-only, plus
/// the taskbar it was measured from. Decided by `track_taskbar`.
static MODE: Mutex<(Mode, Option<taskbar::Taskbar>)> = Mutex::new((Mode::Top, None));

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Top,
    Taskbar,
    TaskbarMini,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Top => "top",
            Mode::Taskbar => "taskbar",
            Mode::TaskbarMini => "taskbar_mini",
        }
    }
}

/// Capsule widths and the gap kept to the icons and the tray, logical px.
const CAPSULE_FULL: f64 = 150.0;
const CAPSULE_MINI: f64 = 44.0;
const CAPSULE_MARGIN: f64 = 8.0;

/// Rectangles of the visible shape, CSS px relative to the window (from the webview).
static HIT_RECTS: Mutex<Vec<hit::HitRect>> = Mutex::new(Vec::new());

/// Device name of the monitor `place_window` last put the notch on (what full screen is judged against).
static NOTCH_MONITOR: Mutex<Option<String>> = Mutex::new(None);
/// A full-screen app is on the notch's monitor, as of the last 1 s check.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// Whether the notch window is shown right now; the window starts visible.
static SHOWN: AtomicBool = AtomicBool::new(true);
/// Wall-clock start of the latest refresh, automatic or manual.
static LAST_REFRESH_START: Mutex<DateTime<Utc>> = Mutex::new(DateTime::<Utc>::MIN_UTC);

#[derive(Serialize)]
struct ScreenInfo {
    id: String,
    label: String,
    primary: bool,
}

/// Shown even while the notch is hidden.
fn toast(app: &AppHandle, title: String, body: String) {
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        eprintln!("toast failed: {err}");
    }
}

/// One toast per event (spec 5.5).
fn on_alert(app: &AppHandle, events: Vec<AlertEvent>) {
    for e in &events {
        let (title, body) = view::toast_text(e, Local::now());
        toast(app, title, body);
    }
}

/// Titles of the events joined for screen readers, or None when there are none.
fn announce(events: &[AlertEvent]) -> Option<String> {
    let now = Local::now();
    let titles: Vec<String> = events.iter().map(|e| view::toast_text(e, now).0).collect();
    (!titles.is_empty()).then(|| titles.join(". "))
}

fn emit(app: &AppHandle, announce: Option<String>) -> ViewModel {
    emit_flash(app, announce, None)
}

/// `flash` is set only on the emit that carries a new session notice (spec 5.3).
fn emit_flash(app: &AppHandle, announce: Option<String>, flash: Option<String>) -> ViewModel {
    emit_with(app, announce, flash, None)
}

/// The one place a view goes out: `flash` (session notice) and `banner` (prayer) are set only on
/// the emit that carries them.
fn emit_with(app: &AppHandle, announce: Option<String>, flash: Option<String>, banner: Option<view::Banner>) -> ViewModel {
    if let Some(b) = &banner {
        *LAST_BANNER.lock().unwrap_or_else(|e| e.into_inner()) = Some((b.clone(), std::time::Instant::now()));
    }
    let mut vm = app.state::<AppState>().view(Local::now(), announce);
    vm.flash = flash;
    vm.banner = banner;
    vm.mode = mode().0.name();
    #[cfg(debug_assertions)]
    eprintln!("usage-updated {}", serde_json::to_string(&vm).unwrap_or_default());
    let _ = app.emit("usage-updated", &vm);
    tray::update(app, &vm);
    vm
}

/// True once `REFRESH_EVERY` of wall-clock time has passed since `last_start` (or the clock went back).
fn due(now: DateTime<Utc>, last_start: DateTime<Utc>) -> bool {
    now < last_start || (now - last_start).num_seconds() >= REFRESH_EVERY.as_secs() as i64
}

/// One refresh cycle; ignored while another is running. Runs again once if a source change
/// arrived meanwhile.
fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.try_start_refresh() {
        return;
    }
    *LAST_REFRESH_START.lock().unwrap_or_else(|e| e.into_inner()) = Utc::now();
    loop {
        emit(app, None);
        // A panic must still release the in-flight slot.
        let result = catch_unwind(AssertUnwindSafe(|| app_state::fetch(&state, &SystemRunner)))
            .unwrap_or(app_state::Fetched::Failed);
        let events = state.finish_refresh(result);
        emit(app, announce(&events));
        on_alert(app, events);
        if !state.take_refresh_again() || !state.try_start_refresh() {
            break;
        }
        *LAST_REFRESH_START.lock().unwrap_or_else(|e| e.into_inner()) = Utc::now();
    }
}

/// The last prayer banner and when it went out: an alert due at start-up fires before the page
/// listens, so the page's first get_view picks it up instead.
static LAST_BANNER: Mutex<Option<(view::Banner, std::time::Instant)>> = Mutex::new(None);

/// The banner with the seconds it has left, or None once it is over.
fn still_showing(b: &view::Banner, elapsed: Duration) -> Option<view::Banner> {
    let left = u64::from(b.seconds).checked_sub(elapsed.as_secs()).filter(|&l| l > 0)?;
    Some(view::Banner { seconds: left as u32, ..b.clone() })
}

#[tauri::command]
fn get_view(state: State<AppState>) -> ViewModel {
    let mut vm = state.view(Local::now(), None);
    vm.mode = mode().0.name();
    vm.banner = LAST_BANNER.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|(b, at)| still_showing(b, at.elapsed()));
    vm
}

fn mode() -> (Mode, Option<taskbar::Taskbar>) {
    *MODE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Decides top / taskbar / rings-only from the setting and a fresh taskbar measurement, and
/// re-places the window when that changes. Runs off the main thread (UI Automation is slow-ish).
fn update_mode(app: &AppHandle) {
    let want_taskbar = app.try_state::<AppState>().is_some_and(|s| s.settings().show_in == settings::ShowIn::Taskbar);
    #[cfg(windows)]
    let measured = if want_taskbar { taskbar::measure().filter(|t| t.usable()) } else { None };
    #[cfg(not(windows))]
    let measured: Option<taskbar::Taskbar> = None;
    let scale = app.primary_monitor().ok().flatten().map_or(1.0, |m| m.scale_factor());
    let px = |v: f64| (v * scale).round() as i32;
    let next = match measured {
        Some(t) => match taskbar::fit(t.tray_left - t.apps_right, px(CAPSULE_FULL), px(CAPSULE_MINI), px(CAPSULE_MARGIN)) {
            taskbar::Fit::Full => Mode::Taskbar,
            taskbar::Fit::Mini => Mode::TaskbarMini,
            taskbar::Fit::Top => Mode::Top,
        },
        None => Mode::Top,
    };
    let changed = {
        let mut m = MODE.lock().unwrap_or_else(|e| e.into_inner());
        let changed = m.0 != next || (next != Mode::Top && m.1.map(|t| t.tray_left) != measured.map(|t| t.tray_left));
        *m = (next, measured);
        changed
    };
    if changed {
        replace(app);
        emit(app, None);
    }
}

/// Every 2 s keeps the mode right as apps open and close (spec 5.5).
fn track_taskbar(app: AppHandle) {
    std::thread::spawn(move || loop {
        update_mode(&app);
        std::thread::sleep(Duration::from_secs(2));
    });
}

#[tauri::command]
fn refresh_now(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || refresh(&app));
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<ViewModel, String> {
    let old_source = state.snapshot().1;
    let (source_changed, events) = state.save_settings(settings).map_err(|e| e.to_string())?;
    replace(&app);
    let handle = app.clone();
    std::thread::spawn(move || update_mode(&handle)); // Show in may have changed
    if let Some(kick) = PRAYER_KICK.get() {
        let _ = kick.send(()); // prayer on/off, zone or reminder may have changed
    }
    let vm = emit(&app, announce(&events));
    on_alert(&app, events);
    if source_changed {
        move_hooks(app.clone(), old_source);
        refresh_now(app);
    }
    Ok(vm)
}

/// Best effort, off the main thread: WSL calls take seconds.
fn move_hooks(app: AppHandle, old_source: Option<source::Source>) {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (s, _) = state.snapshot();
        // Auto is undetected right after a change, so look for the new install here.
        let new = source::detect_with(&SystemRunner, &s.source, s.start_wsl);
        let moved = hooks::move_hooks(old_source.as_ref(), new.as_ref(), s.start_wsl, &state.events_dir(), &SystemRunner);
        if let Err(e) = moved {
            eprintln!("hooks: move after source change failed: {}", e.message());
        }
    });
}

/// The Task alerts switch, read from Claude's settings.json (seconds over WSL, so off the main thread).
/// Once Claude is found, re-installs our hooks if they are on, so an install from an older
/// version gains new events (the PostToolUse heartbeat). Idempotent; foreign hooks stay.
fn refresh_hooks(app: AppHandle) {
    std::thread::spawn(move || {
        for _ in 0..120 {
            let (s, source) = app.state::<AppState>().snapshot();
            if let Some(src) = source.filter(|s| *s != source::Source::NotFound) {
                if hooks::status(Some(&src), s.start_wsl, &SystemRunner).on {
                    let dir = app.state::<AppState>().events_dir();
                    if let Err(e) = hooks::set(Some(&src), s.start_wsl, true, &dir, &SystemRunner) {
                        eprintln!("hooks: refresh: {e}");
                    }
                }
                return;
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

#[tauri::command]
async fn get_task_alerts(app: AppHandle) -> Result<hooks::TaskAlerts, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (s, source) = app.state::<AppState>().snapshot();
        hooks::status(source.as_ref(), s.start_wsl, &SystemRunner)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_task_alerts(app: AppHandle, on: bool) -> Result<hooks::TaskAlerts, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (s, source) = state.snapshot();
        hooks::set(source.as_ref(), s.start_wsl, on, &state.events_dir(), &SystemRunner)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The panel was opened and closed again: finished sessions are seen (spec 5.2).
#[tauri::command]
fn sessions_seen(app: AppHandle, state: State<AppState>) -> ViewModel {
    state.sessions_seen();
    emit(&app, None)
}

/// Every second reads new hook event files into the session list (spec 5.2), then emits and
/// toasts with no state lock held. The first scan, at start, picks up files left from before.
fn track_events(app: AppHandle) {
    std::thread::spawn(move || {
        let mut first = true;
        loop {
            scan_events(&app, first);
            first = false;
            std::thread::sleep(EVENTS_POLL);
        }
    });
}

fn scan_events(app: &AppHandle, first: bool) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let taken = events::take_files(&state.events_dir(), first, now);
    let read_any = !taken.is_empty();
    let notices = state.apply_events(taken, now);
    if state.expire_sessions(now) || read_any {
        let flashes: Vec<&str> = notices.iter().map(|n| n.flash.as_str()).collect();
        let announce = (!flashes.is_empty()).then(|| flashes.join(". "));
        emit_flash(app, announce, flashes.last().map(|f| f.to_string()));
    }
    for n in notices {
        toast(app, n.title, n.body);
    }
}

#[tauri::command]
fn list_screens(app: AppHandle) -> Vec<ScreenInfo> {
    let mut monitors = app.available_monitors().unwrap_or_default();
    monitors.sort_by_key(|m| m.position().x);
    let primary = app.primary_monitor().ok().flatten().map(|m| *m.position());
    let friendly = monitors::friendly_names();
    let names: Vec<_> = monitors.iter().map(|m| m.name().and_then(|n| friendly.get(n)).cloned()).collect();
    let mains: Vec<_> = monitors.iter().map(|m| primary == Some(*m.position())).collect();
    monitors::labels(&names, &mains)
        .into_iter()
        .zip(monitors.iter().zip(mains))
        .map(|(label, (m, primary))| ScreenInfo { id: monitor_id(m), label, primary })
        .collect()
}

#[tauri::command]
async fn list_distros() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| source::list_distros(&SystemRunner)).await.unwrap_or_default()
}

/// The newer version on GitHub, if any; the page asks on start and every 6 hours.
#[tauri::command]
async fn check_update() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| update::check(&SystemRunner)).await.ok().flatten()
}

#[tauri::command]
async fn install_update() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| update::install(&SystemRunner, &std::env::temp_dir()))
        .await
        .map_err(|e| e.to_string())?
}

/// Opens the Claude Code install guide in the default browser.
#[tauri::command]
fn open_install_page() -> Result<(), String> {
    // shortcut: URL unverified (R22), owner to check before release.
    std::process::Command::new("explorer.exe")
        .arg("https://docs.claude.com/en/docs/claude-code/setup")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn monitor_id(m: &Monitor) -> String {
    m.name().cloned().unwrap_or_else(|| format!("{},{}", m.position().x, m.position().y))
}

/// Monitors as (id, physical rect, primary), with their scale factors in the same order.
fn screens(app: &AppHandle) -> (Vec<(String, Rect, bool)>, Vec<f64>) {
    let primary = app.primary_monitor().ok().flatten().map(|m| *m.position());
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            let rect = Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 };
            ((monitor_id(m), rect, primary == Some(*p)), m.scale_factor())
        })
        .unzip()
}

/// Puts the fixed-size notch window on the chosen screen at the chosen position (spec 5.7).
/// A missing screen falls back to the primary one. The size is set in physical px of the
/// target monitor so a monitor with another scale does not leave a wrong-sized window.
fn place_window(app: &AppHandle) -> Result<(), String> {
    let (w, h) = WINDOW;
    let win = app.get_webview_window("notch").ok_or("notch window missing")?;
    let (list, scales) = screens(app);
    if list.is_empty() {
        return Err("no monitor".into());
    }
    let (position, wanted) = app
        .try_state::<AppState>()
        .map(|s| s.settings())
        .map_or((settings::Position::Centre, None), |s| (s.position, s.screen));
    let (current, bar) = mode();
    // Taskbar mode always uses the main screen's taskbar (spec 5.5).
    let picked = if current == Mode::Top { pick_monitor(&list, wanted.as_deref()) } else { pick_monitor(&list, None) };
    let scale = scales[list.iter().position(|s| std::ptr::eq(s, picked)).unwrap_or(0)];
    *NOTCH_MONITOR.lock().unwrap_or_else(|e| e.into_inner()) = Some(picked.0.clone());
    let (pw, ph) = ((w * scale).round() as i32, (h * scale).round() as i32);
    let (x, y) = match (current, bar) {
        // The capsule sits at the window's bottom-right, just left of the tray; the panel grows up.
        (Mode::Taskbar | Mode::TaskbarMini, Some(t)) => (t.tray_left - (CAPSULE_MARGIN * scale).round() as i32 - pw, t.bottom - ph),
        _ => notch_origin(picked.1, scale, position, pw),
    };
    #[cfg(debug_assertions)]
    eprintln!("placed logical={w}x{h} physical={pw}x{ph} origin=({x},{y}) monitor={} scale={scale}", picked.0);
    win.set_position(PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
    win.set_size(PhysicalSize::new(pw as u32, ph as u32)).map_err(|e| e.to_string())?;
    // Hit-testing is relative to the webview, which can sit inside an invisible window frame.
    let inner = win.inner_position().map(|p| (p.x, p.y)).unwrap_or((x, y));
    *WINDOW_RECT.lock().unwrap_or_else(|e| e.into_inner()) = Some((Rect { x: inner.0, y: inner.1, w: pw, h: ph }, scale));
    apply_z_order(app);
    Ok(())
}

/// Places the window again. Window calls are queued to the main thread, where they belong.
fn replace(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = place_window(&handle);
    });
}

/// JAKIM zones for the settings dropdown, as (code, places).
#[tauri::command]
fn get_zones() -> Vec<(&'static str, &'static str)> {
    prayer_zones::ZONES.to_vec()
}

/// Wakes the prayer thread now (prayer times turned on, zone or reminder changed).
static PRAYER_KICK: std::sync::OnceLock<std::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

/// "Now" for prayer timing. Debug builds can shift it with NOTCH_PRAYER_TEST_OFFSET_MIN to see an
/// alert without waiting for a real prayer time.
fn prayer_now() -> DateTime<Utc> {
    #[cfg(debug_assertions)]
    if let Some(min) = std::env::var("NOTCH_PRAYER_TEST_OFFSET_MIN").ok().and_then(|v| v.parse::<i64>().ok()) {
        return Utc::now() + chrono::Duration::minutes(min);
    }
    Utc::now()
}

/// Prayer times (prayer spec 5.1-5.3): keeps this and next month cached for the chosen zone
/// (download on a miss, at most once an hour), fires due alerts as a banner plus a notification,
/// and sleeps until the next prayer or reminder moment, at most 60 s at a time so sleep and clock
/// changes are noticed. Does nothing while prayer times are off.
fn track_prayer(app: AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _ = PRAYER_KICK.set(tx);
    std::thread::spawn(move || {
        let mut tried: Option<(String, DateTime<Utc>)> = None; // zone, last download attempt
        loop {
            let state = app.state::<AppState>();
            let s = state.settings();
            let mut wait = Duration::from_secs(60);
            if s.prayer_on {
                let now = prayer_now();
                let dir = state.data_dir();
                let mut days = prayer::load_days(&dir, &s.prayer_zone, now);
                // Before the download, so a new zone never shows with the old zone's times.
                state.set_prayer_days(days.clone());
                let missing = prayer::missing_months(&days, now);
                let retry_due = tried.as_ref().is_none_or(|(z, at)| *z != s.prayer_zone || now - *at >= chrono::Duration::hours(1));
                if !missing.is_empty() && retry_due {
                    tried = Some((s.prayer_zone.clone(), now));
                    for m in missing {
                        if let Some(body) = prayer::fetch_month(&SystemRunner, &s.prayer_zone, m) {
                            let _ = settings::write_atomic(&prayer::cache_path(&dir, &s.prayer_zone, m), &body);
                        }
                    }
                    days = prayer::load_days(&dir, &s.prayer_zone, now);
                    state.set_prayer_days(days);
                }
                let place = prayer_zones::ZONES.iter().find(|(c, _)| *c == s.prayer_zone).map_or("", |(_, p)| *p).to_string();
                let alerts = state.prayer_due(now);
                if alerts.is_empty() {
                    emit(&app, None); // keeps "in 58m" and the progress bar current
                }
                for a in alerts {
                    let (title, banner) = prayer_texts(&a);
                    toast(&app, title, format!("{} · {place}", banner.short.rsplit(" · ").next().unwrap_or("")));
                    emit_with(&app, Some(banner.text.clone()), None, Some(banner));
                }
                if let Some(next) = state.prayer_wake(now) {
                    let ms = (next - now).num_milliseconds().clamp(200, 60_000) as u64;
                    wait = Duration::from_millis(ms);
                }
            }
            let _ = rx.recv_timeout(wait);
        }
    });
}

/// Notification title and notch banner for one prayer alert (exact copy: plan Global Constraints).
fn prayer_texts(a: &prayer::Alert) -> (String, view::Banner) {
    use chrono_tz::Asia::Kuala_Lumpur;
    let hm = |t: DateTime<Utc>| t.with_timezone(&Kuala_Lumpur).format("%H:%M").to_string();
    match a {
        prayer::Alert::Time(p, t) => (
            format!("{} — It's time to pray", p.name()),
            view::Banner { text: format!("{} · It's time to pray · {}", p.name(), hm(*t)), short: format!("{} · {}", p.name(), hm(*t)), seconds: 60 },
        ),
        prayer::Alert::Remind(p, t, m) => (
            format!("{} in {m} min", p.name()),
            view::Banner { text: format!("{} in {m} min · {}", p.name(), hm(*t)), short: format!("{} · {}", p.name(), hm(*t)), seconds: 20 },
        ),
    }
}

/// The webview reports where the visible shape is; only there does the window take the mouse.
#[tauri::command]
fn set_hit_rects(rects: Vec<hit::HitRect>) {
    *HIT_RECTS.lock().unwrap_or_else(|e| e.into_inner()) = rects;
}

/// Turns the window's mouse input on only while the pointer is over the shape, so clicks beside
/// it reach the app below (spec 4, approach A). Polls fast near the window, slowly elsewhere,
/// and tells the webview when the pointer enters or leaves (it gets no mouseleave once input
/// is ignored).
#[cfg(windows)]
fn track_pointer(app: AppHandle) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    std::thread::spawn(move || {
        let mut inside = false;
        loop {
            let mut p = POINT::default();
            let got = unsafe { GetCursorPos(&mut p) }.is_ok();
            let rect = *WINDOW_RECT.lock().unwrap_or_else(|e| e.into_inner());
            let (now_inside, near) = match (got, rect, SHOWN.load(Ordering::Relaxed)) {
                (true, Some((r, scale)), true) => {
                    let (lx, ly) = (f64::from(p.x - r.x) / scale, f64::from(p.y - r.y) / scale);
                    let rects = HIT_RECTS.lock().unwrap_or_else(|e| e.into_inner());
                    let near = p.x >= r.x - 40 && p.x < r.x + r.w + 40 && p.y >= r.y - 40 && p.y < r.y + r.h + 40;
                    (hit::hit(&rects, lx, ly), near)
                }
                _ => (false, false),
            };
            if now_inside != inside {
                inside = now_inside;
                if let Some(w) = app.get_webview_window("notch") {
                    let _ = w.set_ignore_cursor_events(!inside);
                }
                let _ = app.emit("pointer", inside);
            }
            std::thread::sleep(Duration::from_millis(if near { 16 } else { 100 }));
        }
    });
}


/// Gives focus back to the window that had it before the notch was clicked.
#[tauri::command]
fn focus_previous(state: State<AppState>) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        let prev = state.prev_foreground();
        if prev != 0 {
            // Best effort: Windows may refuse if we no longer own the foreground.
            let _ = unsafe { SetForegroundWindow(HWND(prev as *mut _)) };
        }
    }
    #[cfg(not(windows))]
    let _ = state;
}

/// Every 500 ms remembers the foreground window unless it is the notch itself.
#[cfg(windows)]
fn track_foreground(app: AppHandle) {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        let Some(notch) = app.get_webview_window("notch").and_then(|w| w.hwnd().ok()) else {
            continue;
        };
        let fg = unsafe { GetForegroundWindow() }.0 as isize;
        if fg != 0 && fg != notch.0 as isize {
            app.state::<AppState>().set_prev_foreground(fg);
            // Stay behind windows: once another window is in front, sink the notch again (which
            // also drops always-on-top). But while the desktop itself is in front (Show desktop,
            // Win+D) Explorer keeps it above every normal window, so the notch goes always-on-top
            // until another window comes forward.
            if stays_behind(&app) {
                if fullscreen::is_desktop(&fullscreen::foreground_class()) {
                    lift(notch);
                } else {
                    sink(notch);
                }
            }
            // In the taskbar, clicking the taskbar raises it over the capsule: lift ours again.
            if mode().0 != Mode::Top {
                lift(notch);
            }
        }
    });
}

/// "Stay behind windows" is on and applies (Top mode only; the taskbar is always on top).
fn stays_behind(app: &AppHandle) -> bool {
    app.try_state::<AppState>().is_some_and(|s| {
        let s = s.settings();
        s.stay_behind && s.show_in == settings::ShowIn::Top
    })
}

/// Puts the notch at the bottom of the window stack without activating or moving it.
#[cfg(windows)]
fn sink(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};
    let _ = unsafe { SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE) };
}

/// Puts the notch back on top of other topmost windows (the taskbar) without activating it.
#[cfg(windows)]
fn lift(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};
    let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE) };
}

/// True when the taskbar sits above `notch` in the window stack.
#[cfg(windows)]
fn taskbar_above(notch: windows::Win32::Foundation::HWND) -> bool {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindow, GW_HWNDPREV};
    let Ok(tb) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }) else { return false };
    let mut h = unsafe { GetWindow(notch, GW_HWNDPREV) };
    while let Ok(w) = h {
        if w == tb {
            return true;
        }
        h = unsafe { GetWindow(w, GW_HWNDPREV) };
    }
    false
}

/// In the taskbar, clicking the taskbar (e.g. Show desktop) raises it over the capsule, and it
/// raises itself again after the click with no event to hook. Checking every 50 ms and lifting
/// only when it is above keeps the capsule from vanishing for half a second.
#[cfg(windows)]
fn keep_above_taskbar(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(if mode().0 == Mode::Top { 500 } else { 50 }));
        if mode().0 == Mode::Top {
            continue;
        }
        if let Some(h) = app.get_webview_window("notch").and_then(|w| w.hwnd().ok()) {
            if taskbar_above(h) {
                lift(h);
            }
        }
    });
}

/// Always on top, or at the bottom when "Stay behind windows" applies (spec 5.6).
fn apply_z_order(app: &AppHandle) {
    let Some(win) = app.get_webview_window("notch") else { return };
    let behind = stays_behind(app);
    let _ = win.set_always_on_top(!behind);
    #[cfg(windows)]
    if behind {
        if let Ok(h) = win.hwnd() {
            sink(h);
        }
    }
}

/// Shows or hides the notch to match `show_notch` and the full-screen state, only on a change.
/// ShowWindow(SW_SHOWNOACTIVATE) so showing never steals focus (spec 5.7); no lock is held here.
fn apply_visibility(app: &AppHandle) {
    let show_notch = app.try_state::<AppState>().is_none_or(|s| s.settings().show_notch);
    let fullscreen = FULLSCREEN.load(Ordering::Relaxed);
    let want = fullscreen::should_show(show_notch, fullscreen);
    if SHOWN.load(Ordering::Relaxed) == want {
        return;
    }
    #[cfg(windows)]
    {
        // Look the window up first: if that fails, SHOWN stays put and the next tick retries.
        let Some(hwnd) = app.get_webview_window("notch").and_then(|w| w.hwnd().ok()) else {
            return;
        };
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};
        let _ = unsafe { ShowWindow(hwnd, if want { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
    }
    SHOWN.store(want, Ordering::Relaxed);
    #[cfg(all(debug_assertions, windows))]
    eprintln!("notch visible={want} show_notch={show_notch} fullscreen={fullscreen} foreground_class={}", fullscreen::foreground_class());
}

/// Every second checks for a full-screen app on the notch's monitor and applies the result.
/// On a single-monitor setup the D3D full-screen hint counts too (spec 5.6).
#[cfg(windows)]
fn track_fullscreen(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let device = NOTCH_MONITOR.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let hwnd = app.get_webview_window("notch").and_then(|w| w.hwnd().ok());
        if let (Some(device), Some(hwnd)) = (device, hwnd) {
            let single = app.available_monitors().is_ok_and(|m| m.len() == 1);
            let full = fullscreen::foreground_fullscreen_on(&device, hwnd.0 as isize)
                || (single && fullscreen::quns_d3d_fullscreen());
            FULLSCREEN.store(full, Ordering::Relaxed);
        }
        apply_visibility(&app);
    });
}

fn main() {
    tauri::Builder::default()
        // Must be the first plugin. A second launch just exits; the running copy carries on.
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .on_window_event(|window, event| {
            if let WindowEvent::ScaleFactorChanged { .. } = event {
                let app = window.app_handle().clone();
                std::thread::spawn(move || replace(&app));
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_view,
            refresh_now,
            get_settings,
            save_settings,
            list_screens,
            set_hit_rects,
            get_zones,
            check_update,
            install_update,
            focus_previous,
            list_distros,
            open_install_page,
            get_task_alerts,
            set_task_alerts,
            sessions_seen
        ])
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            app.manage(AppState::new(settings::load(&path), path));
            // The hooks only write into an existing folder, so it must exist before they fire.
            if let Err(e) = std::fs::create_dir_all(app.state::<AppState>().events_dir()) {
                eprintln!("hooks: events folder: {e}");
            }
            tray::build(app.handle())?;
            track_events(app.handle().clone());
            refresh_hooks(app.handle().clone());
            apply_visibility(app.handle()); // show_notch=false must not flash the notch at launch
            #[cfg(debug_assertions)]
            if std::env::var_os("NOTCH_TEST_TOAST").is_some() {
                let now = Local::now();
                let sample = AlertEvent::Crossed(alerts::LimitKind::Session, 91.0, chrono::Utc::now() + chrono::Duration::minutes(90));
                let (title, body) = view::toast_text(&sample, now);
                let r = app.notification().builder().title(title).body(body).show();
                eprintln!("test toast show() -> {r:?}");
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                let last = *LAST_REFRESH_START.lock().unwrap_or_else(|e| e.into_inner());
                if due(Utc::now(), last) {
                    refresh(&handle);
                }
                std::thread::sleep(REFRESH_CHECK);
            });
            // Places at start, then again whenever a monitor is plugged, unplugged or changed.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut seen = None;
                loop {
                    let now = screens(&handle);
                    if seen.as_ref() != Some(&now) {
                        replace(&handle);
                        seen = Some(now);
                    }
                    std::thread::sleep(MONITOR_POLL);
                }
            });
            #[cfg(windows)]
            {
                if let Some(w) = app.get_webview_window("notch") {
                    let _ = w.set_ignore_cursor_events(true); // until the pointer is over the shape
                }
                track_pointer(app.handle().clone());
                track_taskbar(app.handle().clone());
                track_prayer(app.handle().clone());
                keep_above_taskbar(app.handle().clone());
                track_foreground(app.handle().clone());
                track_fullscreen(app.handle().clone());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Claude Notch");
}

#[cfg(test)]
mod tests {
    use super::{due, prayer, prayer_texts, still_showing, view};
    use chrono::{Duration, Utc};

    #[test]
    fn refresh_is_due_after_five_minutes_of_wall_clock() {
        let t = Utc::now();
        assert!(!due(t + Duration::seconds(299), t));
        assert!(due(t + Duration::seconds(300), t));
        assert!(due(t + Duration::hours(8), t)); // woke from sleep
        assert!(due(t - Duration::hours(1), t)); // clock moved back
        assert!(due(t, chrono::DateTime::<Utc>::MIN_UTC)); // never refreshed
    }

    #[test]
    fn prayer_alert_copy() {
        use chrono::TimeZone;
        let t = chrono_tz::Asia::Kuala_Lumpur.with_ymd_and_hms(2026, 11, 4, 18, 58, 0).unwrap().with_timezone(&Utc);
        let (title, b) = prayer_texts(&prayer::Alert::Time(prayer::Prayer::Maghrib, t));
        assert_eq!(title, "Maghrib — It's time to pray");
        assert_eq!((b.text.as_str(), b.short.as_str(), b.seconds), ("Maghrib · It's time to pray · 18:58", "Maghrib · 18:58", 60));
        let (title, b) = prayer_texts(&prayer::Alert::Remind(prayer::Prayer::Maghrib, t, 10));
        assert_eq!(title, "Maghrib in 10 min");
        assert_eq!((b.text.as_str(), b.seconds), ("Maghrib in 10 min · 18:58", 20));
    }

    #[test]
    fn banner_fired_before_the_page_listens_is_still_shown() {
        let b = view::Banner { text: "Subuh in 10 min · 05:48".into(), short: "Subuh · 05:48".into(), seconds: 20 };
        assert_eq!(still_showing(&b, std::time::Duration::from_millis(2500)).map(|x| x.seconds), Some(18));
        assert_eq!(still_showing(&b, std::time::Duration::from_secs(19)).map(|x| x.seconds), Some(1));
        assert!(still_showing(&b, std::time::Duration::from_secs(20)).is_none());
    }
}
