use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText};

use crate::adapters::resolve_latest;
use crate::core::{
    Detection, OperationUpdate, PlatformInfo, ProductId, ProductOperationResult, ProductView,
    ReleaseCandidate, SupportState, TrustRegistry, run_install_batch,
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

    fn selected_ready_count(&self) -> usize {
        self.products
            .iter()
            .filter(|view| view.selected && view.support.can_install())
            .count()
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

        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.add_space(22.0);
            ui.vertical_centered(|ui| {
                ui.heading(RichText::new("AI 客户端安装助手").size(30.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("只从已固定的官方来源解析、下载并验证安装包")
                        .color(Color32::GRAY),
                );
            });
            ui.add_space(18.0);

            ui.horizontal(|ui| {
                ui.label(format!("当前环境：{}", self.platform.description));
                if ui
                    .add_enabled(!self.scanning, egui::Button::new("刷新检测"))
                    .clicked()
                {
                    self.start_scan();
                }
                if self.scanning {
                    ui.spinner();
                }
            });

            if let Some(error) = &self.registry_error {
                ui.colored_label(Color32::RED, format!("信任注册表错误：{error}"));
            }
            ui.add_space(10.0);
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for view in &mut self.products {
                    ui.add_space(10.0);
                    ui.horizontal_top(|ui| {
                        ui.add_enabled(
                            view.support.can_install(),
                            egui::Checkbox::new(&mut view.selected, ""),
                        );
                        ui.vertical(|ui| {
                            ui.label(RichText::new(view.product.display_name()).size(19.0));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&view.status_line).color(Color32::DARK_GRAY),
                                )
                                .wrap(),
                            );
                            if let Some(reason) = view.support.detail() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(reason)
                                            .small()
                                            .color(Color32::from_rgb(156, 86, 0)),
                                    )
                                    .wrap(),
                                );
                            }
                            let color = match view.support {
                                SupportState::Ready => Color32::from_rgb(34, 139, 94),
                                SupportState::Disabled(_) => Color32::from_rgb(156, 86, 0),
                                SupportState::Unsupported(_) => Color32::GRAY,
                            };
                            ui.label(RichText::new(view.support.label()).strong().color(color));
                        });
                    });
                    ui.add_space(10.0);
                    ui.separator();
                }
            });

            ui.add_space(12.0);
            let selected_count = self.selected_ready_count();
            ui.horizontal(|ui| {
                let install = ui.add_enabled(
                    selected_count > 0 && !self.scanning && !self.batch_running,
                    egui::Button::new(format!("确认并顺序安装（{selected_count}）")),
                );
                if install.clicked() {
                    self.confirmation_open = true;
                }
                if self.batch_running && ui.button("取消后续下载").clicked() {
                    self.cancel_flag.store(true, Ordering::Relaxed);
                }
                ui.label(
                    RichText::new("未完成 proof 的项目不会回退到商店引导器、远程脚本或第三方镜像")
                        .small()
                        .color(Color32::GRAY),
                );
            });
            if let Some(summary) = &self.batch_summary {
                ui.add_space(6.0);
                ui.label(summary);
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
        [0.98, 0.98, 0.98, 1.0]
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
