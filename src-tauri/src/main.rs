#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod app_state;
mod fullscreen;
mod pace;
mod placement;
mod settings;
mod source;
mod tray;
mod usage;
mod view;

use alerts::AlertEvent;
use app_state::AppState;
use chrono::Local;
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
const CLOSED_SIZE: (f64, f64) = (220.0, 36.0);
const OPEN_WIDTH: f64 = 320.0;
const MONITOR_POLL: Duration = Duration::from_secs(2);

/// Last logical size placed, so a display change can re-place the window without the webview.
/// Held for the whole of `place_window`, and `place_window` only runs on the main thread, so a
/// re-place can never overwrite a newer size with a stale one.
static LAST_SIZE: Mutex<(f64, f64)> = Mutex::new(CLOSED_SIZE);

/// Device name of the monitor `place_window` last put the notch on (what full screen is judged against).
static NOTCH_MONITOR: Mutex<Option<String>> = Mutex::new(None);
/// A full-screen app is on the notch's monitor, as of the last 1 s check.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// Whether the notch window is shown right now; the window starts visible.
static SHOWN: AtomicBool = AtomicBool::new(true);

#[derive(Serialize)]
struct ScreenInfo {
    id: String,
    label: String,
    primary: bool,
}

/// One toast per event (spec 5.5); shown even while the notch is hidden.
fn on_alert(app: &AppHandle, events: Vec<AlertEvent>) {
    for e in &events {
        let (title, body) = view::toast_text(e, Local::now());
        if let Err(err) = app.notification().builder().title(title).body(body).show() {
            eprintln!("toast failed: {err}");
        }
    }
}

/// Titles of the events joined for screen readers, or None when there are none.
fn announce(events: &[AlertEvent]) -> Option<String> {
    let now = Local::now();
    let titles: Vec<String> = events.iter().map(|e| view::toast_text(e, now).0).collect();
    (!titles.is_empty()).then(|| titles.join(". "))
}

fn emit(app: &AppHandle, announce: Option<String>) -> ViewModel {
    let vm = app.state::<AppState>().view(Local::now(), announce);
    #[cfg(debug_assertions)]
    eprintln!("usage-updated {}", serde_json::to_string(&vm).unwrap_or_default());
    let _ = app.emit("usage-updated", &vm);
    tray::update(app, &vm);
    vm
}

/// One refresh cycle; ignored while another is running. Runs again once if a source change
/// arrived meanwhile.
fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.try_start_refresh() {
        return;
    }
    loop {
        emit(app, None);
        // A panic must still release the in-flight slot.
        let result = catch_unwind(AssertUnwindSafe(|| app_state::fetch(&state, &SystemRunner))).ok().flatten();
        let events = state.finish_refresh(result);
        emit(app, announce(&events));
        on_alert(app, events);
        if !state.take_refresh_again() || !state.try_start_refresh() {
            break;
        }
    }
}

#[tauri::command]
fn get_view(state: State<AppState>) -> ViewModel {
    state.view(Local::now(), None)
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
    let (source_changed, events) = state.save_settings(settings).map_err(|e| e.to_string())?;
    replace(&app);
    let vm = emit(&app, announce(&events));
    on_alert(&app, events);
    if source_changed {
        refresh_now(app);
    }
    Ok(vm)
}

#[tauri::command]
fn list_screens(app: AppHandle) -> Vec<ScreenInfo> {
    let mut monitors = app.available_monitors().unwrap_or_default();
    monitors.sort_by_key(|m| m.position().x);
    let primary = app.primary_monitor().ok().flatten().map(|m| *m.position());
    monitors
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let is_primary = primary == Some(*m.position());
            ScreenInfo {
                id: monitor_id(m),
                label: format!("Screen {}{}", i + 1, if is_primary { " (main)" } else { "" }),
                primary: is_primary,
            }
        })
        .collect()
}

#[tauri::command]
async fn list_distros() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| source::list_distros(&SystemRunner)).await.unwrap_or_default()
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

