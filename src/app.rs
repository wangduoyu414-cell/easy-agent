use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText};

use crate::adapters::resolve_latest;
use crate::core::{
    Detection, OperationUpdate, PlatformInfo, ProductId, ProductOperationResult, ProductView,
    ReleaseCandidate, SupportState, TrustRegistry, run_install_batch, version_is_older,
};
use crate::platform::{current_platform, detect_product};

enum UiEvent {
    Detection(ProductId, Detection),
    Resolution(ProductId, Result<ReleaseCandidate, String>),
    Operation(OperationUpdate),
    BatchFinished(Vec<ProductOperationResult>),
    ScanFinished,
}

pub struct InstallerApp {
    platform: PlatformInfo,
    registry: Option<TrustRegistry>,
    products: Vec<ProductView>,
    event_sender: Sender<UiEvent>,
    event_receiver: Receiver<UiEvent>,
    scanning: bool,
    batch_running: bool,
    confirmation_open: bool,
    cancel_flag: Arc<AtomicBool>,
    batch_summary: Option<String>,
    registry_error: Option<String>,
}

impl InstallerApp {
    pub fn new(context: &eframe::CreationContext<'_>) -> Self {
        context.egui_ctx.set_visuals(egui::Visuals::light());
        install_system_font(&context.egui_ctx);
        let platform = current_platform();
        let registry = TrustRegistry::embedded();
        let registry_error = registry.as_ref().err().map(ToString::to_string);
        let products = ProductId::ALL
            .into_iter()
            .map(|product| {
                let support = registry
                    .as_ref()
                    .map(|registry| {
                        registry.support_state(product, platform.os, platform.architecture)
                    })
                    .unwrap_or_else(|error| {
                        SupportState::Disabled(format!("信任注册表不可用：{error}"))
                    });
                ProductView {
                    product,
                    selected: false,
                    support,
                    detection: Detection::absent("尚未检测"),
                    latest: None,
                    status_line: "等待检测".into(),
                    staged_file: None,
                }
            })
            .collect();
        let (event_sender, event_receiver) = unbounded();
        let mut app = Self {
            platform,
            registry: registry.ok(),
            products,
            event_sender,
            event_receiver,
            scanning: false,
            batch_running: false,
            confirmation_open: false,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            batch_summary: None,
            registry_error,
        };
        app.start_scan();
        app
    }

