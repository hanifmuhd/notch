//! System tray: tooltip with the current numbers and the menu (spec 5.8). Built in `setup`.

use crate::app_state::AppState;
use crate::view::{tray_toggle_label, tray_tooltip, ViewModel};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

/// Handles kept so the tooltip and the Show/Hide text can follow the app.
struct Tray {
    icon: TrayIcon,
    toggle: MenuItem<tauri::Wry>,
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show_notch = app.state::<AppState>().settings().show_notch;
    let toggle = MenuItem::with_id(app, "toggle", tray_toggle_label(show_notch), true, None::<&str>)?;
    // Default off; the box shows what Windows really has registered.
    let on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(app, "autostart", "Start with Windows", true, on, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &autostart, &refresh, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut builder = TrayIconBuilder::new().tooltip("Claude Notch").menu(&menu).on_menu_event({
        let autostart = autostart.clone();
        move |app, event| match event.id().as_ref() {
            "toggle" => toggle_notch(app),
            "autostart" => switch_autostart(app, &autostart),
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
    app.manage(Tray { icon, toggle });
    Ok(())
}

/// Keeps the tooltip and the Show/Hide text current; called whenever the view is emitted.
pub fn update(app: &AppHandle, vm: &ViewModel) {
    let (Some(tray), Some(state)) = (app.try_state::<Tray>(), app.try_state::<AppState>()) else { return };
    let _ = tray.icon.set_tooltip(Some(tray_tooltip(vm)));
    let _ = tray.toggle.set_text(tray_toggle_label(state.settings().show_notch));
}

/// Flips `show_notch` through the same save command the webview uses, then applies it at once.
fn toggle_notch(app: &AppHandle) {
    let mut settings = app.state::<AppState>().settings();
    settings.show_notch = !settings.show_notch;
    if let Err(e) = crate::save_settings(app.clone(), app.state(), settings) {
        eprintln!("tray: save show_notch failed: {e}");
        return;
    }
    crate::apply_visibility(app);
    // The webview keeps its own copy of the settings and would otherwise save the old show_notch back.
    let _ = app.emit("settings-changed", ());
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
