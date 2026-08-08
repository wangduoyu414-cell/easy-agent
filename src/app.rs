use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText};

use crate::adapters::resolve_install_plan;
use crate::core::{
    Architecture, Detection, InstallPlan, OperatingSystem, OperationLog, OperationState,
    OperationUpdate, PackageKind, PlatformInfo, ProductId, ProductOperationResult, ProductView,
    SupportState, TrustRegistry, run_install_batch, version_is_older_for_product,
};
use crate::platform::{current_platform, detect_product};

enum UiEvent {
    Detection(ProductId, Detection),
    Resolution(ProductId, Result<InstallPlan, String>),
    Operation(OperationUpdate),
    BatchFinished {
        results: Vec<ProductOperationResult>,
        log_warning: Option<String>,
    },
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
    active_product: Option<ProductId>,
    active_operation_state: Option<OperationState>,
    download_progress: Option<f32>,
    cancel_requested: bool,
    close_confirmation_open: bool,
    close_after_batch: bool,
}

impl InstallerApp {
    pub fn new(context: &eframe::CreationContext<'_>) -> Self {
        context.egui_ctx.set_visuals(egui::Visuals::light());
        egui_extras::install_image_loaders(&context.egui_ctx);
        install_system_font(&context.egui_ctx);
        let platform = current_platform();
        let registry = TrustRegistry::embedded();
        let registry_error = registry.as_ref().err().map(ToString::to_string);
        let products = ProductId::ALL
            .into_iter()
            .map(|product| {
                let support = registry
                    .as_ref()
                    .map(|registry| registry.support_state_for_platform(product, &platform))
                    .unwrap_or_else(|error| {
                        SupportState::Disabled(format!("信任注册表不可用：{error}"))
                    });
                ProductView {
                    product,
                    selected: false,
                    support,
                    detection: Detection::absent("尚未检测"),
                    install_plan: None,
                    result_unknown: false,
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
            active_product: None,
            active_operation_state: None,
            download_progress: None,
            cancel_requested: false,
            close_confirmation_open: false,
            close_after_batch: false,
        };
        app.start_scan();
        app
    }

    fn start_scan(&mut self) {
        if !self.begin_scan_state() {
            return;
        }
        let sender = self.event_sender.clone();
        let platform = self.platform.clone();
        let registry = self.registry.clone();
        thread::spawn(move || {
            for product in ProductId::ALL {
                let trust = registry.as_ref().and_then(|registry| {
                    registry.find(product, platform.os, platform.architecture)
                });
                let detection = detect_product(product, trust);
                if sender.send(UiEvent::Detection(product, detection)).is_err() {
                    return;
                }
                let resolution = registry
                    .as_ref()
                    .ok_or_else(|| "信任注册表不可用".to_owned())
                    .and_then(|registry| {
                        resolve_install_plan(product, &platform, registry)
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

    fn begin_scan_state(&mut self) -> bool {
        if self.scanning {
            return false;
        }
        self.scanning = true;
        self.active_product = None;
        self.active_operation_state = None;
        self.download_progress = None;
        self.cancel_requested = false;
        for product in &mut self.products {
            product.result_unknown = false;
            product.status_line = "正在检测安装状态与官方版本…".into();
        }
        true
    }

    fn drain_events(&mut self) {
        let mut refresh_after_batch = false;
        while let Ok(event) = self.event_receiver.try_recv() {
            refresh_after_batch |= self.apply_event(event);
        }
        if refresh_after_batch {
            self.start_scan();
        }
    }

    fn apply_event(&mut self, event: UiEvent) -> bool {
        match event {
            UiEvent::Detection(product, detection) => {
                let allow_trusted_unknown_management_update = self
                    .registry
                    .as_ref()
                    .and_then(|registry| {
                        registry.find(product, self.platform.os, self.platform.architecture)
                    })
                    .is_some_and(|entry| entry.allow_trusted_update_when_management_unknown);
                if let Some(view) = self
                    .products
                    .iter_mut()
                    .find(|view| view.product == product)
                {
                    view.status_line = if detection.installed {
                        let management = if detection.managed {
                            " · 受组织管理"
                        } else if !detection.management_known {
                            if allow_trusted_unknown_management_update {
                                " · 管理状态未知（仅允许可信官方更新）"
                            } else {
                                " · 管理状态未知（不会自动覆盖）"
                            }
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
                false
            }
            UiEvent::Resolution(product, resolution) => {
                if let Some(view) = self
                    .products
                    .iter_mut()
                    .find(|view| view.product == product)
                {
                    match resolution {
                        Ok(plan) => {
                            match &plan {
                                InstallPlan::DirectPackage(candidate) => {
                                    view.status_line.push_str(&format!(
                                        " · 官方最新 {} ({:?})",
                                        candidate.version, candidate.architecture
                                    ));
                                }
                                InstallPlan::MicrosoftStore(_) => {
                                    view.status_line.push_str(" · Microsoft Store 后台官方源");
                                }
                            }
                            view.install_plan = Some(plan);
                        }
                        Err(error) => {
                            view.status_line.push_str(&format!(" · 版本解析：{error}"));
                            view.install_plan = None;
                        }
                    }
                }
                false
            }
            UiEvent::Operation(update) => {
                self.active_product = Some(update.product);
                self.active_operation_state = Some(update.state);
                self.download_progress = if update.state == OperationState::Downloading {
                    parse_download_progress(&update.message).or(self.download_progress)
                } else {
                    None
                };
                if let Some(view) = self
                    .products
                    .iter_mut()
                    .find(|view| view.product == update.product)
                {
                    view.result_unknown =
                        update.state == crate::core::OperationState::ResultUnknown;
                    view.status_line = format!("{} · {}", update.state.label(), update.message);
                }
                false
            }
            UiEvent::BatchFinished {
                results,
                log_warning,
            } => {
                self.batch_running = false;
                self.cancel_requested = false;
                self.download_progress = None;
                self.active_operation_state = results.last().map(|result| result.state);
                self.close_confirmation_open = false;
                let succeeded = results
                    .iter()
                    .filter(|result| result.state == crate::core::OperationState::Succeeded)
                    .count();
                let failed = results
                    .iter()
                    .filter(|result| result.state == crate::core::OperationState::Failed)
                    .count();
                let unknown = results
                    .iter()
                    .filter(|result| result.state == crate::core::OperationState::ResultUnknown)
                    .count();
                let cancelled = results.len().saturating_sub(succeeded + failed + unknown);
                let mut summary = format!(
                    "批次完成：成功 {succeeded}，失败 {failed}，结果待复检 {unknown}，取消 {cancelled}"
                );
                if let Some(warning) = log_warning {
                    summary.push_str(&format!(" · 操作日志不可用：{warning}"));
                }
                self.batch_summary = Some(summary);
                for view in &mut self.products {
                    view.selected = false;
                }
                unknown == 0
            }
            UiEvent::ScanFinished => {
                self.scanning = false;
                false
            }
        }
    }

    fn open_confirmation_for(&mut self, product: ProductId) -> bool {
        if self.scanning || self.batch_running {
            return false;
        }
        for view in &mut self.products {
            view.selected = view.product == product;
        }
        self.confirmation_open = true;
        true
    }

    fn start_install_batch(&mut self) {
        if self.batch_running {
            return;
        }
        let plans: Vec<_> = self
            .products
            .iter()
            .filter(|view| view.selected && view.support.can_install())
            .filter_map(|view| view.install_plan.clone())
            .collect();
        let Some(registry) = self.registry.clone() else {
            self.batch_summary = Some("信任注册表不可用，无法安装".into());
            return;
        };
        if plans.is_empty() {
            self.batch_summary = Some("所选产品没有可执行的官方安装计划".into());
            return;
        }
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.batch_running = true;
        self.batch_summary = None;
        self.active_product = plans.first().map(InstallPlan::product);
        self.active_operation_state = Some(OperationState::Ready);
        self.download_progress = None;
        self.cancel_requested = false;
        let sender = self.event_sender.clone();
        let platform = self.platform.clone();
        let cancel = self.cancel_flag.clone();
        let (operation_log, initial_log_warning) = match OperationLog::open_default() {
            Ok(log) => (Some(log), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let log_warning = Arc::new(Mutex::new(initial_log_warning));
        thread::spawn(move || {
            let warning_for_updates = log_warning.clone();
            let results = run_install_batch(plans, platform, registry, cancel, |update| {
                if let Some(log) = &operation_log
                    && let Err(error) = log.record(&update)
                    && let Ok(mut warning) = warning_for_updates.lock()
                    && warning.is_none()
                {
                    *warning = Some(error.to_string());
                }
                let _ = sender.send(UiEvent::Operation(update));
            });
            let log_warning = log_warning.lock().ok().and_then(|warning| warning.clone());
            let _ = sender.send(UiEvent::BatchFinished {
                results,
                log_warning,
            });
        });
    }

    fn request_cancel(&mut self) {
        if !self.batch_running
            || self.cancel_requested
            || !operation_can_cancel(self.active_operation_state)
        {
            return;
        }
        self.cancel_requested = true;
        self.cancel_flag.store(true, Ordering::Relaxed);
        if let Some(product) = self.active_product
            && let Some(view) = self
                .products
                .iter_mut()
                .find(|view| view.product == product)
        {
            view.status_line = "正在取消当前任务，请稍候…".into();
        }
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();
        if self.close_after_batch && !self.batch_running {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.batch_running && ui.ctx().input(|input| input.viewport().close_requested()) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_confirmation_open = true;
        }
        if self.scanning || self.batch_running {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(if self.batch_running {
                    50
                } else {
                    100
                }));
        }

        configure_visuals(ui.ctx());
        let panel = egui::Frame::central_panel(ui.style()).fill(Color32::WHITE);
        panel.show(ui, |ui| {
            let content_width = 650.0_f32.min((ui.available_width() - 32.0).max(560.0));
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
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            draw_brand_icon(ui, 44.0);
                            ui.add_space(12.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("easy agent")
                                        .size(30.0)
                                        .strong()
                                        .color(Color32::from_rgb(18, 22, 29)),
                                );
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new("安全安装与更新常用 AI 客户端")
                                        .size(15.0)
                                        .color(Color32::from_rgb(112, 117, 126)),
                                );
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new("官方来源 · 签名校验 · 更新失败自动回滚")
                                        .size(12.0)
                                        .color(Color32::from_rgb(90, 99, 112)),
                                );
                            });
                        });
                    });
                    ui.add_space(20.0);

                    egui::ScrollArea::vertical()
                        .max_height(421.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let mut ordered_products = self.products.iter().collect::<Vec<_>>();
                            ordered_products
                                .sort_by_key(|view| support_display_rank(&view.support));
                            for view in ordered_products {
                                let is_active = self.active_product == Some(view.product);
                                let show_active_state = is_active && self.batch_running;
                                if draw_product_row(
                                    ui,
                                    view,
                                    self.scanning,
                                    self.batch_running,
                                    show_active_state
                                        .then_some(self.active_operation_state)
                                        .flatten(),
                                    show_active_state
                                        .then_some(self.download_progress)
                                        .flatten(),
                                    show_active_state && self.cancel_requested,
                                ) {
                                    clicked_product = Some(view.product);
                                }
                            }
                        });

                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.label(
                                RichText::new(format!(
                                    "{} · easy agent {}",
                                    friendly_platform_description(&self.platform),
                                    env!("CARGO_PKG_VERSION")
                                ))
                                .size(12.0)
                                .color(Color32::from_rgb(112, 117, 126)),
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
                                if operation_can_cancel(self.active_operation_state) {
                                    let cancel = ui.add_enabled(
                                        !self.cancel_requested,
                                        egui::Button::new(
                                            RichText::new(if self.cancel_requested {
                                                "正在取消…"
                                            } else {
                                                "取消任务"
                                            })
                                            .size(12.0)
                                            .color(Color32::from_rgb(178, 66, 56)),
                                        )
                                        .frame(false),
                                    );
                                    if cancel.clicked() {
                                        self.request_cancel();
                                    }
                                } else {
                                    ui.label(
                                        RichText::new("正在完成关键步骤，请勿关闭")
                                            .size(12.0)
                                            .color(Color32::from_rgb(168, 96, 44)),
                                    );
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
                self.open_confirmation_for(product);
            }
        });

        if self.confirmation_open {
            let mut confirm = false;
            let mut close = false;
            let action_label = self
                .products
                .iter()
                .find(|view| view.selected)
                .map(|view| {
                    format!(
                        "{} {}",
                        if view.detection.installed {
                            "更新"
                        } else {
                            "安装"
                        },
                        view.product.display_name()
                    )
                })
                .unwrap_or_else(|| "开始操作".into());
            let modal = egui::Modal::new(egui::Id::new("install-confirmation"))
                .backdrop_color(Color32::from_black_alpha(48))
                .frame(
                    egui::Frame::popup(ui.style())
                        .fill(Color32::WHITE)
                        .corner_radius(12.0)
                        .inner_margin(egui::Margin::symmetric(18, 16)),
                )
                .show(ui.ctx(), |ui| {
                    ui.set_width(460.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("确认{action_label}"))
                                .size(18.0)
                                .strong()
                                .color(Color32::from_rgb(24, 28, 35)),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let close_button = egui::Button::new(
                                    RichText::new("×")
                                        .size(18.0)
                                        .color(Color32::from_rgb(116, 121, 130)),
                                )
                                .frame(false);
                                if ui.add(close_button).clicked() {
                                    close = true;
                                }
                            },
                        );
                    });
                    ui.add_space(5.0);
                    ui.label(
                        RichText::new("请确认版本、官方来源和安装位置")
                            .size(13.0)
                            .color(Color32::from_rgb(82, 88, 98)),
                    );
                    ui.add_space(12.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(247, 249, 252))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(13, 11))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            for view in self
                                .products
                                .iter()
                                .filter(|view| view.selected && view.support.can_install())
                            {
                                if let Some(plan) = &view.install_plan {
                                    ui.horizontal(|ui| {
                                        draw_product_icon(ui, view.product);
                                        ui.add_space(10.0);
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(view.product.display_name())
                                                    .size(16.0)
                                                    .strong()
                                                    .color(Color32::from_rgb(24, 28, 35)),
                                            );
                                            ui.add_space(3.0);
                                            let (version, source) = match plan {
                                                InstallPlan::DirectPackage(candidate) => (
                                                    format!(
                                                        "{} → {}",
                                                        view.detection
                                                            .version
                                                            .as_deref()
                                                            .map(|version| display_version(
                                                                view.product,
                                                                version
                                                            ))
                                                            .unwrap_or_else(|| "未安装".into()),
                                                        display_version(
                                                            view.product,
                                                            &candidate.version
                                                        )
                                                    ),
                                                    format!(
                                                        "{} · {} · {}",
                                                        friendly_architecture(
                                                            candidate.architecture
                                                        ),
                                                        friendly_package_kind(
                                                            candidate.package_kind
                                                        ),
                                                        candidate
                                                            .download_url
                                                            .host_str()
                                                            .unwrap_or("官方服务")
                                                    ),
                                                ),
                                                InstallPlan::MicrosoftStore(store) => (
                                                    view.detection
                                                        .version
                                                        .as_deref()
                                                        .map(|version| format!(
                                                            "当前版本 {version} → 官方最新版本"
                                                        ))
                                                        .unwrap_or_else(|| {
                                                            "未安装 → 官方最新版本".into()
                                                        }),
                                                    format!(
                                                        "{} · Microsoft 官方服务",
                                                        friendly_architecture(store.architecture)
                                                    ),
                                                ),
                                            };
                                            ui.label(
                                                RichText::new(version)
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(70, 77, 88)),
                                            );
                                            ui.add_space(2.0);
                                            ui.label(
                                                RichText::new(source)
                                                    .size(12.0)
                                                    .color(Color32::from_rgb(96, 103, 114)),
                                            );
                                        });
                                    });
                                }
                            }
                        });
                    ui.add_space(11.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(244, 248, 255))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(
                                    "将检查文件完整性、官方签名、应用身份、版本和芯片架构。更新失败会自动恢复原版本。",
                                )
                                .size(12.0)
                                .color(Color32::from_rgb(64, 82, 112)),
                            );
                            ui.add_space(3.0);
                            ui.label(
                                RichText::new(install_location_summary(&self.platform))
                                    .size(12.0)
                                    .color(Color32::from_rgb(82, 88, 98)),
                            );
                        });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "请先保存目标客户端中的未提交内容。如果应用正在运行，安装会提示你先退出或由系统安全处理。",
                        )
                        .size(12.0)
                        .color(Color32::from_rgb(82, 88, 98)),
                    );
                    ui.add_space(14.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let start = egui::Button::new(
                            RichText::new(&action_label)
                                .size(13.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .min_size(egui::vec2(128.0, 36.0))
                        .fill(Color32::from_rgb(44, 105, 200))
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(7.0);
                        if ui.add(start).clicked() {
                            confirm = true;
                        }
                        let back = egui::Button::new(RichText::new("返回").size(13.5))
                            .min_size(egui::vec2(82.0, 36.0));
                        if ui.add(back).clicked() {
                            close = true;
                        }
                    });
                });
            if modal.should_close() || close {
                self.confirmation_open = false;
            }
            if confirm {
                self.confirmation_open = false;
                self.start_install_batch();
            }
        }

        if self.close_confirmation_open {
            let mut keep_open = true;
            let can_cancel = operation_can_cancel(self.active_operation_state);
            egui::Modal::new(egui::Id::new("close-running-task"))
                .backdrop_color(Color32::from_black_alpha(52))
                .frame(
                    egui::Frame::popup(ui.style())
                        .fill(Color32::WHITE)
                        .corner_radius(12.0)
                        .inner_margin(egui::Margin::symmetric(20, 18)),
                )
                .show(ui.ctx(), |ui| {
                    ui.set_width(410.0);
                    ui.label(
                        RichText::new("任务仍在进行")
                            .size(18.0)
                            .strong()
                            .color(Color32::from_rgb(24, 28, 35)),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(if can_cancel {
                            "现在关闭可能中断下载。可以继续等待，或安全取消任务后自动退出。"
                        } else {
                            "应用正在写入或复检，当前不能安全退出。请等待这一步完成。"
                        })
                        .size(13.0)
                        .color(Color32::from_rgb(82, 88, 98)),
                    );
                    ui.add_space(16.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let wait = egui::Button::new(
                            RichText::new("继续等待").size(13.5).color(Color32::WHITE),
                        )
                        .min_size(egui::vec2(96.0, 36.0))
                        .fill(Color32::from_rgb(44, 105, 200))
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(7.0);
                        if ui.add(wait).clicked() {
                            keep_open = false;
                        }
                        if can_cancel {
                            let cancel_and_close = egui::Button::new(
                                RichText::new(if self.cancel_requested {
                                    "取消完成后退出"
                                } else {
                                    "取消任务并退出"
                                })
                                .size(13.5),
                            )
                            .min_size(egui::vec2(126.0, 36.0));
                            if ui.add(cancel_and_close).clicked() {
                                self.request_cancel();
                                self.close_after_batch = true;
                                keep_open = false;
                            }
                        }
                    });
                });
            if !keep_open {
                self.close_confirmation_open = false;
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
    active_state: Option<OperationState>,
    download_progress: Option<f32>,
    cancel_requested: bool,
) -> bool {
    let row_height = 84.0;
    let (rect, row_response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::hover(),
    );
    if active_state.is_some() {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(2.0, 4.0)),
            9.0,
            Color32::from_rgb(246, 249, 255),
        );
    } else if row_response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(2.0, 4.0)),
            9.0,
            Color32::from_rgb(250, 251, 253),
        );
    }
    let mut clicked = false;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(12.0, 2.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            draw_product_icon(ui, view.product);
            ui.add_space(16.0);

            ui.allocate_ui_with_layout(
                egui::vec2((rect.width() - 188.0).max(270.0), 60.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(
                        RichText::new(view.product.display_name())
                            .size(18.0)
                            .color(Color32::from_rgb(24, 28, 35)),
                    );
                    ui.add_space(4.0);
                    let subtitle = product_subtitle(view, scanning, batch_running);
                    let detail = product_detail(view);
                    let subtitle_response = ui.add(
                        egui::Label::new(
                            RichText::new(&subtitle)
                                .size(12.5)
                                .color(subtitle_color(view)),
                        )
                        .truncate(),
                    );
                    subtitle_response.on_hover_text(detail);
                    if let Some(progress) = download_progress {
                        ui.add_space(5.0);
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(ui.available_width())
                                .desired_height(5.0)
                                .fill(Color32::from_rgb(44, 105, 200))
                                .animate(true),
                        );
                    }
                },
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, enabled) = active_state
                    .map(|state| {
                        (
                            if cancel_requested {
                                "正在取消"
                            } else {
                                operation_button_label(state)
                            },
                            false,
                        )
                    })
                    .unwrap_or_else(|| product_action(view, scanning, batch_running));
                let blue = Color32::from_rgb(44, 105, 200);
                let button =
                    egui::Button::new(RichText::new(label).size(14.5).strong().color(if enabled {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(112, 119, 130)
                    }))
                    .min_size(egui::vec2(92.0, 38.0))
                    .fill(if enabled {
                        blue
                    } else {
                        Color32::from_rgb(247, 248, 250)
                    })
                    .stroke(egui::Stroke::new(
                        1.0,
                        if enabled {
                            blue
                        } else {
                            Color32::from_rgb(216, 220, 227)
                        },
                    ))
                    .corner_radius(7.0);
                let mut response = ui.add_enabled(enabled, button);
                response = response.on_hover_text(product_detail(view));
                if response.clicked() {
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
    if view.result_unknown {
        return view.status_line.clone();
    }
    if batch_running && view.selected {
        return view.status_line.clone();
    }
    if scanning && view.install_plan.is_none() {
        return "正在检测安装状态与官方版本".into();
    }
    match &view.install_plan {
        Some(InstallPlan::DirectPackage(latest)) => {
            match (view.detection.installed, view.detection.version.as_deref()) {
                (true, Some(installed)) => {
                    if version_is_older_for_product(view.product, installed, &latest.version) {
                        format!(
                            "已安装 {} · 可更新至 {}",
                            display_version(view.product, installed),
                            display_version(view.product, &latest.version)
                        )
                    } else {
                        format!(
                            "已安装 {} · 已是最新版本",
                            display_version(view.product, installed)
                        )
                    }
                }
                (true, None) => "已检测到安装".into(),
                (false, _) => format!(
                    "可安装 · 最新版本 {}",
                    display_version(view.product, &latest.version)
                ),
            }
        }
        Some(InstallPlan::MicrosoftStore(_)) => {
            match (view.detection.installed, view.detection.version.as_deref()) {
                (true, Some(installed)) => format!("已安装 {installed} · 可检查更新"),
                (true, None) => "已检测到安装 · 版本未知".into(),
                (false, _) => "由 Microsoft 官方服务安装最新版本".into(),
            }
        }
        None => match &view.support {
            SupportState::Disabled(_) => disabled_product_summary(view.product).into(),
            SupportState::Unsupported(_) => "当前系统或芯片不受官方支持".into(),
            SupportState::Ready => {
                match (view.detection.installed, view.detection.version.as_deref()) {
                    (true, Some(version)) => format!(
                        "已安装 {} · 暂时无法获取最新版本",
                        display_version(view.product, version)
                    ),
                    (true, None) => "已检测到安装 · 暂时无法获取最新版本".into(),
                    (false, _) => "暂时无法获取最新版本，请稍后刷新".into(),
                }
            }
        },
    }
}

