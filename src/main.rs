#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod analytics;
mod app;
mod auth;
mod db;
mod import;
mod models;
mod notes;
mod sms;
mod theme;
mod ui;

use app::CrustomApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([1040.0, 680.0])
            .with_title("Crustom — Billing Intelligence"),
        ..Default::default()
    };

    eframe::run_native(
        "Crustom — Billing Intelligence",
        options,
        Box::new(|cc| Ok(Box::new(CrustomApp::new(cc)))),
    )
}
