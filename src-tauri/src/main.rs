#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod app_state;
mod pace;
mod placement;
mod settings;
mod source;
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
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, State, WindowEvent};
use view::ViewModel;

const REFRESH_EVERY: Duration = Duration::from_secs(300);
const CLOSED_SIZE: (f64, f64) = (220.0, 36.0);
const OPEN_WIDTH: f64 = 320.0;
const MONITOR_POLL: Duration = Duration::from_secs(2);

/// Last logical size placed, so a display change can re-place the window without the webview.
static LAST_SIZE: Mutex<(f64, f64)> = Mutex::new(CLOSED_SIZE);

#[derive(Serialize)]
struct ScreenInfo {
    id: String,
    label: String,
    primary: bool,
}

/// Hook for Task 14 (toasts); events are already folded into `announce` by the caller.
fn on_alert(_events: Vec<AlertEvent>) {}

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
        on_alert(events);
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
    on_alert(events);
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
fn place_window(app: &AppHandle, w: f64, h: f64) -> Result<(), String> {
    *LAST_SIZE.lock().unwrap_or_else(|e| e.into_inner()) = (w, h);
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
    let (pw, ph) = ((w * scale).round() as i32, (h * scale).round() as i32);
    let (x, y) = notch_origin(picked.1, scale, position, pw);
    #[cfg(debug_assertions)]
    eprintln!("placed logical={w}x{h} physical={pw}x{ph} origin=({x},{y}) monitor={} scale={scale}", picked.0);
    win.set_position(PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
    win.set_size(PhysicalSize::new(pw as u32, ph as u32)).map_err(|e| e.to_string())
}

/// Places the window again at its last logical size.
fn replace(app: &AppHandle) {
    let (w, h) = *LAST_SIZE.lock().unwrap_or_else(|e| e.into_inner());
    let _ = place_window(app, w, h);
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
    place_window(&app, w, h)
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

fn main() {
    tauri::Builder::default()
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
            track_foreground(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Claude Notch");
}
