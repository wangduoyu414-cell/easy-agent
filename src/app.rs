use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText};

use crate::adapters::{resolve_install_plan, resolve_verified_download_fallback};
use crate::core::{
    Architecture, ArtifactSource, Detection, InstallExecutionGate, InstallPlan, OperatingSystem,
    OperationLog, OperationState, OperationUpdate, PackageKind, PlatformInfo, ProductId,
    ProductOperationResult, ProductView, SupportState, TrustRegistry, run_install_plan,
    version_is_older_for_product,
};
use crate::platform::{current_platform, detect_product, detect_products};

enum UiEvent {
    Detection(ProductId, Detection),
    Resolution(ProductId, Result<InstallPlan, String>),
    Operation(OperationUpdate),
    ProductFinished {
        result: ProductOperationResult,
        log_warning: Option<String>,
    },
    ProductScanFinished(ProductId),
}

#[derive(Debug, Clone)]
struct ProductTask {
    state: OperationState,
    download_progress: Option<f32>,
    cancel: Arc<AtomicBool>,
    cancel_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProductRowAction {
    Open(ProductId),
    Cancel(ProductId),
}

pub struct InstallerApp {
    platform: PlatformInfo,
    registry: Option<TrustRegistry>,
    products: Vec<ProductView>,
    event_sender: Sender<UiEvent>,
    event_receiver: Receiver<UiEvent>,
    scanning: bool,
    pending_scans: HashSet<ProductId>,
    confirmation_open: bool,
    batch_summary: Option<String>,
    registry_error: Option<String>,
    tasks: HashMap<ProductId, ProductTask>,
    execution_gate: Arc<InstallExecutionGate>,
    operation_log: Option<Arc<OperationLog>>,
    log_warning: Arc<Mutex<Option<String>>>,
    close_confirmation_open: bool,
    close_after_tasks: bool,
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
        let (operation_log, log_warning) = match OperationLog::open_default() {
            Ok(log) => (Some(Arc::new(log)), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let mut app = Self {
            platform,
            registry: registry.ok(),
            products,
            event_sender,
            event_receiver,
            scanning: false,
            pending_scans: HashSet::new(),
            confirmation_open: false,
            batch_summary: None,
            registry_error,
            tasks: HashMap::new(),
            execution_gate: Arc::new(InstallExecutionGate::default()),
            operation_log,
            log_warning: Arc::new(Mutex::new(log_warning)),
            close_confirmation_open: false,
            close_after_tasks: false,
        };
        app.start_scan();
        app
    }

    fn start_scan(&mut self) {
        if !self.begin_scan_state() {
            return;
        }
        if self.platform.os == OperatingSystem::Windows {
            self.spawn_windows_full_scan();
            return;
        }
        for product in ProductId::ALL {
            self.spawn_product_scan(product);
        }
    }

    fn spawn_windows_full_scan(&self) {
        for products in [
            vec![ProductId::Claude, ProductId::ChatGpt],
            vec![ProductId::WorkBuddy, ProductId::Hermes, ProductId::CcSwitch],
        ] {
            let sender = self.event_sender.clone();
            let platform = self.platform.clone();
            let registry = self.registry.clone();
            thread::spawn(move || {
                let detections = detect_products(&platform, registry.as_ref(), &products);
                for product in products {
                    let detection = detections.get(&product).cloned().unwrap_or_else(|| {
                        Detection::failed(format!("{} 缺少本机检测结果", product.display_name()))
                    });
                    if sender.send(UiEvent::Detection(product, detection)).is_err() {
                        return;
                    }

                    let sender = sender.clone();
                    let platform = platform.clone();
                    let registry = registry.clone();
                    thread::spawn(move || {
                        let resolution =
                            resolve_product_plan(product, &platform, registry.as_ref());
                        if sender
                            .send(UiEvent::Resolution(product, resolution))
                            .is_err()
                        {
                            return;
                        }
                        let _ = sender.send(UiEvent::ProductScanFinished(product));
                    });
                }
            });
        }
    }

    fn start_product_scan(&mut self, product: ProductId) {
        if self.pending_scans.contains(&product) || self.tasks.contains_key(&product) {
            return;
        }
        self.pending_scans.insert(product);
        self.scanning = true;
        if let Some(view) = self
            .products
            .iter_mut()
            .find(|view| view.product == product)
        {
            view.result_unknown = false;
            view.install_plan = None;
            view.status_line = "正在重新检测安装状态与可用版本…".into();
        }
        self.spawn_product_scan(product);
    }

    fn spawn_product_scan(&self, product: ProductId) {
        let sender = self.event_sender.clone();
        let platform = self.platform.clone();
        let registry = self.registry.clone();
        thread::spawn(move || {
            let trust = registry
                .as_ref()
                .and_then(|registry| registry.find(product, platform.os, platform.architecture));
            let detection = detect_product(product, trust);
            if sender.send(UiEvent::Detection(product, detection)).is_err() {
                return;
            }
            let resolution = resolve_product_plan(product, &platform, registry.as_ref());
            if sender
                .send(UiEvent::Resolution(product, resolution))
                .is_err()
            {
                return;
            }
            let _ = sender.send(UiEvent::ProductScanFinished(product));
        });
    }

    fn begin_scan_state(&mut self) -> bool {
        if self.scanning || !self.tasks.is_empty() {
            return false;
        }
        self.scanning = true;
        self.pending_scans.clear();
        self.pending_scans
            .extend(self.products.iter().map(|view| view.product));
        for product in &mut self.products {
            product.result_unknown = false;
            product.install_plan = None;
            product.status_line = "正在检测安装状态与可用版本…".into();
        }
        true
    }

    fn drain_events(&mut self) {
        let mut refresh_products = HashSet::new();
        while let Ok(event) = self.event_receiver.try_recv() {
            if let Some(product) = self.apply_event(event) {
                refresh_products.insert(product);
            }
        }
        for product in refresh_products {
            self.start_product_scan(product);
        }
    }

    fn apply_event(&mut self, event: UiEvent) -> Option<ProductId> {
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
                    view.status_line = if detection.is_failed() {
                        detection.evidence.clone()
                    } else if detection.installed {
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
                None
            }
            UiEvent::Resolution(product, resolution) => {
                if let Some(view) = self
                    .products
                    .iter_mut()
                    .find(|view| view.product == product)
                {
                    match resolution {
                        Ok(plan) => {
                            view.install_plan = Some(plan);
                        }
                        Err(error) => {
                            view.status_line.push_str(&format!(" · 版本解析：{error}"));
                            view.install_plan = None;
                        }
                    }
                }
                None
            }
            UiEvent::Operation(update) => {
                if let Some(task) = self.tasks.get_mut(&update.product) {
                    task.state = update.state;
                    task.download_progress = if update.state == OperationState::Downloading {
                        parse_download_progress(&update.message)
                    } else {
                        None
                    };
                }
                if let Some(view) = self
                    .products
                    .iter_mut()
                    .find(|view| view.product == update.product)
                {
                    view.result_unknown =
                        update.state == crate::core::OperationState::ResultUnknown;
                    view.status_line = format!("{} · {}", update.state.label(), update.message);
                }
                None
            }
            UiEvent::ProductFinished {
                result,
                log_warning,
            } => {
                self.tasks.remove(&result.product);
                let mut summary = match result.state {
                    OperationState::Succeeded => {
                        format!("{}：{}", result.product.display_name(), result.message)
                    }
                    OperationState::Failed => {
                        format!(
                            "{} 安装失败，请查看对应项目的错误详情",
                            result.product.display_name()
                        )
                    }
                    OperationState::ResultUnknown => format!(
                        "{} 的安装结果尚未确认，请稍后刷新状态",
                        result.product.display_name()
                    ),
                    OperationState::Cancelled => {
                        format!("{} 的任务已取消", result.product.display_name())
                    }
                    _ => format!("{} 的任务已结束", result.product.display_name()),
                };
                if let Some(warning) = log_warning {
                    summary.push_str(&format!(" · 操作日志不可用：{warning}"));
                }
                self.batch_summary = Some(summary);
                if let Some(view) = self
                    .products
                    .iter_mut()
                    .find(|view| view.product == result.product)
                {
                    view.selected = false;
                }
                if self.tasks.is_empty() {
                    self.close_confirmation_open = false;
                }
                (result.state == OperationState::Succeeded).then_some(result.product)
            }
            UiEvent::ProductScanFinished(product) => {
                self.pending_scans.remove(&product);
                self.scanning = !self.pending_scans.is_empty();
                None
            }
        }
    }