    fn start_scan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        for product in &mut self.products {
            product.status_line = "正在检测安装状态与官方版本…".into();
        }
        let sender = self.event_sender.clone();
        let platform = self.platform.clone();
        let registry = self.registry.clone();
        thread::spawn(move || {
            for product in ProductId::ALL {
                let detection = detect_product(product);
                if sender.send(UiEvent::Detection(product, detection)).is_err() {
                    return;
                }
                let resolution = registry
                    .as_ref()
                    .ok_or_else(|| "信任注册表不可用".to_owned())
                    .and_then(|registry| {
                        resolve_latest(product, &platform, registry)
                            .map_err(|error| error.to_string())
                    });
                if sender
                    .send(UiEvent::Resolution(product, resolution))
                    .is_err()
                {
                    return;
                }
            }
            let _ = sender.send(UiEvent::ScanFinished);
        });
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.event_receiver.try_recv() {
            match event {
                UiEvent::Detection(product, detection) => {
                    if let Some(view) = self
                        .products
                        .iter_mut()
                        .find(|view| view.product == product)
                    {
                        view.status_line = if detection.installed {
                            let management = if detection.managed {
                                " · 受组织管理"
                            } else if !detection.management_known {
                                " · 管理状态未知（不会自动覆盖）"
                            } else {
                                ""
                            };
                            match &detection.version {
                                Some(version) if !version.is_empty() => {
                                    format!("已安装 {version} · {}{management}", detection.evidence)
                                }
                                _ => format!("已安装 · {}{management}", detection.evidence),
                            }
                        } else {
                            format!("未检测到安装 · {}", detection.evidence)
                        };
                        view.detection = detection;
                    }
                }
                UiEvent::Resolution(product, resolution) => {
                    if let Some(view) = self
                        .products
                        .iter_mut()
                        .find(|view| view.product == product)
                    {
                        match resolution {
                            Ok(candidate) => {
                                view.status_line.push_str(&format!(
                                    " · 官方最新 {} ({:?})",
                                    candidate.version, candidate.architecture
                                ));
                                view.latest = Some(candidate);
                            }
                            Err(error) => {
                                view.status_line.push_str(&format!(" · 版本解析：{error}"));
                                view.latest = None;
                            }
                        }
                    }
                }
                UiEvent::Operation(update) => {
                    if let Some(view) = self
                        .products
                        .iter_mut()
                        .find(|view| view.product == update.product)
                    {
                        view.status_line = format!("{} · {}", update.state.label(), update.message);
                    }
                }
                UiEvent::BatchFinished(results) => {
                    self.batch_running = false;
                    let succeeded = results
                        .iter()
                        .filter(|result| result.state == crate::core::OperationState::Succeeded)
                        .count();
                    let failed = results
                        .iter()
                        .filter(|result| result.state == crate::core::OperationState::Failed)
                        .count();
                    let cancelled = results.len().saturating_sub(succeeded + failed);
                    self.batch_summary = Some(format!(
                        "批次完成：成功 {succeeded}，失败 {failed}，取消 {cancelled}"
                    ));
                    for view in &mut self.products {
                        view.selected = false;
                    }
                }
                UiEvent::ScanFinished => self.scanning = false,
            }
        }
    }

    fn start_install_batch(&mut self) {
        if self.batch_running {
            return;
        }
        let candidates: Vec<_> = self
            .products
            .iter()
            .filter(|view| view.selected && view.support.can_install())
            .filter_map(|view| view.latest.clone())
            .collect();
        let Some(registry) = self.registry.clone() else {
            self.batch_summary = Some("信任注册表不可用，无法安装".into());
            return;
        };
        if candidates.is_empty() {
            self.batch_summary = Some("所选产品没有可执行的官方候选包".into());
            return;
        }
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.batch_running = true;
        self.batch_summary = None;
        let sender = self.event_sender.clone();
        let platform = self.platform.clone();
        let cancel = self.cancel_flag.clone();
        thread::spawn(move || {
            let results = run_install_batch(candidates, platform, registry, cancel, |update| {
                let _ = sender.send(UiEvent::Operation(update));
            });
            let _ = sender.send(UiEvent::BatchFinished(results));
        });
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();
        if self.scanning {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        configure_visuals(ui.ctx());
        let panel = egui::Frame::central_panel(ui.style()).fill(Color32::WHITE);
        panel.show(ui, |ui| {
            let content_width = 738.0_f32.min((ui.available_width() - 36.0).max(560.0));
            let left = ui.max_rect().center().x - content_width / 2.0;
            let content_rect = egui::Rect::from_min_max(
                egui::pos2(left, ui.max_rect().top()),
                egui::pos2(left + content_width, ui.max_rect().bottom()),
            );
            let mut clicked_product = None;

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(content_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.add_space(46.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("客户端下载")
                                .size(40.0)
                                .strong()
                                .color(Color32::from_rgb(18, 22, 29)),
                        );
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("选择需要安装的客户端")
                                .size(19.0)
                                .color(Color32::from_rgb(154, 158, 166)),
                        );
                    });
                    ui.add_space(44.0);

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for view in &self.products {
                                if draw_product_row(ui, view, self.scanning, self.batch_running) {
                                    clicked_product = Some(view.product);
                                }
                            }
                        });

                    ui.add_space(18.0);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.label(
                                RichText::new(self.platform.description.clone())
                                    .size(12.0)
                                    .color(Color32::from_rgb(165, 169, 177)),
                            );
                            ui.label(
                                RichText::new("·")
                                    .size(12.0)
                                    .color(Color32::from_rgb(190, 193, 199)),
                            );
                            let refresh = ui.add_enabled(
                                !self.scanning && !self.batch_running,
                                egui::Button::new(
                                    RichText::new(if self.scanning {
                                        "正在检测"
                                    } else {
                                        "刷新状态"
                                    })
                                    .size(12.0)
                                    .color(Color32::from_rgb(90, 130, 192)),
                                )
                                .frame(false),
                            );
                            if refresh.clicked() {
                                self.start_scan();
                            }
                            if self.scanning {
                                ui.spinner();
                            }
                            if self.batch_running {
                                let cancel = ui.add(
                                    egui::Button::new(
                                        RichText::new("取消后续下载")
                                            .size(12.0)
                                            .color(Color32::from_rgb(180, 84, 72)),
                                    )
                                    .frame(false),
                                );
                                if cancel.clicked() {
                                    self.cancel_flag.store(true, Ordering::Relaxed);
                                }
                            }
                        });
                        if let Some(summary) = &self.batch_summary {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(summary)
                                    .size(12.0)
                                    .color(Color32::from_rgb(120, 124, 132)),
                            );
                        }
                        if let Some(error) = &self.registry_error {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("配置不可用：{error}"))
                                    .size(12.0)
                                    .color(Color32::from_rgb(190, 70, 60)),
                            );
                        }
                    });
                },
            );

            if let Some(product) = clicked_product {
                for view in &mut self.products {
                    view.selected = view.product == product;
                }
                self.confirmation_open = true;
            }
        });

        if self.confirmation_open {
            let mut confirm = false;
            let mut open = true;
            egui::Window::new("安装前确认")
                .collapsible(false)
                .resizable(true)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label("将按以下顺序从官方来源下载、验证并启动安装：");
                    ui.add_space(6.0);
                    for view in self
                        .products
                        .iter()
                        .filter(|view| view.selected && view.support.can_install())
                    {
                        if let Some(candidate) = &view.latest {
                            ui.label(format!(
                                "• {} {} · {:?} · {:?} · {}",
                                view.product.display_name(),
                                candidate.version,
                                candidate.architecture,
                                candidate.package_kind,
                                candidate.download_url.host_str().unwrap_or("未知 host")
                            ));
                        }
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "交互安装器可能请求 UAC；取消只会停止尚未启动的下载/验证，不会强杀正在运行的厂商安装器。",
                        )
                        .small()
                        .color(Color32::DARK_GRAY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("开始安装").clicked() {
                            confirm = true;
                        }
                        if ui.button("返回").clicked() {
                            self.confirmation_open = false;
                        }
                    });
                });
            if !open {
                self.confirmation_open = false;
            }
            if confirm {
                self.confirmation_open = false;
                self.start_install_batch();
            }
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [1.0, 1.0, 1.0, 1.0]
    }
}

