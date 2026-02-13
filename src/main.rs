mod app;
mod config;
mod index;
mod indexer;
mod types;

use eframe::egui;

fn load_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory(icon_bytes)
        .expect("Failed to load icon")
        .into_rgba8();
    let (w, h) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    }
}

fn main() -> eframe::Result<()> {
    let icon = load_icon();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("drozoSearch")
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "drozoSearch",
        options,
        Box::new(|cc| Ok(Box::new(app::DrozoSearchApp::new(cc)))),
    )
}