    fn open_confirmation_for(&mut self, product: ProductId) -> bool {
        if self.pending_scans.contains(&product) || self.tasks.contains_key(&product) {
            return false;
        }
        for view in &mut self.products {
            view.selected = view.product == product;
        }
        self.confirmation_open = true;
        true
    }

    fn start_install_task(&mut self) {
        let plan = self
            .products
            .iter()
            .filter(|view| view.selected && view.support.can_install())
            .filter_map(|view| view.install_plan.clone())
            .next();
        let Some(registry) = self.registry.clone() else {
            self.batch_summary = Some("信任注册表不可用，无法安装".into());
            return;
        };
        let Some(plan) = plan else {
            self.batch_summary = Some("所选产品没有可执行的可信安装计划".into());
            return;
        };
        let product = plan.product();
        if self.tasks.contains_key(&product) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.tasks.insert(
            product,
            ProductTask {
                state: OperationState::Ready,
                download_progress: None,
                cancel: cancel.clone(),
                cancel_requested: false,
            },
        );
        self.batch_summary = None;
        for view in &mut self.products {
            if view.product == product {
                view.status_line = "准备执行安装任务…".into();
            }
            view.selected = false;
        }
        let sender = self.event_sender.clone();
        let platform = self.platform.clone();
        let execution_gate = self.execution_gate.clone();
        let operation_log = self.operation_log.clone();
        let log_warning = self.log_warning.clone();
        thread::spawn(move || {
            let warning_for_updates = log_warning.clone();
            if let Some(log) = &operation_log {
                let audit_update = OperationUpdate {
                    product,
                    state: OperationState::Ready,
                    message: install_plan_audit_message(&plan),
                };
                if let Err(error) = log.record(&audit_update)
                    && let Ok(mut warning) = warning_for_updates.lock()
                    && warning.is_none()
                {
                    *warning = Some(error.to_string());
                }
            }
            let fallback_platform = platform.clone();
            let fallback_registry = registry.clone();
            let result = run_install_plan(
                plan,
                platform,
                registry,
                cancel,
                execution_gate.as_ref(),
                |candidate| {
                    let supports_fallback = candidate.product == ProductId::Claude
                        || candidate.product == ProductId::ChatGpt
                            && fallback_platform.os == OperatingSystem::MacOs;
                    if !supports_fallback {
                        return Ok(None);
                    }
                    resolve_verified_download_fallback(
                        candidate,
                        &fallback_platform,
                        &fallback_registry,
                    )
                    .map(Some)
                    .map_err(|error| error.to_string())
                },
                |update| {
                    if let Some(log) = &operation_log
                        && let Err(error) = log.record(&update)
                        && let Ok(mut warning) = warning_for_updates.lock()
                        && warning.is_none()
                    {
                        *warning = Some(error.to_string());
                    }
                    let _ = sender.send(UiEvent::Operation(update));
                },
            );
            let log_warning = log_warning.lock().ok().and_then(|warning| warning.clone());
            let _ = sender.send(UiEvent::ProductFinished {
                result,
                log_warning,
            });
        });
    }