fn subtitle_color(view: &ProductView) -> Color32 {
    match view.support {
        SupportState::Ready => Color32::from_rgb(96, 103, 114),
        SupportState::Disabled(_) => Color32::from_rgb(154, 94, 42),
        SupportState::Unsupported(_) => Color32::from_rgb(116, 122, 132),
    }
}

fn product_action(view: &ProductView, scanning: bool, batch_running: bool) -> (&'static str, bool) {
    if view.result_unknown {
        return ("待复检", false);
    }
    if batch_running {
        if view.selected {
            return ("处理中", false);
        }
        let (label, _) = product_action(view, scanning, false);
        return (label, false);
    }
    if scanning {
        return ("检测中", false);
    }
    match &view.support {
        SupportState::Disabled(_) => ("暂不可用", false),
        SupportState::Unsupported(_) => ("不支持", false),
        SupportState::Ready => {
            let Some(plan) = &view.install_plan else {
                return ("暂不可用", false);
            };
            match plan {
                InstallPlan::DirectPackage(latest) => {
                    if let Some(installed) = view.detection.version.as_deref() {
                        if version_is_older_for_product(view.product, installed, &latest.version) {
                            ("更新", true)
                        } else {
                            ("已安装", false)
                        }
                    } else {
                        ("安装", true)
                    }
                }
                InstallPlan::MicrosoftStore(_) => {
                    if view.detection.managed || !view.detection.management_known {
                        return ("受管理", false);
                    }
                    if view.detection.installed {
                        if view.detection.version.is_some() {
                            ("更新", true)
                        } else {
                            ("版本未知", false)
                        }
                    } else {
                        ("安装", true)
                    }
                }
            }
        }
    }
}