/// Puts the notch, `w` x `h` logical px, on the chosen screen at the chosen position (spec 5.7).
/// A missing screen falls back to the primary one. The size is set in physical px of the
/// target monitor so a monitor with another scale does not leave a wrong-sized window.
/// `size` None re-places at the size stored under the lock.
fn place_window(app: &AppHandle, size: Option<(f64, f64)>) -> Result<(), String> {
    let mut last = LAST_SIZE.lock().unwrap_or_else(|e| e.into_inner());
    let (w, h) = remember_size(&mut last, size);
    let win = app.get_webview_window("notch").ok_or("notch window missing")?;
    let (list, scales) = screens(app);
    if list.is_empty() {
        return Err("no monitor".into());
    }
    let (position, wanted) = app
        .try_state::<AppState>()
        .map(|s| s.settings())
        .map_or((settings::Position::Centre, None), |s| (s.position, s.screen));
    let picked = pick_monitor(&list, wanted.as_deref());
    let scale = scales[list.iter().position(|s| std::ptr::eq(s, picked)).unwrap_or(0)];
    *NOTCH_MONITOR.lock().unwrap_or_else(|e| e.into_inner()) = Some(picked.0.clone());
    let (pw, ph) = ((w * scale).round() as i32, (h * scale).round() as i32);
    let (x, y) = notch_origin(picked.1, scale, position, pw);
    #[cfg(debug_assertions)]
    eprintln!("placed logical={w}x{h} physical={pw}x{ph} origin=({x},{y}) monitor={} scale={scale}", picked.0);
    win.set_position(PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
    win.set_size(PhysicalSize::new(pw as u32, ph as u32)).map_err(|e| e.to_string())
}

/// Stores `given` if there is one and returns the size to place.
fn remember_size(slot: &mut (f64, f64), given: Option<(f64, f64)>) -> (f64, f64) {
    if let Some(size) = given {
        *slot = size;
    }
    *slot
}

/// Places the window again at its last logical size. Window calls under the `LAST_SIZE` lock
/// need the main thread, which may itself be waiting for that lock in `set_panel_open`, so the
/// work is queued to the main thread rather than run on the caller's thread.
fn replace(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let _ = place_window(&handle, None);
    });
}

/// Resizes the notch for the panel, anchored to its position setting. Closed, the window
/// takes the pill's measured `width` (its text changes with the state, spec 5.2/5.7).
#[tauri::command]
fn set_panel_open(app: AppHandle, open: bool, height: f64, width: Option<f64>) -> Result<(), String> {
    let (w, h) = if open {
        (OPEN_WIDTH, height.clamp(36.0, 600.0))
    } else {
        let w = width.filter(|w| w.is_finite()).unwrap_or(CLOSED_SIZE.0);
        (w.clamp(36.0, 480.0), CLOSED_SIZE.1)
    };
    place_window(&app, Some((w, h)))
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
        }
    });
}

/// Shows or hides the notch to match `show_notch` and the full-screen state, only on a change.
/// ShowWindow(SW_SHOWNOACTIVATE) so showing never steals focus (spec 5.7); no lock is held here.
fn apply_visibility(app: &AppHandle) {
    let show_notch = app.try_state::<AppState>().is_none_or(|s| s.settings().show_notch);
    let fullscreen = FULLSCREEN.load(Ordering::Relaxed);
    let want = fullscreen::should_show(show_notch, fullscreen);
    if SHOWN.swap(want, Ordering::Relaxed) == want {
        return;
    }
    #[cfg(debug_assertions)]
    #[cfg(windows)]
    eprintln!("notch visible={want} show_notch={show_notch} fullscreen={fullscreen} foreground_class={}", fullscreen::foreground_class());
    #[cfg(windows)]
    if let Some(hwnd) = app.get_webview_window("notch").and_then(|w| w.hwnd().ok()) {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};
        let _ = unsafe { ShowWindow(hwnd, if want { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
    }
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
    SHOWN.store(want, Ordering::Relaxed);
    #[cfg(all(debug_assertions, windows))]
    eprintln!("notch visible={want} show_notch={show_notch} fullscreen={fullscreen} foreground_class={}", fullscreen::foreground_class());
            let full = fullscreen::foreground_fullscreen_on(&device, hwnd.0 as isize)
                || (single && fullscreen::quns_d3d_fullscreen());
            FULLSCREEN.store(full, Ordering::Relaxed);
        }
        apply_visibility(&app);
    });
}

fn main() {
    tauri::Builder::default()
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
            set_panel_open,
            focus_previous,
            list_distros,
            open_install_page
        ])
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            app.manage(AppState::new(settings::load(&path), path));
            tray::build(app.handle())?;
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
                refresh(&handle);
                std::thread::sleep(REFRESH_EVERY);
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
    use super::remember_size;

    #[test]
    fn re_place_uses_the_stored_size_and_a_new_size_replaces_it() {
        let mut slot = (220.0, 36.0);
        assert_eq!(remember_size(&mut slot, None), (220.0, 36.0));
        assert_eq!(remember_size(&mut slot, Some((320.0, 300.0))), (320.0, 300.0));
        assert_eq!(remember_size(&mut slot, None), (320.0, 300.0));
    }
}