    fn request_cancel(&mut self, product: ProductId) {
        let Some(task) = self.tasks.get_mut(&product) else {
            return;
        };
        if task.cancel_requested || !operation_can_cancel(Some(task.state)) {
            return;
        }
        task.cancel_requested = true;
        task.cancel.store(true, Ordering::Relaxed);
        if let Some(view) = self
            .products
            .iter_mut()
            .find(|view| view.product == product)
        {
            view.status_line = "正在取消当前任务，请稍候…".into();
        }
    }

    fn request_cancel_all(&mut self) {
        let products = self.tasks.keys().copied().collect::<Vec<_>>();
        for product in products {
            self.request_cancel(product);
        }
    }
}

impl eframe::App for InstallerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();
        if self.close_after_tasks && self.tasks.is_empty() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.tasks.is_empty() && ui.ctx().input(|input| input.viewport().close_requested()) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_confirmation_open = true;
        }
        if self.scanning || !self.tasks.is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(
                    if self.tasks.is_empty() { 100 } else { 50 },
                ));
        }

        configure_visuals(ui.ctx());
        let panel = egui::Frame::central_panel(ui.style()).fill(Color32::WHITE);
        panel.show(ui, |ui| {
            let content_width = 620.0_f32.min((ui.available_width() - 32.0).max(540.0));
            let left = ui.max_rect().center().x - content_width / 2.0;
            let content_rect = egui::Rect::from_min_max(
                egui::pos2(left, ui.max_rect().top()),
                egui::pos2(left + content_width, ui.max_rect().bottom()),
            );
            let mut row_action = None;

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(content_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.add_space(26.0);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            draw_brand_icon(ui, 42.0);
                            ui.add_space(11.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("easy agent")
                                        .size(28.0)
                                        .strong()
                                        .color(Color32::from_rgb(18, 22, 29)),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("常用AI客户端  一键安装  官方来源")
                                        .size(14.0)
                                        .color(Color32::from_rgb(92, 99, 110)),
                                );
                            });
                        });
                    });
                    ui.add_space(22.0);

                    egui::ScrollArea::vertical()
                        .max_height(411.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let mut ordered_products = self.products.iter().collect::<Vec<_>>();
                            ordered_products
                                .sort_by_key(|view| support_display_rank(&view.support));
                            for view in ordered_products {
                                if let Some(action) = draw_product_row(
                                    ui,
                                    view,
                                    self.pending_scans.contains(&view.product),
                                    self.tasks.get(&view.product),
                                ) {
                                    row_action = Some(action);
                                }
                            }
                        });

                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.label(
                                RichText::new(friendly_platform_description(&self.platform))
                                    .size(12.0)
                                    .color(Color32::from_rgb(112, 117, 126)),
                            );
                            ui.label(
                                RichText::new("·")
                                    .size(12.0)
                                    .color(Color32::from_rgb(190, 193, 199)),
                            );
                            let refresh = ui.add_enabled(
                                !self.scanning && self.tasks.is_empty(),
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
                            if !self.tasks.is_empty() {
                                ui.spinner();
                                ui.label(
                                    RichText::new(format!("{} 个任务进行中", self.tasks.len()))
                                        .size(12.0)
                                        .color(Color32::from_rgb(112, 117, 126)),
                                );
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

            match row_action {
                Some(ProductRowAction::Open(product)) => {
                    self.open_confirmation_for(product);
                }
                Some(ProductRowAction::Cancel(product)) => self.request_cancel(product),
                None => {}
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
                    let verb = if view.detection.installed {
                        "更新"
                    } else {
                        "安装"
                    };
                    format!("{} {}", verb, view.product.display_name())
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
                        RichText::new("请确认当前状态和目标版本")
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
                                            let version = match plan {
                                                InstallPlan::DirectPackage(candidate) => format!(
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
                                                InstallPlan::MicrosoftStore(_) => view
                                                    .detection
                                                    .version
                                                    .as_deref()
                                                    .map(|version| {
                                                        format!(
                                                            "当前版本 {version} → 安装微软提供的最新版本"
                                                        )
                                                    })
                                                    .unwrap_or_else(|| {
                                                        "未安装 → 安装微软提供的最新版本".into()
                                                    }),
                                            };
                                            ui.label(
                                                RichText::new(version)
                                                    .size(13.0)
                                                    .color(Color32::from_rgb(70, 77, 88)),
                                            );
                                            if let Some(notice) = product_install_notice(
                                                view.product,
                                                &self.platform,
                                                plan,
                                            ) {
                                                ui.add_space(3.0);
                                                ui.label(
                                                    RichText::new(notice)
                                                        .size(12.0)
                                                        .color(Color32::from_rgb(154, 94, 42)),
                                                );
                                            }
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
                                    "将检查文件完整性、平台签名、应用身份、版本和芯片架构。验证失败不会启动安装；安装异常后会重新检测实际状态。",
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
                self.start_install_task();
            }
        }

        if self.close_confirmation_open {
            let mut keep_open = true;
            let can_cancel = !self.tasks.is_empty()
                && self
                    .tasks
                    .values()
                    .all(|task| operation_can_cancel(Some(task.state)));
            let cancel_requested = self.tasks.values().all(|task| task.cancel_requested);
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
                            "当前任务都还在下载、验证或排队。可以继续等待，或全部安全取消后退出。"
                        } else {
                            "至少一个应用正在写入系统或复检，当前不能安全退出。请等待关键步骤完成。"
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
                                RichText::new(if cancel_requested {
                                    "取消完成后退出"
                                } else {
                                    "取消全部并退出"
                                })
                                .size(13.5),
                            )
                            .min_size(egui::vec2(126.0, 36.0));
                            if ui.add(cancel_and_close).clicked() {
                                self.request_cancel_all();
                                self.close_after_tasks = true;
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
    task: Option<&ProductTask>,
) -> Option<ProductRowAction> {
    let active_state = task.map(|task| task.state);
    let row_height = 82.0;
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
                egui::vec2((rect.width() - 184.0).max(260.0), 58.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.label(
                        RichText::new(view.product.display_name())
                            .size(17.5)
                            .color(Color32::from_rgb(24, 28, 35)),
                    );
                    ui.add_space(4.0);
                    let subtitle = product_subtitle(view, scanning, active_state);
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
                    if let Some(progress) = task.and_then(|task| task.download_progress) {
                        ui.add_space(5.0);
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(ui.available_width())
                                .desired_height(5.0)
                                .fill(Color32::from_rgb(44, 105, 200))
                                .animate(true),
                        );
                    } else if active_state.is_some() {
                        ui.add_space(5.0);
                        draw_indeterminate_progress(ui);
                    }
                },
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cancellable = task.is_some_and(|task| operation_can_cancel(Some(task.state)));
                let cancel_requested = task.is_some_and(|task| task.cancel_requested);
                let (label, enabled) = if let Some(state) = active_state {
                    if cancellable {
                        (
                            if cancel_requested {
                                "正在取消"
                            } else {
                                "取消"
                            },
                            !cancel_requested,
                        )
                    } else {
                        (operation_button_label(state), false)
                    }
                } else {
                    product_action(view, scanning)
                };
                let blue = Color32::from_rgb(44, 105, 200);
                let action_color = if cancellable {
                    Color32::from_rgb(178, 66, 56)
                } else {
                    blue
                };
                let button =
                    egui::Button::new(RichText::new(label).size(14.5).strong().color(if enabled {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(112, 119, 130)
                    }))
                    .min_size(egui::vec2(90.0, 36.0))
                    .fill(if enabled {
                        action_color
                    } else {
                        Color32::from_rgb(247, 248, 250)
                    })
                    .stroke(egui::Stroke::new(
                        1.0,
                        if enabled {
                            action_color
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
    clicked.then_some(if task.is_some() {
        ProductRowAction::Cancel(view.product)
    } else {
        ProductRowAction::Open(view.product)
    })
}

fn product_subtitle(
    view: &ProductView,
    scanning: bool,
    active_state: Option<OperationState>,
) -> String {
    if view.result_unknown {
        return view.status_line.clone();
    }
    if active_state.is_some() {
        return view.status_line.clone();
    }
    if view.detection.is_failed() {
        return "本机安装状态检测失败".into();
    }
    if scanning && view.install_plan.is_none() {
        if view.detection.evidence == "尚未检测" {
            return "正在检测本机安装状态".into();
        }
        return if view.detection.installed {
            match view.detection.version.as_deref() {
                Some(version) => format!(
                    "{} · 正在获取最新版本",
                    installed_version_summary(view, version)
                ),
                None => "已检测到安装 · 正在获取最新版本".into(),
            }
        } else {
            "未安装 · 正在获取最新版本".into()
        };
    }
    match &view.install_plan {
        Some(InstallPlan::DirectPackage(latest)) => {
            match (view.detection.installed, view.detection.version.as_deref()) {
                (true, Some(installed)) => {
                    let installed_summary = installed_version_summary(view, installed);
                    if version_is_older_for_product(view.product, installed, &latest.version) {
                        format!(
                            "{installed_summary} · 可更新至 {}",
                            display_version(view.product, &latest.version)
                        )
                    } else {
                        format!("{installed_summary} · 已是最新版本")
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
                (true, Some(installed)) => {
                    format!("已安装 {installed} · 可检查并安装更新")
                }
                (true, None) => "已检测到安装 · 版本未知".into(),
                (false, _) => "可安装最新版本".into(),
            }
        }
        None => match &view.support {
            SupportState::Disabled(_) => disabled_product_summary(view.product).into(),
            SupportState::Unsupported(_) => "当前系统或芯片不受官方支持".into(),
            SupportState::Ready => {
                if let Some(reason) = product_resolution_failure_summary(view) {
                    return match (view.detection.installed, view.detection.version.as_deref()) {
                        (true, Some(version)) => {
                            format!("{} · {reason}", installed_version_summary(view, version))
                        }
                        (true, None) => format!("已检测到安装 · {reason}"),
                        (false, _) => reason.into(),
                    };
                }
                match (view.detection.installed, view.detection.version.as_deref()) {
                    (true, Some(version)) => format!(
                        "{} · 暂时无法获取最新版本",
                        installed_version_summary(view, version)
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

fn product_action(view: &ProductView, scanning: bool) -> (&'static str, bool) {
    if view.result_unknown {
        return ("待复检", false);
    }
    if view.detection.is_failed() {
        return ("检测失败", false);
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
                            if detection_location(view) == Some(DetectedLocation::UserApplications)
                            {
                                ("个人位置", false)
                            } else {
                                ("已安装", false)
                            }
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

fn resolve_product_plan(
    product: ProductId,
    platform: &PlatformInfo,
    registry: Option<&TrustRegistry>,
) -> Result<InstallPlan, String> {
    registry
        .ok_or_else(|| "信任注册表不可用".to_owned())
        .and_then(|registry| {
            resolve_install_plan(product, platform, registry).map_err(|error| error.to_string())
        })
}

fn product_detail(view: &ProductView) -> String {
    match &view.support {
        SupportState::Disabled(reason) | SupportState::Unsupported(reason) => reason.clone(),
        SupportState::Ready if view.detection.is_failed() => view.status_line.clone(),
        SupportState::Ready => product_resolution_failure_summary(view)
            .map(str::to_owned)
            .unwrap_or_else(|| view.status_line.clone()),
    }
}

fn product_resolution_failure_summary(view: &ProductView) -> Option<&'static str> {
    if view.product != ProductId::Claude || !view.status_line.contains("版本解析：") {
        return None;
    }
    let detail = view.status_line.to_ascii_lowercase();
    if detail.contains("certificate")
        && [
            "verify",
            "verification",
            "invalid",
            "expired",
            "not valid",
            "issuer",
            "untrusted",
            "not trusted",
        ]
        .iter()
        .any(|marker| detail.contains(marker))
    {
        Some("下载连接校验失败")
    } else if detail.contains("verified mirror unavailable") {
        Some("最新版本暂时不可用")
    } else if [
        "region is unavailable",
        "http 403",
        "http 451",
        "http 408",
        "http 429",
        "timed out",
        "timeout",
        "connect",
        "error sending request",
    ]
    .iter()
    .any(|marker| detail.contains(marker))
    {
        Some("当前网络无法获取最新版本")
    } else {
        None
    }
}

fn disabled_product_summary(product: ProductId) -> &'static str {
    match product {
        ProductId::Claude => "当前暂不可用",
        ProductId::Hermes => "官方安装流程暂未完成验证",
        _ => "当前版本暂未完成安全验证",
    }
}

fn install_plan_audit_message(plan: &InstallPlan) -> String {
    match plan {
        InstallPlan::DirectPackage(candidate) => {
            let source = match candidate.source {
                ArtifactSource::Official => "official".to_owned(),
                ArtifactSource::VerifiedMirror { synced_at_unix } => {
                    format!("verified_mirror synced_at_unix={synced_at_unix}")
                }
            };
            format!(
                "artifact_source={source} artifact_host={} target_version={} architecture={:?} package_kind={:?}",
                candidate.download_url.host_str().unwrap_or("unknown"),
                candidate.version,
                candidate.architecture,
                candidate.package_kind
            )
        }
        InstallPlan::MicrosoftStore(plan) => format!(
            "artifact_source=microsoft_web_installer target_architecture={:?}",
            plan.architecture
        ),
    }
}

fn product_install_notice(
    product: ProductId,
    platform: &PlatformInfo,
    plan: &InstallPlan,
) -> Option<&'static str> {
    if product == ProductId::Claude
        && platform.os == OperatingSystem::Windows
        && matches!(
            plan,
            InstallPlan::DirectPackage(candidate) if candidate.package_kind == PackageKind::Msix
        )
    {
        Some(
            "将直接部署已下载并验证的 Claude 完整 MSIX，安装阶段不再联网。Windows 可能显示管理员授权；Cowork 仍可能要求启用虚拟机平台并重启。",
        )
    } else if product == ProductId::ChatGpt
        && platform.os == OperatingSystem::Windows
        && matches!(plan, InstallPlan::MicrosoftStore(_))
    {
        Some(
            "ChatGPT 会打开微软安装器；按提示完成即可。普通安装不可用时，程序会自动准备完整安装包。",
        )
    } else {
        None
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetectedLocation {
    UserApplications,
    SystemApplications,
}

fn detection_location(view: &ProductView) -> Option<DetectedLocation> {
    if view.detection.evidence.contains("用户 Applications") {
        Some(DetectedLocation::UserApplications)
    } else if view.detection.evidence.contains("系统 Applications") {
        Some(DetectedLocation::SystemApplications)
    } else {
        None
    }
}

fn installed_version_summary(view: &ProductView, version: &str) -> String {
    let version = display_version(view.product, version);
    match detection_location(view) {
        Some(DetectedLocation::UserApplications) => {
            format!("检测到 {version} · 个人 Applications")
        }
        Some(DetectedLocation::SystemApplications) => {
            format!("已安装 {version} · 系统 Applications")
        }
        None => format!("已安装 {version}"),
    }
}

const fn operation_button_label(state: OperationState) -> &'static str {
    match state {
        OperationState::Ready => "准备中",
        OperationState::Downloading => "下载中",
        OperationState::Verifying => "验证中",
        OperationState::Queued => "等待安装",
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
        Some(
            OperationState::Ready
                | OperationState::Downloading
                | OperationState::Verifying
                | OperationState::Queued
        )
    )
}

fn parse_download_progress(message: &str) -> Option<f32> {
    let before_percent = message.rsplit_once('%')?.0;
    let percent = before_percent
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .rfind(|part| !part.is_empty())?
        .parse::<f32>()
        .ok()?;
    Some((percent / 100.0).clamp(0.0, 1.0))
}

fn draw_indeterminate_progress(ui: &mut egui::Ui) {
    let width = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 5.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 3.0, Color32::from_rgb(231, 236, 244));

    let segment_width = (rect.width() * 0.22).clamp(28.0, 92.0);
    let phase = (ui.input(|input| input.time) * 0.72).fract() as f32;
    let left = rect.left() - segment_width + phase * (rect.width() + segment_width);
    let segment = egui::Rect::from_min_size(
        egui::pos2(left, rect.top()),
        egui::vec2(segment_width, rect.height()),
    );
    ui.painter()
        .with_clip_rect(rect)
        .rect_filled(segment, 3.0, Color32::from_rgb(44, 105, 200));
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

fn install_location_summary(platform: &PlatformInfo) -> &'static str {
    match platform.os {
        OperatingSystem::MacOs => {
            "安装包：验证通过后保存到系统“下载”目录；应用首次安装优先使用系统 Applications。"
        }
        OperatingSystem::Windows => {
            "安装包：普通客户端验证后保存到系统“下载”目录；ChatGPT 由微软安装器或临时完整包完成安装。"
        }
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
    let visual_size = match product {
        ProductId::Hermes => 34.0,
        ProductId::Claude => 38.0,
        ProductId::ChatGpt | ProductId::WorkBuddy => 40.0,
        ProductId::CcSwitch => 37.0,
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 9.0, Color32::from_rgb(248, 249, 251));
    let image_rect =
        egui::Rect::from_center_size(rect.center(), egui::vec2(visual_size, visual_size));
    ui.put(
        image_rect,
        egui::Image::new(source)
            .fit_to_exact_size(egui::vec2(visual_size, visual_size))
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
        .insert(0, "system-cjk".into());
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
    use crate::core::{ArtifactSource, ReleaseCandidate};

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
            install_plan: Some(InstallPlan::MicrosoftStore(
                crate::core::MicrosoftStorePlan {
                    product: ProductId::ChatGpt,
                    architecture: Architecture::X64,
                    store_id: "9PLM9XGG6VKS".into(),
                },
            )),
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
                source: ArtifactSource::Official,
                minimum_macos_version: None,
                expected_size: None,
                expected_sha256: None,
                detached_signature: None,
                bootstrap_payload: None,
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
            pending_scans: HashSet::new(),
            confirmation_open: false,
            batch_summary: None,
            registry_error: None,
            tasks: HashMap::new(),
            execution_gate: Arc::new(InstallExecutionGate::default()),
            operation_log: None,
            log_warning: Arc::new(Mutex::new(None)),
            close_confirmation_open: false,
            close_after_tasks: false,
        }
    }

    fn test_task(state: OperationState) -> ProductTask {
        ProductTask {
            state,
            download_progress: None,
            cancel: Arc::new(AtomicBool::new(false)),
            cancel_requested: false,
        }
    }

    #[test]
    fn one_running_product_does_not_block_another_product() {
        let mut app = test_app();
        assert_eq!(
            product_subtitle(&app.products[0], false, None),
            "已安装 26.721.11231.0 · 可检查并安装更新"
        );
        assert_eq!(product_action(&app.products[0], false), ("更新", true));
        assert_eq!(product_action(&app.products[1], false), ("安装", true));
        assert_eq!(product_action(&app.products[1], true), ("检测中", false));
        assert!(app.open_confirmation_for(ProductId::ChatGpt));
        assert!(app.products[0].selected);
        app.confirmation_open = false;
        app.tasks
            .insert(ProductId::ChatGpt, test_task(OperationState::Downloading));

        assert!(!app.open_confirmation_for(ProductId::ChatGpt));
        assert!(app.open_confirmation_for(ProductId::WorkBuddy));
        assert!(app.confirmation_open);
        assert!(!app.products[0].selected);
        assert!(app.products[1].selected);
    }

    #[test]
    fn one_slow_product_scan_does_not_block_a_ready_product_action() {
        let mut app = test_app();
        app.scanning = true;
        app.pending_scans.insert(ProductId::WorkBuddy);

        assert!(app.open_confirmation_for(ProductId::ChatGpt));
        assert!(!app.open_confirmation_for(ProductId::WorkBuddy));
        assert_eq!(product_action(&app.products[0], false), ("更新", true));
        assert_eq!(product_action(&app.products[1], true), ("检测中", false));
    }

    #[test]
    fn local_detection_result_is_visible_while_latest_version_is_loading() {
        let mut absent = workbuddy_view();
        absent.install_plan = None;
        assert_eq!(
            product_subtitle(&absent, true, None),
            "未安装 · 正在获取最新版本"
        );

        let mut installed = chatgpt_view();
        installed.install_plan = None;
        assert_eq!(
            product_subtitle(&installed, true, None),
            "已安装 26.721.11231.0 · 正在获取最新版本"
        );
    }

    #[test]
    fn failed_local_detection_is_not_presented_as_installable() {
        let mut view = workbuddy_view();
        view.detection = Detection::failed("Windows 本机检测超过 20 秒");
        view.status_line = view.detection.evidence.clone();

        assert_eq!(product_subtitle(&view, false, None), "本机安装状态检测失败");
        assert_eq!(product_action(&view, false), ("检测失败", false));
        assert!(product_detail(&view).contains("超过 20 秒"));
    }

    #[test]
    fn claude_windows_msix_discloses_offline_install_and_cowork_scope() {
        let plan = InstallPlan::DirectPackage(ReleaseCandidate {
            product: ProductId::Claude,
            version: "1.26832.0".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Msix,
            download_url: url::Url::parse(
                "https://downloads.claude.ai/releases/win32/x64/1.26832.0/Claude.msix",
            )
            .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: None,
            expected_size: None,
            expected_sha256: None,
            detached_signature: None,
            bootstrap_payload: None,
        });
        let platform = PlatformInfo {
            os: OperatingSystem::Windows,
            architecture: Architecture::X64,
            os_version: None,
            description: "windows / x64".into(),
        };

        assert!(
            product_install_notice(ProductId::Claude, &platform, &plan).is_some_and(|notice| {
                notice.contains("安装阶段不再联网") && notice.contains("Cowork")
            })
        );
        assert!(install_plan_audit_message(&plan).contains("artifact_source=official"));

        let mut mirror_plan = plan.clone();
        let InstallPlan::DirectPackage(candidate) = &mut mirror_plan else {
            unreachable!();
        };
        candidate.source = ArtifactSource::VerifiedMirror {
            synced_at_unix: 1_800_000_000,
        };
        assert!(
            install_plan_audit_message(&mirror_plan).contains("artifact_source=verified_mirror")
        );
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
        assert_eq!(product_action(&view, false), ("更新", true));
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
            product_subtitle(&view, false, None),
            "当前版本暂未完成安全验证"
        );
        assert_eq!(
            product_detail(&view),
            "官方摘要与下载文件不一致，安装保持禁用"
        );
        assert_eq!(product_action(&view, false), ("暂不可用", false));

        view.support = SupportState::Unsupported("厂商明确不支持 Intel Mac".into());
        assert_eq!(
            product_subtitle(&view, false, None),
            "当前系统或芯片不受官方支持"
        );
        assert_eq!(product_detail(&view), "厂商明确不支持 Intel Mac");
        assert_eq!(product_action(&view, false), ("不支持", false));

        view.support = SupportState::Ready;
        view.status_line = "已安装 5.3.8 · 版本解析：server returned HTTP 403".into();
        assert_eq!(
            product_subtitle(&view, false, None),
            "已安装 5.3.8 · 暂时无法获取最新版本"
        );
        assert_eq!(product_detail(&view), view.status_line);

        view.product = ProductId::Claude;
        view.detection = Detection::absent("fixture");
        view.status_line =
            "未检测到安装 · fixture · 版本解析：server returned HTTP 403 Forbidden".into();
        assert_eq!(
            product_subtitle(&view, false, None),
            "当前网络无法获取最新版本"
        );
        view.status_line = "未检测到安装 · fixture · 版本解析：official source unavailable (server returned HTTP 403); verified mirror unavailable (server returned HTTP 503)".into();
        assert_eq!(product_subtitle(&view, false, None), "最新版本暂时不可用");
    }

    #[test]
    fn download_progress_and_cancel_boundaries_are_ui_safe() {
        let progress = parse_download_progress("已下载 65.8% (260.3/395.7 MiB)").unwrap();
        assert!((progress - 0.658).abs() < 0.0001);
        let store_progress =
            parse_download_progress("下载 Microsoft.DesktopAppInstaller.msixbundle：42.5%")
                .unwrap();
        assert!((store_progress - 0.425).abs() < 0.0001);
        assert_eq!(parse_download_progress("已下载 12.0 MiB"), None);
        assert!(operation_can_cancel(Some(OperationState::Downloading)));
        assert!(operation_can_cancel(Some(OperationState::Verifying)));
        assert!(operation_can_cancel(Some(OperationState::Queued)));
        assert!(!operation_can_cancel(Some(OperationState::Installing)));
        assert!(!operation_can_cancel(Some(OperationState::Postchecking)));
    }

    #[test]
    fn a_new_download_phase_does_not_reuse_the_previous_files_completed_progress() {
        let mut app = test_app();
        let mut task = test_task(OperationState::Downloading);
        task.download_progress = Some(1.0);
        app.tasks.insert(ProductId::ChatGpt, task);

        app.apply_event(UiEvent::Operation(OperationUpdate {
            product: ProductId::ChatGpt,
            state: OperationState::Downloading,
            message: "正在下载下一项已验证安装包".into(),
        }));
        assert_eq!(
            app.tasks
                .get(&ProductId::ChatGpt)
                .unwrap()
                .download_progress,
            None
        );

        app.apply_event(UiEvent::Operation(OperationUpdate {
            product: ProductId::ChatGpt,
            state: OperationState::Downloading,
            message: "已下载 8.0% (8.0/100.0 MiB)".into(),
        }));
        assert_eq!(
            app.tasks
                .get(&ProductId::ChatGpt)
                .unwrap()
                .download_progress,
            Some(0.08)
        );
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
        assert_eq!(product_action(&view, false), ("已安装", false));
        assert_eq!(
            product_subtitle(&view, false, None),
            "已安装 5.3.8 · 已是最新版本"
        );
    }

    #[test]
    fn user_applications_install_is_not_presented_as_a_system_install() {
        let mut view = workbuddy_view();
        view.detection = Detection {
            installed: true,
            version: Some("5.3.8".into()),
            managed: false,
            management_known: true,
            package_identity: Some("com.workbuddy.workbuddy".into()),
            package_family: None,
            publisher: Some("FN2V63AD2J".into()),
            architecture: Some(Architecture::X64),
            evidence: "用户 Applications · 已通过 Bundle/签名/Gatekeeper 检查".into(),
        };
        assert_eq!(
            product_subtitle(&view, false, None),
            "检测到 5.3.8 · 个人 Applications · 已是最新版本"
        );
        assert_eq!(product_action(&view, false), ("个人位置", false));
    }

    #[test]
    fn successful_product_finish_requests_only_that_product_rescan() {
        let mut app = test_app();
        app.tasks
            .insert(ProductId::ChatGpt, test_task(OperationState::Installing));
        app.products[0].selected = true;
        let refresh = app.apply_event(UiEvent::ProductFinished {
            result: ProductOperationResult {
                product: ProductId::ChatGpt,
                state: OperationState::Succeeded,
                message: "ok".into(),
            },
            log_warning: None,
        });

        assert_eq!(refresh, Some(ProductId::ChatGpt));
        assert!(app.tasks.is_empty());
        assert!(!app.products[0].selected);
        assert_eq!(app.batch_summary.as_deref(), Some("ChatGPT：ok"));
    }

    #[test]
    fn result_unknown_suppresses_auto_rescan_until_explicit_refresh() {
        let mut app = test_app();
        app.tasks
            .insert(ProductId::ChatGpt, test_task(OperationState::Installing));
        app.products[0].selected = true;
        app.apply_event(UiEvent::Operation(OperationUpdate {
            product: ProductId::ChatGpt,
            state: OperationState::ResultUnknown,
            message: "deployment continues".into(),
        }));
        assert_eq!(product_action(&app.products[0], false), ("待复检", false));

        let refresh = app.apply_event(UiEvent::ProductFinished {
            result: ProductOperationResult {
                product: ProductId::ChatGpt,
                state: OperationState::ResultUnknown,
                message: "unknown".into(),
            },
            log_warning: None,
        });
        assert_eq!(refresh, None);
        assert!(app.products[0].result_unknown);

        assert!(app.begin_scan_state());
        assert!(app.scanning);
        assert!(app.products.iter().all(|view| !view.result_unknown));
    }

    #[test]
    fn product_summary_reports_when_the_local_operation_log_is_unavailable() {
        let mut app = test_app();
        app.tasks
            .insert(ProductId::WorkBuddy, test_task(OperationState::Downloading));
        app.apply_event(UiEvent::Operation(OperationUpdate {
            product: ProductId::WorkBuddy,
            state: OperationState::Failed,
            message: "download failed: checksum mismatch".into(),
        }));
        let refresh = app.apply_event(UiEvent::ProductFinished {
            result: ProductOperationResult {
                product: ProductId::WorkBuddy,
                state: OperationState::Failed,
                message: "download failed".into(),
            },
            log_warning: Some("access denied".into()),
        });
        assert_eq!(refresh, None);
        assert_eq!(
            app.batch_summary.as_deref(),
            Some("WorkBuddy 安装失败，请查看对应项目的错误详情 · 操作日志不可用：access denied")
        );
        assert!(
            app.products
                .iter()
                .find(|view| view.product == ProductId::WorkBuddy)
                .unwrap()
                .status_line
                .contains("checksum mismatch")
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
                    "WorkBuddy 5.3.8 正在检测安装状态与可用版本更新不可用",
                )
            });
        });

        assert!(contains_required_glyphs);
    }
}
