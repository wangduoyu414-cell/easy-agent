#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ai_client_installer::app::InstallerApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([920.0, 700.0])
            .with_min_inner_size([760.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "AI 客户端安装助手",
        options,
        Box::new(|creation_context| Ok(Box::new(InstallerApp::new(creation_context)))),
    )
}
