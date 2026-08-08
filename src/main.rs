#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use easy_agent::{APP_ID, APP_NAME, app::InstallerApp};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_icon(application_icon())
            .with_inner_size([840.0, 660.0])
            .with_min_inner_size([760.0, 620.0])
            .with_max_inner_size([980.0, 760.0]),
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|creation_context| Ok(Box::new(InstallerApp::new(creation_context)))),
    )
}

fn application_icon() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/branding/easy-agent-icon-512.png"))
        .expect("the bundled easy agent icon must be a valid PNG")
}
