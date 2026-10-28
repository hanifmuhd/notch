#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[allow(dead_code)]
mod alerts;
#[allow(dead_code)]
mod pace;
#[allow(dead_code)]
mod settings;
#[allow(dead_code)]
mod source;
#[allow(dead_code)]
mod usage;
#[allow(dead_code)]
mod view;

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running Claude Notch");
}