fn configure_visuals(context: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = Color32::WHITE;
    visuals.window_fill = Color32::WHITE;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.faint_bg_color = Color32::from_rgb(248, 250, 253);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    context.set_visuals(visuals);
}

fn draw_product_row(
    ui: &mut egui::Ui,
    view: &ProductView,
    scanning: bool,
    batch_running: bool,
) -> bool {
    let row_height = 102.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::hover(),
    );
    let mut clicked = false;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(16.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            let (icon_rect, _) =
                ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
            draw_product_icon(ui.painter(), icon_rect, view.product);
            ui.add_space(22.0);

            ui.allocate_ui_with_layout(
                egui::vec2((rect.width() - 220.0).max(280.0), 58.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(
                        RichText::new(view.product.display_name())
                            .size(22.0)
                            .color(Color32::from_rgb(24, 28, 35)),
                    );
                    ui.add_space(7.0);
                    ui.label(
                        RichText::new(product_subtitle(view, scanning, batch_running))
                            .size(12.5)
                            .color(subtitle_color(view)),
                    );
                },
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, enabled) = product_action(view, scanning, batch_running);
                let blue = Color32::from_rgb(66, 124, 216);
                let button = egui::Button::new(RichText::new(label).size(17.0).color(if enabled {
                    blue
                } else {
                    Color32::from_rgb(157, 164, 175)
                }))
                .min_size(egui::vec2(94.0, 43.0))
                .fill(Color32::WHITE)
                .stroke(egui::Stroke::new(
                    1.0,
                    if enabled {
                        blue
                    } else {
                        Color32::from_rgb(207, 212, 220)
                    },
                ))
                .corner_radius(6.0);
                if ui.add_enabled(enabled, button).clicked() {
                    clicked = true;
                }
            });
        },
    );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, Color32::from_rgb(232, 235, 240)),
    );
    clicked
}

fn product_subtitle(view: &ProductView, scanning: bool, batch_running: bool) -> String {
    if batch_running && view.selected {
        return view.status_line.clone();
    }
    if scanning && view.latest.is_none() {
        return "正在检测安装状态与官方版本".into();
    }
    match (
        view.detection.installed,
        view.detection.version.as_deref(),
        view.latest.as_ref(),
    ) {
        (true, Some(installed), Some(latest)) => {
            if version_is_older(installed, &latest.version) {
                format!("已安装 {installed} · 可更新至 {}", latest.version)
            } else {
                format!("已安装 {installed} · 已是最新版本")
            }
        }
        (true, Some(installed), None) => format!("已安装 {installed}"),
        (true, None, _) => "已检测到安装".into(),
        (false, _, Some(latest)) => format!("最新版本 {}", latest.version),
        (false, _, None) if matches!(view.support, SupportState::Unsupported(_)) => {
            "当前系统或架构不支持".into()
        }
        (false, _, None) => "官方版本信息暂不可用".into(),
    }
}

