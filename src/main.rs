#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "android"))]
fn main() -> eframe::Result<()> {
    fiksiarski_game_hub::run_desktop()
}

#[cfg(target_os = "android")]
fn main() {}