fn product_detail(view: &ProductView) -> String {
    match &view.support {
        SupportState::Disabled(reason) | SupportState::Unsupported(reason) => reason.clone(),
        SupportState::Ready => view.status_line.clone(),
    }
}

fn disabled_product_summary(product: ProductId) -> &'static str {
    match product {
        ProductId::Claude => "官方下载服务暂时不可用",
        ProductId::Hermes => "官方安装流程暂未完成验证",
        _ => "当前版本暂未完成安全验证",
    }
}

const fn support_display_rank(support: &SupportState) -> u8 {
    match support {
        SupportState::Ready => 0,
        SupportState::Disabled(_) => 1,
        SupportState::Unsupported(_) => 2,
    }
}

fn display_version(product: ProductId, version: &str) -> String {
    if product == ProductId::WorkBuddy {
        version.split('.').take(3).collect::<Vec<_>>().join(".")
    } else {
        version.to_owned()
    }
}

const fn operation_button_label(state: OperationState) -> &'static str {
    match state {
        OperationState::Ready => "准备中",
        OperationState::Downloading => "下载中",
        OperationState::Verifying => "验证中",
        OperationState::AwaitingUserInstall => "准备安装",
        OperationState::Installing => "安装中",
        OperationState::Postchecking => "复检中",
        OperationState::Succeeded => "已完成",
        OperationState::ResultUnknown => "待复检",
        OperationState::Failed => "失败",
        OperationState::Cancelled => "已取消",
    }
}

