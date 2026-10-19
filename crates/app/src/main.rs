//! FormatCraft desktop app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod app;
mod dialogs;
mod i18n;
mod icons;

use eframe::egui;

static ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/formatcraft-256.png");

fn icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
    Some(egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("FormatCraft")
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([760.0, 480.0])
        .with_drag_and_drop(true)
        .with_app_id("formatcraft");
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "FormatCraft",
        options,
        Box::new(|cc| Ok(Box::new(app::FormatCraftApp::new(cc)))),
    )
}
