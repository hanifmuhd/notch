#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod app_state;
mod pace;
mod settings;
mod source;
mod usage;
mod view;

use alerts::AlertEvent;
use app_state::AppState;
use chrono::{Local, Utc};
use serde::Serialize;
use settings::Settings;
use source::{detect, usage_command, wsl_running, Runner, Source, SystemRunner, CLI_TIMEOUT};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State};
use usage::{parse_usage, UsageReading};
use view::ViewModel;

const REFRESH_EVERY: Duration = Duration::from_secs(300);
const CLOSED_SIZE: (f64, f64) = (220.0, 36.0);
const OPEN_WIDTH: f64 = 320.0;

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

/// Runs the CLI (blocking, 6-9 s on WSL). None means the call failed and the reading is stale.
fn fetch(state: &AppState) -> Option<UsageReading> {
    let runner = SystemRunner;
    let settings = state.settings();
    let mut source = state.source();
    // NotFound is never cached: Claude may have been installed since the last tick.
    if source == Source::NotFound {
        source = detect(&runner, &settings.source);
        state.set_source(source.clone());
    }
    if let Source::Wsl { distro } = &source {
        if !settings.start_wsl && !wsl_running(&runner, distro) {
            return None;
        }
    }
    if source == Source::NotFound {
        return None;
    }
    let (program, args) = usage_command(&source);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let raw = runner.run(&program, &args, CLI_TIMEOUT).ok()?;
    parse_usage(&raw, Utc::now()).ok()
}

/// One refresh cycle; ignored while another is running.
fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.try_start_refresh() {
        return;
    }
    emit(app, None);
    // A panic must still release the in-flight slot.
    let result = catch_unwind(AssertUnwindSafe(|| fetch(&state))).ok().flatten();
    let events = state.finish_refresh(result);
    emit(app, announce(&events));
    on_alert(events);
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
                id: m.name().cloned().unwrap_or_else(|| format!("{},{}", m.position().x, m.position().y)),
                label: format!("Screen {}{}", i + 1, if is_primary { " (main)" } else { "" }),
                primary: is_primary,
            }
        })
        .collect()
}

/// Resizes the notch for the panel, keeping the top-centre point fixed.
#[tauri::command]
fn set_panel_open(app: AppHandle, open: bool, height: f64) -> Result<(), String> {
    let win = app.get_webview_window("notch").ok_or("notch window missing")?;
    let (w, h) = if open { (OPEN_WIDTH, height.clamp(36.0, 600.0)) } else { CLOSED_SIZE };
    let scale = win.scale_factor().map_err(|e| e.to_string())?;
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let old = win.outer_size().map_err(|e| e.to_string())?;
    let new_w = (w * scale).round() as i32;
    let centre = pos.x + old.width as i32 / 2;
    win.set_size(LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
    win.set_position(PhysicalPosition::new(centre - new_w / 2, pos.y)).map_err(|e| e.to_string())
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
        .invoke_handler(tauri::generate_handler![
            get_view,
            refresh_now,
            get_settings,
            save_settings,
            list_screens,
            set_panel_open,
            focus_previous
        ])
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            app.manage(AppState::new(settings::load(&path), path));
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                refresh(&handle);
                std::thread::sleep(REFRESH_EVERY);
            });
            #[cfg(windows)]
            track_foreground(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Claude Notch");
}