const fn operation_can_cancel(state: Option<OperationState>) -> bool {
    matches!(
        state,
        Some(OperationState::Ready | OperationState::Downloading | OperationState::Verifying)
    )
}

fn parse_download_progress(message: &str) -> Option<f32> {
    let percent = message
        .strip_prefix("已下载 ")?
        .split('%')
        .next()?
        .trim()
        .parse::<f32>()
        .ok()?;
    Some((percent / 100.0).clamp(0.0, 1.0))
}

fn friendly_platform_description(platform: &PlatformInfo) -> String {
    let version = platform
        .os_version
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(|value| format!(" {value}"))
        .unwrap_or_default();
    match (platform.os, platform.architecture) {
        (OperatingSystem::MacOs, Architecture::X64) => format!("macOS{version} · Intel Mac"),
        (OperatingSystem::MacOs, Architecture::Arm64) => {
            format!("macOS{version} · Apple Silicon")
        }
        (OperatingSystem::Windows, Architecture::X64) => format!("Windows{version} · x64"),
        (OperatingSystem::Windows, Architecture::Arm64) => format!("Windows{version} · ARM64"),
        _ => platform.description.clone(),
    }
}

const fn friendly_architecture(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X64 => "Intel / x64",
        Architecture::Arm64 => "Apple Silicon / ARM64",
        Architecture::Unsupported => "未知芯片架构",
    }
}