fn subtitle_color(view: &ProductView) -> Color32 {
    match view.support {
        SupportState::Ready => Color32::from_rgb(132, 137, 145),
        SupportState::Disabled(_) => Color32::from_rgb(142, 147, 156),
        SupportState::Unsupported(_) => Color32::from_rgb(165, 169, 177),
    }
}

fn product_action(view: &ProductView, scanning: bool, batch_running: bool) -> (&'static str, bool) {
    if batch_running && view.selected {
        return ("处理中", false);
    }
    if scanning && view.latest.is_none() {
        return ("检测中", false);
    }
    match &view.support {
        SupportState::Disabled(_) => ("待验证", false),
        SupportState::Unsupported(_) => ("不支持", false),
        SupportState::Ready => {
            let Some(latest) = &view.latest else {
                return ("不可用", false);
            };
            if let Some(installed) = view.detection.version.as_deref() {
                if version_is_older(installed, &latest.version) {
                    ("更新", true)
                } else {
                    ("已安装", false)
                }
            } else {
                ("下载", true)
            }
        }
    }
}

fn draw_product_icon(painter: &egui::Painter, rect: egui::Rect, product: ProductId) {
    let center = rect.center();
    match product {
        ProductId::Hermes => {
            let black = Color32::from_rgb(30, 32, 36);
            for (offset, scale) in [(0.0, 1.0), (8.0, 0.82), (15.0, 0.62)] {
                let y = center.y - 13.0 + offset;
                let points = vec![
                    egui::pos2(center.x - 17.0 * scale, y + 9.0 * scale),
                    egui::pos2(center.x + 13.0 * scale, y - 8.0 * scale),
                    egui::pos2(center.x + 8.0 * scale, y + 7.0 * scale),
                    egui::pos2(center.x - 14.0 * scale, y + 15.0 * scale),
                ];
                painter.add(egui::Shape::convex_polygon(
                    points,
                    black,
                    egui::Stroke::NONE,
                ));
            }
        }
        ProductId::Claude => {
            let orange = Color32::from_rgb(220, 103, 60);
            painter.circle_filled(center, 5.5, orange);
            for index in 0..12 {
                let angle = index as f32 * std::f32::consts::TAU / 12.0;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [center + direction * 9.0, center + direction * 22.0],
                    egui::Stroke::new(3.0, orange),
                );
            }
        }
        ProductId::ChatGpt => {
            let black = Color32::from_rgb(20, 23, 27);
            for index in 0..6 {
                let angle = index as f32 * std::f32::consts::TAU / 6.0;
                let loop_center = center + egui::vec2(angle.cos(), angle.sin()) * 8.0;
                painter.circle_stroke(loop_center, 12.0, egui::Stroke::new(2.7, black));
            }
            painter.circle_filled(center, 5.0, Color32::WHITE);
            painter.circle_stroke(center, 7.0, egui::Stroke::new(2.5, black));
        }
        ProductId::WorkBuddy => {
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "W",
                egui::FontId::proportional(38.0),
                Color32::from_rgb(60, 132, 226),
            );
        }
        ProductId::CcSwitch => {
            let gray = Color32::from_rgb(112, 132, 155);
            painter.rect_stroke(
                rect.shrink(5.0),
                8.0,
                egui::Stroke::new(2.0, gray),
                egui::StrokeKind::Middle,
            );
            let y1 = center.y - 7.0;
            let y2 = center.y + 7.0;
            painter.arrow(
                egui::pos2(center.x + 13.0, y1),
                egui::vec2(-25.0, 0.0),
                egui::Stroke::new(2.2, gray),
            );
            painter.arrow(
                egui::pos2(center.x - 13.0, y2),
                egui::vec2(25.0, 0.0),
                egui::Stroke::new(2.2, gray),
            );
        }
    }
}

fn install_system_font(context: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        "/System/Library/Fonts/PingFang.ttc",
    ];
    let Some((_, bytes)) = candidates
        .iter()
        .find_map(|path| fs::read(path).ok().map(|bytes| (*path, bytes)))
    else {
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("system-cjk".into(), FontData::from_owned(bytes).into());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "system-cjk".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "system-cjk".into());
    context.set_fonts(fonts);
}
