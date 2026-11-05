//! System tray: tooltip with the current numbers and the menu (spec 5.8). Built in `setup`.

use crate::app_state::AppState;
use crate::view::{tray_toggle_label, tray_tooltip, ViewModel};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

/// Handles kept so the tooltip and the Show/Hide text can follow the app.
struct Tray {
    icon: TrayIcon,
    toggle: MenuItem<tauri::Wry>,
    /// macOS: the menu's info lines and update version last shown, so it is rebuilt only on change.
    #[cfg(target_os = "macos")]
    shown: std::sync::Mutex<(Vec<String>, Option<String>)>,
}

/// The experimental macOS menu: info lines, then the actions (no notch to hide yet).
#[cfg(target_os = "macos")]
fn mac_menu(app: &AppHandle, lines: &[String], update: Option<&str>) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    for (i, line) in lines.iter().enumerate() {
        menu.append(&MenuItem::with_id(app, format!("line{i}"), line, false, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    if let Some(v) = update {
        menu.append(&MenuItem::with_id(app, "update", format!("Update to {v}"), true, None::<&str>)?)?;
    }
    let on = app.autolaunch().is_enabled().unwrap_or(false);
    menu.append(&CheckMenuItem::with_id(app, "autostart", "Open at login", true, on, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
    Ok(menu)
}

#[cfg(target_os = "macos")]
fn rebuild_mac_menu(app: &AppHandle, tray: &Tray) {
    let shown = tray.shown.lock().unwrap_or_else(|e| e.into_inner());
    match mac_menu(app, &shown.0, shown.1.as_deref()) {
        Ok(menu) => {
            let _ = tray.icon.set_menu(Some(menu));
        }
        Err(e) => eprintln!("tray: macOS menu: {e}"),
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show_notch = app.state::<AppState>().settings().show_notch;
    let toggle = MenuItem::with_id(app, "toggle", tray_toggle_label(show_notch), true, None::<&str>)?;
    // Default off; the box shows what Windows really has registered.
    let on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(app, "autostart", "Start with Windows", true, on, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    #[cfg(not(target_os = "macos"))]
    let menu = Menu::with_items(app, &[&toggle, &autostart, &settings, &refresh, &PredefinedMenuItem::separator(app)?, &quit])?;
    #[cfg(target_os = "macos")]
    let menu = mac_menu(app, &[], None)?;

    let mut builder = TrayIconBuilder::new().tooltip("Claude Notch").menu(&menu).on_menu_event({
        let autostart = autostart.clone();
        move |app, event| match event.id().as_ref() {
            "toggle" => toggle_notch(app),
            "autostart" => {
                switch_autostart(app, &autostart);
                #[cfg(target_os = "macos")]
                if let Some(tray) = app.try_state::<Tray>() {
                    rebuild_mac_menu(app, &tray); // the macOS menu holds its own check box
                }
            }
            "update" => {
                std::thread::spawn(|| {
                    if let Err(e) = crate::update::install(&crate::source::SystemRunner, &std::env::temp_dir()) {
                        eprintln!("tray: update failed: {e}");
                    }
                });
            }
            "settings" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::show_settings(&app) {
                        eprintln!("tray: open settings failed: {e}");
                    }
                });
            }
            "refresh" => crate::refresh_now(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        }
    });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder.build(app)?;
    #[cfg(debug_assertions)]
    eprintln!("tray created");
    app.manage(Tray {
        icon,
        toggle,
        #[cfg(target_os = "macos")]
        shown: std::sync::Mutex::new((Vec::new(), None)),
    });
    Ok(())
}

/// Keeps the tooltip and the Show/Hide text current; called whenever the view is emitted.
pub fn update(app: &AppHandle, vm: &ViewModel) {
    let (Some(tray), Some(state)) = (app.try_state::<Tray>(), app.try_state::<AppState>()) else { return };
    let _ = tray.icon.set_tooltip(Some(tray_tooltip(vm)));
    let _ = tray.toggle.set_text(tray_toggle_label(state.settings().show_notch));
    #[cfg(target_os = "macos")]
    {
        let _ = tray.icon.set_title(Some(crate::view::menu_title(vm)));
        let want = (crate::view::menu_lines(vm), crate::LATEST_UPDATE.lock().unwrap_or_else(|e| e.into_inner()).clone());
        let changed = {
            let mut shown = tray.shown.lock().unwrap_or_else(|e| e.into_inner());
            let changed = *shown != want;
            *shown = want;
            changed
        };
        if changed {
            rebuild_mac_menu(app, &tray);
        }
    }
}

/// Flips `show_notch` through the same save command the webview uses, then applies it at once.
fn toggle_notch(app: &AppHandle) {
    let mut settings = app.state::<AppState>().settings();
    settings.show_notch = !settings.show_notch;
    if let Err(e) = crate::save_settings(app.clone(), app.state(), settings) {
        eprintln!("tray: save show_notch failed: {e}");
        return;
    }
    crate::apply_visibility(app); // save_settings already told both pages to reload their copy
}

fn switch_autostart(app: &AppHandle, item: &CheckMenuItem<tauri::Wry>) {
    let auto = app.autolaunch();
    let result = if auto.is_enabled().unwrap_or(false) { auto.disable() } else { auto.enable() };
    if let Err(e) = result {
        eprintln!("tray: autostart change failed: {e}");
    }
    // The click already flipped the box; show what Windows really has.
    let _ = item.set_checked(auto.is_enabled().unwrap_or(false));
}