const fn friendly_package_kind(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::Exe => "EXE 安装包",
        PackageKind::Msi => "MSI 安装包",
        PackageKind::Msix => "MSIX 应用包",
        PackageKind::Dmg => "DMG 应用包",
        PackageKind::TarGz => "签名应用压缩包",
        PackageKind::Zip => "ZIP 应用包",
    }
}

fn install_location_summary(platform: &PlatformInfo) -> &'static str {
    match platform.os {
        OperatingSystem::MacOs => "安装位置：已有应用原位置；首次安装到用户 Applications。",
        OperatingSystem::Windows => "安装位置：由 Windows 或官方安装程序安全管理。",
        OperatingSystem::Unsupported => "安装位置：当前平台不受支持。",
    }
}

fn draw_product_icon(ui: &mut egui::Ui, product: ProductId) {
    let source = match product {
        ProductId::Hermes => egui::include_image!("../assets/icons/official/hermes.png"),
        ProductId::Claude => egui::include_image!("../assets/icons/official/claude.png"),
        ProductId::ChatGpt => egui::include_image!("../assets/icons/official/chatgpt.png"),
        ProductId::WorkBuddy => egui::include_image!("../assets/icons/official/workbuddy.png"),
        ProductId::CcSwitch => egui::include_image!("../assets/icons/official/cc-switch.png"),
    };
    ui.add(
        egui::Image::new(source)
            .fit_to_exact_size(egui::vec2(44.0, 44.0))
            .maintain_aspect_ratio(true),
    );
}

fn draw_brand_icon(ui: &mut egui::Ui, size: f32) {
    ui.add(
        egui::Image::new(egui::include_image!(
            "../assets/branding/easy-agent-icon-512.png"
        ))
        .fit_to_exact_size(egui::vec2(size, size))
        .maintain_aspect_ratio(true),
    );
}

#[cfg(target_os = "windows")]
const SYSTEM_CJK_FONT_CANDIDATES: &[(&str, u32)] = &[
    (r"C:\Windows\Fonts\msyh.ttc", 0),
    (r"C:\Windows\Fonts\msyh.ttf", 0),
    (r"C:\Windows\Fonts\simhei.ttf", 0),
    (r"C:\Windows\Fonts\simsun.ttc", 0),
];

#[cfg(target_os = "macos")]
const SYSTEM_CJK_FONT_CANDIDATES: &[(&str, u32)] = &[
    ("/System/Library/Fonts/PingFang.ttc", 0),
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/System/Library/Fonts/STHeiti Medium.ttc", 0),
    ("/System/Library/Fonts/STHeiti Light.ttc", 0),
    ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0),
];

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const SYSTEM_CJK_FONT_CANDIDATES: &[(&str, u32)] = &[];

fn install_system_font(context: &egui::Context) -> bool {
    let Some((_, bytes, face_index)) =
        SYSTEM_CJK_FONT_CANDIDATES
            .iter()
            .find_map(|(path, face_index)| {
                fs::read(path).ok().map(|bytes| (*path, bytes, *face_index))
            })
    else {
        return false;
    };

    let mut fonts = FontDefinitions::default();
    let mut font_data = FontData::from_owned(bytes);
    font_data.index = face_index;
    fonts
        .font_data
        .insert("system-cjk".into(), font_data.into());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push("system-cjk".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("system-cjk".into());
    context.set_fonts(fonts);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ReleaseCandidate;

    fn chatgpt_view() -> ProductView {
        ProductView {
            product: ProductId::ChatGpt,
            selected: false,
            support: SupportState::Ready,
            detection: Detection {
                installed: true,
                version: Some("26.721.11231.0".into()),
                managed: false,
                management_known: true,
                package_identity: Some("OpenAI.Codex".into()),
                package_family: Some("OpenAI.Codex_2p2nqsd0c76g0".into()),
                publisher: Some("CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B".into()),
                architecture: Some(Architecture::X64),
                evidence: "fixture".into(),
            },
            install_plan: Some(InstallPlan::DirectPackage(ReleaseCandidate {
                product: ProductId::ChatGpt,
                version: "26.727.6591.0".into(),
                architecture: Architecture::X64,
                package_kind: PackageKind::Msix,
                download_url: url::Url::parse(
                    "https://persistent.oaistatic.com/codex-app-prod/releases/26.727.6591.0/ChatGPT-x64.msix",
                )
                .unwrap(),
                expected_sha256: None,
                detached_signature: None,
            })),
            result_unknown: false,
            status_line: "已安装".into(),
            staged_file: None,
        }
    }

    fn workbuddy_view() -> ProductView {
        ProductView {
            product: ProductId::WorkBuddy,
            selected: false,
            support: SupportState::Ready,
            detection: Detection::absent("fixture"),
            install_plan: Some(InstallPlan::DirectPackage(ReleaseCandidate {
                product: ProductId::WorkBuddy,
                version: "5.3.8.34705286".into(),
                architecture: Architecture::X64,
                package_kind: PackageKind::Exe,
                download_url: url::Url::parse("https://example.invalid/workbuddy.exe").unwrap(),
                expected_sha256: None,
                detached_signature: None,
            })),
            result_unknown: false,
            status_line: "可安装".into(),
            staged_file: None,
        }
    }

    fn test_app() -> InstallerApp {
        let (event_sender, event_receiver) = unbounded();
        InstallerApp {
            platform: PlatformInfo {
                os: OperatingSystem::Windows,
                architecture: Architecture::X64,
                os_version: None,
                description: "windows / x64".into(),
            },
            registry: None,
            products: vec![chatgpt_view(), workbuddy_view()],
            event_sender,
            event_receiver,
            scanning: false,
            batch_running: false,
            confirmation_open: false,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            batch_summary: None,
            registry_error: None,
            active_product: None,
            active_operation_state: None,
            download_progress: None,
            cancel_requested: false,
            close_confirmation_open: false,
            close_after_batch: false,
        }
    }

    #[test]
    fn batch_mutex_disables_every_action_and_blocks_a_second_confirmation() {
        let mut app = test_app();
        assert_eq!(
            product_subtitle(&app.products[0], false, false),
            "已安装 26.721.11231.0 · 可更新至 26.727.6591.0"
        );
        assert_eq!(
            product_action(&app.products[0], false, false),
            ("更新", true)
        );
        assert_eq!(
            product_action(&app.products[1], false, false),
            ("安装", true)
        );
        assert_eq!(
            product_action(&app.products[1], true, false),
            ("检测中", false)
        );
        assert!(app.open_confirmation_for(ProductId::ChatGpt));
        assert!(app.products[0].selected);
        app.confirmation_open = false;
        app.batch_running = true;

        assert_eq!(
            product_action(&app.products[0], false, true),
            ("处理中", false)
        );
        assert_eq!(
            product_action(&app.products[1], false, true),
            ("安装", false)
        );
        assert!(!app.open_confirmation_for(ProductId::WorkBuddy));
        assert!(!app.confirmation_open);
        assert!(app.products[0].selected);
        assert!(!app.products[1].selected);
    }

    #[test]
    fn registered_older_workbuddy_is_presented_as_an_update() {
        let mut view = workbuddy_view();
        view.detection = Detection {
            installed: true,
            version: Some("5.1.7".into()),
            managed: false,
            management_known: true,
            package_identity: None,
            package_family: None,
            publisher: Some("Tencent Technology (Shenzhen) Company Limited".into()),
            architecture: None,
            evidence: "Uninstall:WorkBuddy 5.1.7 [HKCU]".into(),
        };
        assert_eq!(product_action(&view, false, false), ("更新", true));
    }

    #[test]
    fn unavailable_products_show_a_simple_summary_and_preserve_exact_details() {
        let mut view = workbuddy_view();
        view.install_plan = None;
        view.support = SupportState::Disabled("官方摘要与下载文件不一致，安装保持禁用".into());
        view.detection = Detection {
            installed: true,
            version: Some("5.3.8".into()),
            ..Detection::absent("fixture")
        };
        assert_eq!(
            product_subtitle(&view, false, false),
            "当前版本暂未完成安全验证"
        );
        assert_eq!(
            product_detail(&view),
            "官方摘要与下载文件不一致，安装保持禁用"
        );
        assert_eq!(product_action(&view, false, false), ("暂不可用", false));

        view.support = SupportState::Unsupported("厂商明确不支持 Intel Mac".into());
        assert_eq!(
            product_subtitle(&view, false, false),
            "当前系统或芯片不受官方支持"
        );
        assert_eq!(product_detail(&view), "厂商明确不支持 Intel Mac");
        assert_eq!(product_action(&view, false, false), ("不支持", false));

        view.support = SupportState::Ready;
        view.status_line = "已安装 5.3.8 · 版本解析：server returned HTTP 403".into();
        assert_eq!(
            product_subtitle(&view, false, false),
            "已安装 5.3.8 · 暂时无法获取最新版本"
        );
        assert_eq!(product_detail(&view), view.status_line);
    }

    #[test]
    fn download_progress_and_cancel_boundaries_are_ui_safe() {
        let progress = parse_download_progress("已下载 65.8% (260.3/395.7 MiB)").unwrap();
        assert!((progress - 0.658).abs() < 0.0001);
        assert_eq!(parse_download_progress("已下载 12.0 MiB"), None);
        assert!(operation_can_cancel(Some(OperationState::Downloading)));
        assert!(operation_can_cancel(Some(OperationState::Verifying)));
        assert!(!operation_can_cancel(Some(OperationState::Installing)));
        assert!(!operation_can_cancel(Some(OperationState::Postchecking)));
    }

    #[test]
    fn platform_and_workbuddy_versions_use_user_facing_labels() {
        assert_eq!(
            display_version(ProductId::WorkBuddy, "5.3.8.34705286"),
            "5.3.8"
        );
        assert_eq!(
            friendly_platform_description(&PlatformInfo {
                os: OperatingSystem::MacOs,
                architecture: Architecture::X64,
                os_version: Some("26.4.1".into()),
                description: "macos 26.4.1 / x64".into(),
            }),
            "macOS 26.4.1 · Intel Mac"
        );
    }

    #[test]
    fn workbuddy_registered_release_version_matches_the_full_api_build_version() {
        let mut view = workbuddy_view();
        view.detection = Detection {
            installed: true,
            version: Some("5.3.8".into()),
            managed: false,
            management_known: true,
            package_identity: None,
            package_family: None,
            publisher: Some("Tencent Technology (Shenzhen) Company Limited".into()),
            architecture: Some(Architecture::X64),
            evidence: "Uninstall:WorkBuddy 5.3.8 [HKCU]".into(),
        };
        assert_eq!(product_action(&view, false, false), ("已安装", false));
        assert_eq!(
            product_subtitle(&view, false, false),
            "已安装 5.3.8 · 已是最新版本"
        );
    }

    #[test]
    fn successful_batch_finish_requests_the_ui_rescan_and_clears_selection() {
        let mut app = test_app();
        app.batch_running = true;
        app.products[0].selected = true;
        let refresh = app.apply_event(UiEvent::BatchFinished {
            results: vec![ProductOperationResult {
                product: ProductId::ChatGpt,
                state: OperationState::Succeeded,
                message: "ok".into(),
            }],
            log_warning: None,
        });

        assert!(refresh);
        assert!(!app.batch_running);
        assert!(app.products.iter().all(|view| !view.selected));
        assert_eq!(
            app.batch_summary.as_deref(),
            Some("批次完成：成功 1，失败 0，结果待复检 0，取消 0")
        );
    }

    #[test]
    fn result_unknown_suppresses_auto_rescan_until_explicit_refresh() {
        let mut app = test_app();
        app.batch_running = true;
        app.products[0].selected = true;
        app.apply_event(UiEvent::Operation(OperationUpdate {
            product: ProductId::ChatGpt,
            state: OperationState::ResultUnknown,
            message: "deployment continues".into(),
        }));
        assert_eq!(
            product_action(&app.products[0], false, true),
            ("待复检", false)
        );

        let refresh = app.apply_event(UiEvent::BatchFinished {
            results: vec![ProductOperationResult {
                product: ProductId::ChatGpt,
                state: OperationState::ResultUnknown,
                message: "unknown".into(),
            }],
            log_warning: None,
        });
        assert!(!refresh);
        assert!(app.products[0].result_unknown);

        assert!(app.begin_scan_state());
        assert!(app.scanning);
        assert!(app.products.iter().all(|view| !view.result_unknown));
    }

    #[test]
    fn batch_summary_reports_when_the_local_operation_log_is_unavailable() {
        let mut app = test_app();
        app.batch_running = true;
        let refresh = app.apply_event(UiEvent::BatchFinished {
            results: vec![ProductOperationResult {
                product: ProductId::WorkBuddy,
                state: OperationState::Failed,
                message: "download failed".into(),
            }],
            log_warning: Some("access denied".into()),
        });
        assert!(refresh);
        assert_eq!(
            app.batch_summary.as_deref(),
            Some("批次完成：成功 0，失败 1，结果待复检 0，取消 0 · 操作日志不可用：access denied")
        );
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn system_font_fallback_contains_required_chinese_glyphs() {
        let context = egui::Context::default();
        assert!(install_system_font(&context));

        let mut contains_required_glyphs = false;
        let _ = context.run_ui(egui::RawInput::default(), |ui| {
            contains_required_glyphs = ui.ctx().fonts_mut(|fonts| {
                fonts.has_glyphs(
                    &egui::FontId::proportional(16.0),
                    "正在检测安装状态与官方版本更新不可用",
                )
            });
        });

        assert!(contains_required_glyphs);
    }
}
