use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use crate::platform::{
    ArtifactVerification, StoreInstallError, StoreInstallRequest, VerifiedInstallRequest,
    VerifiedInstallerPayload, detect_product, downloads_directory, execute_microsoft_store_install,
    execute_verified_installer, preflight_direct_install, verify_artifact,
};

use super::{
    ArtifactSource, Detection, DownloadControl, DownloadError, DownloadRequest, DownloadResult,
    InstallPlan, OperationState, OperationUpdate, PackageKind, PlatformInfo,
    ProductOperationResult, ReleaseCandidate, RemoteDigestPolicy, TrustEntry, TrustRegistry,
    download_error_allows_verified_fallback, download_to_private_staging_controlled,
    save_verified_download_copy, verify_configured_updater_signature_file,
};

const DIRECT_POSTCHECK_ATTEMPTS: usize = 46;
const DIRECT_POSTCHECK_INTERVAL: Duration = Duration::from_secs(2);
const INSTALL_GATE_CANCEL_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Default)]
pub struct InstallExecutionGate {
    state: Mutex<InstallExecutionGateState>,
    changed: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallExecutionLane {
    WindowsMsi,
    WindowsAppx,
}

#[derive(Debug, Default)]
struct InstallExecutionGateState {
    windows_msi: InstallExecutionLaneState,
    windows_appx: InstallExecutionLaneState,
}

#[derive(Debug, Default)]
struct InstallExecutionLaneState {
    active: bool,
    next_ticket: u64,
    queue: VecDeque<u64>,
}

struct InstallExecutionPermit<'a> {
    gate: &'a InstallExecutionGate,
    lane: InstallExecutionLane,
}

impl InstallExecutionGateState {
    fn lane_mut(&mut self, lane: InstallExecutionLane) -> &mut InstallExecutionLaneState {
        match lane {
            InstallExecutionLane::WindowsMsi => &mut self.windows_msi,
            InstallExecutionLane::WindowsAppx => &mut self.windows_appx,
        }
    }
}

impl InstallExecutionGate {
    fn acquire(
        &self,
        lane: InstallExecutionLane,
        cancel: &AtomicBool,
        mut on_wait: impl FnMut(),
    ) -> Result<InstallExecutionPermit<'_>, InstallOneError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| InstallOneError::Failed("安装队列状态异常".into()))?;
        let ticket = {
            let lane_state = state.lane_mut(lane);
            let ticket = lane_state.next_ticket;
            lane_state.next_ticket = lane_state.next_ticket.wrapping_add(1);
            lane_state.queue.push_back(ticket);
            ticket
        };
        let mut wait_announced = false;

        loop {
            if cancel.load(Ordering::Relaxed) {
                state
                    .lane_mut(lane)
                    .queue
                    .retain(|queued| *queued != ticket);
                self.changed.notify_all();
                return Err(InstallOneError::Cancelled(
                    "已取消等待安装，未写入系统".into(),
                ));
            }
            let can_acquire = {
                let lane_state = state.lane_mut(lane);
                !lane_state.active && lane_state.queue.front() == Some(&ticket)
            };
            if can_acquire {
                let lane_state = state.lane_mut(lane);
                lane_state.queue.pop_front();
                lane_state.active = true;
                return Ok(InstallExecutionPermit { gate: self, lane });
            }
            if !wait_announced {
                wait_announced = true;
                drop(state);
                on_wait();
                state = self
                    .state
                    .lock()
                    .map_err(|_| InstallOneError::Failed("安装队列状态异常".into()))?;
                continue;
            }
            let (next_state, _) = self
                .changed
                .wait_timeout(state, INSTALL_GATE_CANCEL_POLL)
                .map_err(|_| InstallOneError::Failed("安装队列状态异常".into()))?;
            state = next_state;
        }
    }
}

impl Drop for InstallExecutionPermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.gate.state.lock() {
            state.lane_mut(self.lane).active = false;
            self.gate.changed.notify_all();
        }
    }
}

const fn exclusive_install_lane(kind: PackageKind) -> Option<InstallExecutionLane> {
    match kind {
        PackageKind::Msi => Some(InstallExecutionLane::WindowsMsi),
        PackageKind::Msix => Some(InstallExecutionLane::WindowsAppx),
        _ => None,
    }
}

pub fn run_install_batch(
    plans: Vec<InstallPlan>,
    platform: PlatformInfo,
    registry: TrustRegistry,
    cancel: Arc<AtomicBool>,
    resolve_download_fallback: impl Fn(&ReleaseCandidate) -> Result<Option<ReleaseCandidate>, String>,
    on_update: impl Fn(OperationUpdate),
) -> Vec<ProductOperationResult> {
    let execution_gate = InstallExecutionGate::default();
    let mut results = Vec::with_capacity(plans.len());
    for plan in plans {
        results.push(run_install_plan(
            plan,
            platform.clone(),
            registry.clone(),
            cancel.clone(),
            &execution_gate,
            &resolve_download_fallback,
            &on_update,
        ));
    }
    results
}

pub fn run_install_plan(
    plan: InstallPlan,
    platform: PlatformInfo,
    registry: TrustRegistry,
    cancel: Arc<AtomicBool>,
    execution_gate: &InstallExecutionGate,
    resolve_download_fallback: impl Fn(&ReleaseCandidate) -> Result<Option<ReleaseCandidate>, String>,
    on_update: impl Fn(OperationUpdate),
) -> ProductOperationResult {
    let product = plan.product();
    if cancel.load(Ordering::Relaxed) {
        let result = ProductOperationResult {
            product,
            state: OperationState::Cancelled,
            message: "任务已取消，未开始下载或安装".into(),
        };
        on_update(OperationUpdate {
            product,
            state: result.state,
            message: result.message.clone(),
        });
        return result;
    }
    on_update(OperationUpdate {
        product,
        state: OperationState::Ready,
        message: "用户已确认，开始执行可信安装计划".into(),
    });
    let result = match install_one(
        &plan,
        &platform,
        &registry,
        &cancel,
        execution_gate,
        &resolve_download_fallback,
        &on_update,
    ) {
        Ok(message) => ProductOperationResult {
            product,
            state: OperationState::Succeeded,
            message,
        },
        Err(InstallOneError::Cancelled(message)) => ProductOperationResult {
            product,
            state: OperationState::Cancelled,
            message,
        },
        Err(InstallOneError::Failed(message)) => ProductOperationResult {
            product,
            state: OperationState::Failed,
            message,
        },
        Err(InstallOneError::ResultUnknown(message)) => ProductOperationResult {
            product,
            state: OperationState::ResultUnknown,
            message,
        },
    };
    on_update(OperationUpdate {
        product,
        state: result.state,
        message: result.message.clone(),
    });
    result
}

#[derive(Debug)]
enum InstallOneError {
    Cancelled(String),
    Failed(String),
    ResultUnknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreinstallDecision {
    Proceed,
    AlreadyCurrent(String),
    Reject(String),
}

fn install_one(
    plan: &InstallPlan,
    platform: &PlatformInfo,
    registry: &TrustRegistry,
    cancel: &Arc<AtomicBool>,
    execution_gate: &InstallExecutionGate,
    resolve_download_fallback: &impl Fn(&ReleaseCandidate) -> Result<Option<ReleaseCandidate>, String>,
    on_update: &impl Fn(OperationUpdate),
) -> Result<String, InstallOneError> {
    let product = plan.product();
    let trust = registry
        .find(product, platform.os, platform.architecture)
        .ok_or_else(|| InstallOneError::Failed("缺少当前平台的信任条目".into()))?;
    if !trust.enabled {
        return Err(InstallOneError::Failed(format!(
            "信任条目尚未启用：{}",
            trust.status_reason
        )));
    }
    if plan.architecture() != platform.architecture {
        return Err(InstallOneError::Failed("候选包架构与当前设备不一致".into()));
    }
    let current = detect_product(product, Some(trust));
    match plan {
        InstallPlan::DirectPackage(candidate) => install_direct_package(
            candidate,
            trust,
            cancel,
            execution_gate,
            resolve_download_fallback,
            on_update,
            &current,
        ),
        InstallPlan::MicrosoftStore(store_plan) => {
            let _permit =
                execution_gate.acquire(InstallExecutionLane::WindowsAppx, cancel, || {
                    on_update(OperationUpdate {
                        product,
                        state: OperationState::Queued,
                        message: "另一个 Windows 应用包正在写入系统，当前任务稍后自动继续".into(),
                    });
                })?;
            if cancel.load(Ordering::Relaxed) {
                return Err(InstallOneError::Cancelled(
                    "已在进入 ChatGPT 安装流程前取消".into(),
                ));
            }
            execute_microsoft_store_install(&StoreInstallRequest {
                plan: store_plan,
                trust,
                initial_detection: &current,
                cancel,
                on_update,
            })
            .map_err(|error| match error {
                StoreInstallError::Cancelled(message) => InstallOneError::Cancelled(message),
                StoreInstallError::ResultUnknown(message) => {
                    InstallOneError::ResultUnknown(message)
                }
                StoreInstallError::Failed(message) => InstallOneError::Failed(message),
            })
        }
    }
}

fn install_direct_package(
    candidate: &ReleaseCandidate,
    trust: &super::TrustEntry,
    cancel: &Arc<AtomicBool>,
    execution_gate: &InstallExecutionGate,
    resolve_download_fallback: &impl Fn(&ReleaseCandidate) -> Result<Option<ReleaseCandidate>, String>,
    on_update: &impl Fn(OperationUpdate),
    current: &Detection,
) -> Result<String, InstallOneError> {
    match assess_existing_install_for_product(
        current,
        candidate.product,
        &candidate.version,
        trust.allow_trusted_update_when_management_unknown,
    ) {
        PreinstallDecision::Proceed => {}
        PreinstallDecision::AlreadyCurrent(message) => return Ok(message),
        PreinstallDecision::Reject(message) => return Err(InstallOneError::Failed(message)),
    }
    preflight_direct_install(trust, candidate.architecture)
        .map_err(|error| InstallOneError::Failed(format!("安装前检查失败：{error}")))?;

    let mut active_candidate = candidate.clone();
    let download = match download_candidate(
        &active_candidate,
        trust,
        cancel,
        on_update,
        "正在下载安装程序",
    ) {
        Ok(download) => download,
        Err(DownloadError::Cancelled) => {
            return Err(InstallOneError::Cancelled(
                "下载已取消，未启动安装器".into(),
            ));
        }
        Err(official_error)
            if matches!(active_candidate.source, ArtifactSource::Official)
                && download_error_allows_verified_fallback(&official_error) =>
        {
            let fallback =
                resolve_download_fallback(&active_candidate).map_err(|fallback_error| {
                    InstallOneError::Failed(format!(
                        "下载失败：{official_error}；自动恢复失败：{fallback_error}"
                    ))
                })?;
            let Some(fallback) = fallback else {
                return Err(InstallOneError::Failed(format!(
                    "下载失败：{official_error}"
                )));
            };
            validate_verified_download_fallback(&active_candidate, &fallback)
                .map_err(InstallOneError::Failed)?;
            emit(
                on_update,
                &active_candidate,
                OperationState::Downloading,
                "当前网络中断，正在自动恢复下载",
            );
            active_candidate = fallback;
            match download_candidate(
                &active_candidate,
                trust,
                cancel,
                on_update,
                "正在从受验证备用节点下载安装程序",
            ) {
                Ok(download) => download,
                Err(DownloadError::Cancelled) => {
                    return Err(InstallOneError::Cancelled(
                        "下载已取消，未启动安装器".into(),
                    ));
                }
                Err(fallback_error) => {
                    return Err(InstallOneError::Failed(format!(
                        "下载失败：直连 {official_error}；自动恢复 {fallback_error}"
                    )));
                }
            }
        }
        Err(error) => return Err(InstallOneError::Failed(format!("下载失败：{error}"))),
    };
    let candidate = &active_candidate;

    verify_downloaded_candidate(candidate, &download, trust, on_update)?;

    let payload_download = if let Some(payload) = candidate.bootstrap_payload.as_deref() {
        if payload.bootstrap_payload.is_some()
            || payload.product != candidate.product
            || payload.version != candidate.version
            || payload.architecture != candidate.architecture
        {
            return Err(InstallOneError::Failed(
                "完整离线安装包与安装程序合同不一致".into(),
            ));
        }
        let payload_download = match download_candidate(
            payload,
            trust,
            cancel,
            on_update,
            "正在下载完整离线安装包；完成后官方安装程序不再访问下载服务器",
        ) {
            Ok(download) => download,
            Err(DownloadError::Cancelled) => {
                return Err(InstallOneError::Cancelled(
                    "完整离线安装包下载已取消，未启动安装程序".into(),
                ));
            }
            Err(error) => {
                return Err(InstallOneError::Failed(format!(
                    "完整离线安装包下载失败：{error}"
                )));
            }
        };
        verify_downloaded_candidate(payload, &payload_download, trust, on_update)?;
        Some(payload_download)
    } else {
        None
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallOneError::Cancelled("已在启动安装器前取消".into()));
    }

    let _permit = if let Some(lane) = exclusive_install_lane(candidate.package_kind) {
        Some(execution_gate.acquire(lane, cancel, || {
            emit(
                on_update,
                candidate,
                OperationState::Queued,
                "同类 Windows 系统安装任务正在进行，当前任务稍后自动继续",
            );
        })?)
    } else {
        None
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallOneError::Cancelled("已在启动安装器前取消".into()));
    }

    let (handoff_message, installing_message) = match candidate.package_kind {
        PackageKind::Exe if candidate.bootstrap_payload.is_some() => (
            "即将启动厂商安装程序；完整安装包已经在本机验证，安装阶段无需再次下载",
            "厂商安装程序正在使用本地完整包安装；等待其退出",
        ),
        PackageKind::Msix => (
            "即将部署已验证的完整 MSIX；运行中的目标客户端可能被系统关闭",
            "Windows 正在部署应用包；等待系统返回结果",
        ),
        PackageKind::Msi => (
            "即将静默运行已验证的 MSI；启动后不强制终止",
            "Windows Installer 正在运行；等待其退出",
        ),
        PackageKind::Dmg | PackageKind::TarGz | PackageKind::Zip => (
            "即将把已验证的 macOS 应用写入 Applications；不会执行远端脚本",
            "正在复制并原子替换已验证的应用包",
        ),
        _ => (
            "即将启动厂商安装程序；部分安装器可能静默执行，启动后不强制终止",
            "厂商安装程序运行中；等待其退出",
        ),
    };
    emit(
        on_update,
        candidate,
        OperationState::AwaitingUserInstall,
        handoff_message,
    );
    emit(
        on_update,
        candidate,
        OperationState::Installing,
        installing_message,
    );
    let verified_payload = candidate
        .bootstrap_payload
        .as_deref()
        .zip(payload_download.as_ref())
        .map(|(payload, download)| VerifiedInstallerPayload {
            private_root: download.private_root.path(),
            path: &download.staged_path,
            verified_identity: &download.identity,
            expected_sha256: payload.expected_sha256.as_deref(),
            kind: payload.package_kind,
            expected_architecture: payload.architecture,
            detached_signature: payload.detached_signature.as_deref(),
        });
    let execution = execute_verified_installer(&VerifiedInstallRequest {
        private_root: download.private_root.path(),
        path: &download.staged_path,
        verified_identity: &download.identity,
        expected_sha256: match trust.remote_digest_policy {
            RemoteDigestPolicy::EnforceIfPresent => candidate.expected_sha256.as_deref(),
            RemoteDigestPolicy::PlatformSignatureOnly => None,
        },
        kind: candidate.package_kind,
        trust,
        expected_architecture: candidate.architecture,
        detached_signature: candidate.detached_signature.as_deref(),
        bootstrap_payload: verified_payload.as_ref(),
    })
    .map_err(|error| InstallOneError::Failed(format!("无法启动安装：{error}")))?;
    let installer_succeeded = execution.exit_code == 0
        || (candidate.package_kind == PackageKind::Msi && execution.exit_code == 3010);
    if !installer_succeeded {
        let detail = execution
            .error_summary
            .as_deref()
            .map(|summary| format!("：{summary}"))
            .unwrap_or_default();
        return Err(InstallOneError::Failed(format!(
            "厂商安装器退出码为 {}{detail}",
            execution.exit_code
        )));
    }
    emit(
        on_update,
        candidate,
        OperationState::Installing,
        if execution.exit_code == 3010 {
            "厂商安装器已成功完成并请求稍后重启，先执行安装结果复检"
        } else {
            "厂商安装器退出码为 0，准备复检"
        },
    );

    emit(
        on_update,
        candidate,
        OperationState::Postchecking,
        "等待系统登记产品身份与版本",
    );
    wait_for_direct_postcheck_with(
        candidate,
        trust,
        DIRECT_POSTCHECK_ATTEMPTS,
        DIRECT_POSTCHECK_INTERVAL,
        || detect_product(candidate.product, Some(trust)),
        thread::sleep,
    )
}

fn verify_downloaded_candidate(
    candidate: &ReleaseCandidate,
    download: &DownloadResult,
    trust: &TrustEntry,
    on_update: &impl Fn(OperationUpdate),
) -> Result<(), InstallOneError> {
    emit(
        on_update,
        candidate,
        OperationState::Verifying,
        "正在核对摘要、平台签名、产品身份、版本和架构",
    );
    if let Some(warning) = evaluate_remote_digest(
        trust.remote_digest_policy,
        candidate.expected_sha256.as_deref(),
        &download.identity.sha256,
    )
    .map_err(InstallOneError::Failed)?
    {
        emit(on_update, candidate, OperationState::Verifying, warning);
    }
    let updater_signature_verified = verify_configured_updater_signature_file(
        &download.staged_path,
        trust.updater_public_key.as_deref(),
        trust.sparkle_ed25519_public_key.as_deref(),
        candidate.detached_signature.as_deref(),
    )
    .map_err(|error| InstallOneError::Failed(format!("更新器签名验证失败：{error}")))?;
    let verification = verify_artifact(
        &download.staged_path,
        candidate.package_kind,
        trust,
        candidate.architecture,
        updater_signature_verified,
    )
    .map_err(|error| InstallOneError::Failed(format!("平台验证失败：{error}")))?;
    verify_candidate_artifact_version(candidate, &verification).map_err(InstallOneError::Failed)?;

    let downloads = downloads_directory()
        .map_err(|error| InstallOneError::Failed(format!("无法定位系统“下载”目录：{error}")))?;
    let visible_copy = save_verified_download_copy(download, &downloads).map_err(|error| {
        InstallOneError::Failed(format!("无法保存安装包到系统“下载”目录：{error}"))
    })?;
    let visible_name = visible_copy
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("已验证安装包");
    emit(
        on_update,
        candidate,
        OperationState::Verifying,
        &format!("已验证并保存到系统“下载”目录：{visible_name}"),
    );
    Ok(())
}

fn download_candidate(
    candidate: &ReleaseCandidate,
    trust: &TrustEntry,
    cancel: &Arc<AtomicBool>,
    on_update: &impl Fn(OperationUpdate),
    initial_message: &str,
) -> Result<DownloadResult, DownloadError> {
    let safe_version: String = candidate
        .version
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let file_name = format!(
        "{}-{}.{}",
        candidate.product.key(),
        safe_version,
        candidate.package_kind.extension()
    );
    emit(
        on_update,
        candidate,
        OperationState::Downloading,
        initial_message,
    );
    let progress = |received: u64, total: Option<u64>| {
        let message = match total {
            Some(total) if total > 0 => format!(
                "已下载 {:.1}% ({:.1}/{:.1} MiB)",
                received as f64 * 100.0 / total as f64,
                received as f64 / 1_048_576.0,
                total as f64 / 1_048_576.0
            ),
            _ => format!("已下载 {:.1} MiB", received as f64 / 1_048_576.0),
        };
        emit(on_update, candidate, OperationState::Downloading, &message);
    };
    let is_cancelled = || cancel.load(Ordering::Relaxed);
    let download_url_rules = match candidate.source {
        ArtifactSource::Official => &trust.url_rules,
        ArtifactSource::VerifiedMirror { .. } => &trust.mirror_url_rules,
    };
    download_to_private_staging_controlled(
        &DownloadRequest {
            url: candidate.download_url.clone(),
            file_name,
            url_rules: download_url_rules,
            expected_size: candidate.expected_size,
        },
        &DownloadControl {
            is_cancelled: &is_cancelled,
            on_progress: &progress,
        },
    )
}

fn validate_verified_download_fallback(
    primary: &ReleaseCandidate,
    fallback: &ReleaseCandidate,
) -> Result<(), String> {
    if !fallback.source.is_verified_mirror()
        || fallback.product != primary.product
        || fallback.version != primary.version
        || fallback.architecture != primary.architecture
        || fallback.package_kind != primary.package_kind
        || fallback.expected_size.is_none()
        || primary
            .expected_size
            .is_some_and(|expected| fallback.expected_size != Some(expected))
        || primary
            .expected_sha256
            .as_deref()
            .is_some_and(|expected| fallback.expected_sha256.as_deref() != Some(expected))
        || primary
            .detached_signature
            .as_deref()
            .is_some_and(|expected| fallback.detached_signature.as_deref() != Some(expected))
        || fallback.expected_sha256.is_none()
        || !bootstrap_payloads_match(primary, fallback)
    {
        return Err("自动恢复包与已确认的目标版本不完全一致，已停止安装".into());
    }
    Ok(())
}

fn bootstrap_payloads_match(primary: &ReleaseCandidate, fallback: &ReleaseCandidate) -> bool {
    match (
        primary.bootstrap_payload.as_deref(),
        fallback.bootstrap_payload.as_deref(),
    ) {
        (None, None) => true,
        (Some(primary), Some(fallback)) => {
            primary.bootstrap_payload.is_none()
                && fallback.bootstrap_payload.is_none()
                && primary.source.is_verified_mirror()
                && fallback.source.is_verified_mirror()
                && primary.product == fallback.product
                && primary.version == fallback.version
                && primary.architecture == fallback.architecture
                && primary.package_kind == fallback.package_kind
                && primary.download_url == fallback.download_url
                && primary.expected_size == fallback.expected_size
                && primary.expected_sha256 == fallback.expected_sha256
                && primary.detached_signature == fallback.detached_signature
        }
        _ => false,
    }
}

fn evaluate_remote_digest(
    policy: RemoteDigestPolicy,
    expected: Option<&str>,
    actual: &str,
) -> Result<Option<&'static str>, String> {
    let Some(expected) = expected else {
        return Ok(None);
    };
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(None);
    }
    match policy {
        RemoteDigestPolicy::EnforceIfPresent => Err("预期 SHA-256 摘要不匹配".into()),
        RemoteDigestPolicy::PlatformSignatureOnly => Ok(Some(
            "厂商提供的 SHA-256 与下载文件不一致；按 WorkBuddy macOS 专用策略继续验证 Apple 签名和应用身份",
        )),
    }
}

enum DirectPostcheckDecision {
    Success(String),
    Retry(String),
    Failed(String),
}

fn evaluate_direct_postcheck(
    detection: &Detection,
    candidate: &ReleaseCandidate,
    trust: &super::TrustEntry,
) -> DirectPostcheckDecision {
    if !detection.installed {
        return DirectPostcheckDecision::Retry(format!("尚未发现产品：{}", detection.evidence));
    }
    if let Some(expected_family) = &trust.package_family
        && detection.package_family.as_deref() != Some(expected_family.as_str())
    {
        return DirectPostcheckDecision::Failed(format!(
            "安装后 package family 不匹配：{:?}",
            detection.package_family
        ));
    }
    if trust.package_family.is_some()
        && let Some(expected_identity) = &trust.package_identity
        && detection.package_identity.as_deref() != Some(expected_identity.as_str())
    {
        return DirectPostcheckDecision::Failed(format!(
            "安装后 package identity 不匹配：{:?}",
            detection.package_identity
        ));
    }
    if trust.package_family.is_some()
        && let Some(expected_publisher) = &trust.msix_publisher
        && detection.publisher.as_deref() != Some(expected_publisher.as_str())
    {
        return DirectPostcheckDecision::Failed(format!(
            "安装后 publisher 不匹配：{:?}",
            detection.publisher
        ));
    }
    if trust.package_family.is_some() && detection.architecture != Some(candidate.architecture) {
        return DirectPostcheckDecision::Failed(format!(
            "安装后 AppX 架构不匹配：期望 {:?}，实际 {:?}",
            candidate.architecture, detection.architecture
        ));
    }
    if let Some(executable) = trust.postinstall_executable.as_deref() {
        match detection.architecture {
            Some(architecture) if architecture == candidate.architecture => {}
            Some(architecture) => {
                return DirectPostcheckDecision::Failed(format!(
                    "安装后 {executable} 架构不匹配：期望 {:?}，实际 {:?}",
                    candidate.architecture, architecture
                ));
            }
            None => {
                return DirectPostcheckDecision::Retry(format!(
                    "已发现产品，但暂未确认 {executable} 的架构"
                ));
            }
        }
    }
    if let Some(expected_bundle_id) = trust.macos_bundle_id.as_deref()
        && detection.package_identity.as_deref() != Some(expected_bundle_id)
    {
        return DirectPostcheckDecision::Failed(format!(
            "安装后 Bundle ID 不匹配：{:?}",
            detection.package_identity
        ));
    }
    if let Some(expected_team_id) = trust.macos_team_id.as_deref()
        && detection.publisher.as_deref() != Some(expected_team_id)
    {
        return DirectPostcheckDecision::Failed(format!(
            "安装后 Team ID 不匹配：{:?}",
            detection.publisher
        ));
    }
    if trust.macos_bundle_id.is_some() && detection.architecture != Some(candidate.architecture) {
        return DirectPostcheckDecision::Failed(format!(
            "安装后应用架构不匹配：期望 {:?}，实际 {:?}",
            candidate.architecture, detection.architecture
        ));
    }
    let Some(installed_version) = detection.version.as_deref() else {
        return DirectPostcheckDecision::Retry(format!(
            "已发现产品但版本未知：{}",
            detection.evidence
        ));
    };
    if version_is_older_for_product(candidate.product, installed_version, &candidate.version) {
        return DirectPostcheckDecision::Retry(format!(
            "当前仍为 {installed_version}，目标为 {}",
            candidate.version
        ));
    }
    DirectPostcheckDecision::Success(format!("复检成功：{installed_version}"))
}

fn verify_candidate_artifact_version(
    candidate: &ReleaseCandidate,
    verification: &ArtifactVerification,
) -> Result<(), String> {
    let requires_exact_artifact_version = matches!(
        candidate.package_kind,
        PackageKind::Msix | PackageKind::Dmg | PackageKind::TarGz | PackageKind::Zip
    );
    if !requires_exact_artifact_version {
        return Ok(());
    }
    match verification.version.as_deref() {
        Some(version)
            if compare_versions_with_precision(
                version,
                &candidate.version,
                version_precision(candidate.product),
            ) == std::cmp::Ordering::Equal =>
        {
            Ok(())
        }
        Some(version) => Err(format!(
            "包内版本与官方清单不一致：清单 {}，包内 {version}",
            candidate.version
        )),
        None => Err("安装包未提供可验证的包内版本".into()),
    }
}

fn wait_for_direct_postcheck_with(
    candidate: &ReleaseCandidate,
    trust: &super::TrustEntry,
    attempts: usize,
    interval: Duration,
    mut detect: impl FnMut() -> Detection,
    mut wait: impl FnMut(Duration),
) -> Result<String, InstallOneError> {
    let mut last_observation = "尚未执行复检".to_owned();
    for attempt in 0..attempts.max(1) {
        let detection = detect();
        match evaluate_direct_postcheck(&detection, candidate, trust) {
            DirectPostcheckDecision::Success(message) => return Ok(message),
            DirectPostcheckDecision::Failed(message) => {
                return Err(InstallOneError::Failed(message));
            }
            DirectPostcheckDecision::Retry(message) => last_observation = message,
        }
        if attempt + 1 < attempts.max(1) {
            wait(interval);
        }
    }
    Err(InstallOneError::ResultUnknown(format!(
        "厂商安装器已正常退出，但暂未确认目标版本；最后检测：{last_observation}"
    )))
}

fn emit(
    on_update: &impl Fn(OperationUpdate),
    candidate: &ReleaseCandidate,
    state: OperationState,
    message: &str,
) {
    on_update(OperationUpdate {
        product: candidate.product,
        state,
        message: message.into(),
    });
}

pub fn version_is_older(installed: &str, target: &str) -> bool {
    compare_versions(installed, target).is_lt()
}

pub fn version_is_older_for_product(
    product: super::ProductId,
    installed: &str,
    target: &str,
) -> bool {
    compare_versions_with_precision(installed, target, version_precision(product)).is_lt()
}

pub fn assess_existing_install(detection: &Detection, target: &str) -> PreinstallDecision {
    assess_existing_install_with_precision(detection, target, None, false)
}

pub fn assess_existing_install_for_product(
    detection: &Detection,
    product: super::ProductId,
    target: &str,
    allow_unknown_management: bool,
) -> PreinstallDecision {
    assess_existing_install_with_precision(
        detection,
        target,
        version_precision(product),
        allow_unknown_management,
    )
}

fn assess_existing_install_with_precision(
    detection: &Detection,
    target: &str,
    precision: Option<usize>,
    allow_unknown_management: bool,
) -> PreinstallDecision {
    if detection.is_failed() {
        return PreinstallDecision::Reject("无法确认当前安装状态，请先刷新状态后再试".into());
    }
    if !detection.installed {
        return PreinstallDecision::Proceed;
    }
    if detection.managed {
        return PreinstallDecision::Reject("检测到受组织管理的安装，拒绝自动覆盖".into());
    }
    if !detection.management_known && !allow_unknown_management {
        return PreinstallDecision::Reject(
            "无法确认现有安装是否受组织管理，按失败关闭策略拒绝覆盖".into(),
        );
    }
    let Some(installed) = detection.version.as_deref() else {
        return PreinstallDecision::Reject("现有安装版本未知，拒绝自动覆盖".into());
    };
    match compare_versions_with_precision(installed, target, precision) {
        std::cmp::Ordering::Greater => PreinstallDecision::Reject(format!(
            "已安装版本 {installed} 高于目标 {target}，拒绝降级"
        )),
        std::cmp::Ordering::Equal => {
            PreinstallDecision::AlreadyCurrent(format!("已安装目标版本 {installed}，无需重复安装"))
        }
        std::cmp::Ordering::Less => PreinstallDecision::Proceed,
    }
}

fn compare_versions(installed: &str, target: &str) -> std::cmp::Ordering {
    compare_versions_with_precision(installed, target, None)
}

fn compare_versions_with_precision(
    installed: &str,
    target: &str,
    precision: Option<usize>,
) -> std::cmp::Ordering {
    let parse = |value: &str| -> Vec<u64> {
        value
            .split(|character: char| !character.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let mut installed = parse(installed);
    let mut target = parse(target);
    let length = precision.unwrap_or_else(|| installed.len().max(target.len()));
    installed.truncate(length);
    target.truncate(length);
    installed.resize(length, 0);
    target.resize(length, 0);
    installed.cmp(&target)
}

const fn version_precision(product: super::ProductId) -> Option<usize> {
    match product {
        super::ProductId::WorkBuddy => Some(3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;

    use url::Url;

    use super::{
        InstallExecutionGate, InstallExecutionLane, InstallOneError, PreinstallDecision,
        assess_existing_install, evaluate_remote_digest, exclusive_install_lane,
        validate_verified_download_fallback, verify_candidate_artifact_version,
        wait_for_direct_postcheck_with,
    };
    use crate::core::{
        Architecture, ArtifactSource, Detection, OperatingSystem, PackageKind, ProductId,
        ReleaseCandidate, RemoteDigestPolicy, TrustRegistry,
    };
    use crate::platform::ArtifactVerification;

    fn workbuddy_candidate() -> ReleaseCandidate {
        ReleaseCandidate {
            product: ProductId::WorkBuddy,
            version: "5.3.8.34705286".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Exe,
            download_url: Url::parse("https://download.codebuddy.cn/workbuddy/WorkBuddySetup.exe")
                .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: None,
            expected_size: None,
            expected_sha256: None,
            detached_signature: None,
            bootstrap_payload: None,
        }
    }

    #[test]
    fn install_gate_serializes_only_the_same_system_engine_and_allows_cancellation() {
        let gate = Arc::new(InstallExecutionGate::default());
        let first_cancel = AtomicBool::new(false);
        let first_permit = gate
            .acquire(InstallExecutionLane::WindowsMsi, &first_cancel, || {})
            .unwrap();

        let appx_cancel = AtomicBool::new(false);
        let appx_permit = gate
            .acquire(InstallExecutionLane::WindowsAppx, &appx_cancel, || {})
            .unwrap();
        drop(appx_permit);

        let second_gate = gate.clone();
        let second_cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = second_cancel.clone();
        let wait_announced = Arc::new(AtomicBool::new(false));
        let wait_for_thread = wait_announced.clone();
        let second = thread::spawn(move || {
            second_gate
                .acquire(InstallExecutionLane::WindowsMsi, &cancel_for_thread, || {
                    wait_for_thread.store(true, Ordering::Relaxed)
                })
                .is_err()
        });

        thread::sleep(Duration::from_millis(80));
        assert!(wait_announced.load(Ordering::Relaxed));
        second_cancel.store(true, Ordering::Relaxed);
        assert!(second.join().unwrap());

        let third_gate = gate.clone();
        let third = thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let _permit = third_gate
                .acquire(InstallExecutionLane::WindowsMsi, &cancel, || {})
                .unwrap();
            true
        });
        thread::sleep(Duration::from_millis(80));
        assert!(!third.is_finished());
        drop(first_permit);
        assert!(third.join().unwrap());

        assert_eq!(exclusive_install_lane(PackageKind::Exe), None);
        assert_eq!(exclusive_install_lane(PackageKind::Dmg), None);
        assert_eq!(
            exclusive_install_lane(PackageKind::Msi),
            Some(InstallExecutionLane::WindowsMsi)
        );
        assert_eq!(
            exclusive_install_lane(PackageKind::Msix),
            Some(InstallExecutionLane::WindowsAppx)
        );
    }

    fn workbuddy_detection(version: Option<&str>) -> Detection {
        Detection {
            installed: true,
            version: version.map(ToOwned::to_owned),
            managed: false,
            management_known: true,
            package_identity: None,
            package_family: None,
            publisher: Some("Tencent Technology (Shenzhen) Company Limited".into()),
            architecture: Some(Architecture::X64),
            evidence: "Uninstall:WorkBuddy [HKCU]".into(),
        }
    }

    fn chatgpt_candidate() -> ReleaseCandidate {
        ReleaseCandidate {
            product: ProductId::ChatGpt,
            version: "26.727.6591.0".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Msix,
            download_url: Url::parse(
                "https://persistent.oaistatic.com/codex-app-prod/releases/26.727.6591.0/ChatGPT-x64.msix",
            )
            .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: None,
            expected_size: None,
            expected_sha256: None,
            detached_signature: None,
            bootstrap_payload: None,
        }
    }

    fn chatgpt_detection() -> Detection {
        Detection {
            installed: true,
            version: Some("26.727.6591.0".into()),
            managed: false,
            management_known: true,
            package_identity: Some("OpenAI.Codex".into()),
            package_family: Some("OpenAI.Codex_2p2nqsd0c76g0".into()),
            publisher: Some("CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B".into()),
            architecture: Some(Architecture::X64),
            evidence: "AppX:OpenAI.Codex".into(),
        }
    }

    fn cc_switch_macos_candidate() -> ReleaseCandidate {
        ReleaseCandidate {
            product: ProductId::CcSwitch,
            version: "3.19.1".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::TarGz,
            download_url: Url::parse("https://dl.ccswitch.io/v3.19.1/CC-Switch-macOS.tar.gz")
                .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: None,
            expected_size: None,
            expected_sha256: None,
            detached_signature: Some("untrusted comment: fixture".into()),
            bootstrap_payload: None,
        }
    }

    fn cc_switch_macos_detection() -> Detection {
        Detection {
            installed: true,
            version: Some("3.19.1".into()),
            managed: false,
            management_known: true,
            package_identity: Some("com.ccswitch.desktop".into()),
            package_family: None,
            publisher: Some("R8UR22V2F9".into()),
            architecture: Some(Architecture::X64),
            evidence: "用户 Applications · 已通过 Bundle/签名/Gatekeeper 检查".into(),
        }
    }

    #[test]
    fn direct_postcheck_waits_for_the_target_version() {
        let candidate = workbuddy_candidate();
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        let mut detections = VecDeque::from([
            Detection::absent("registry not updated"),
            workbuddy_detection(Some("5.1.7")),
            workbuddy_detection(Some("5.3.8")),
        ]);
        let mut waits = 0;
        let result = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            5,
            Duration::ZERO,
            || detections.pop_front().unwrap(),
            |_| waits += 1,
        )
        .unwrap();
        assert_eq!(result, "复检成功：5.3.8");
        assert_eq!(waits, 2);
    }

    #[test]
    fn failed_detection_never_becomes_a_new_install_decision() {
        let decision =
            assess_existing_install(&Detection::failed("Windows AppX 查询失败"), "1.0.0");
        assert!(
            matches!(decision, PreinstallDecision::Reject(message) if message.contains("刷新状态"))
        );
    }

    #[test]
    fn workbuddy_macos_digest_policy_keeps_platform_verification_but_other_entries_fail() {
        assert!(
            evaluate_remote_digest(
                RemoteDigestPolicy::EnforceIfPresent,
                Some("expected"),
                "actual"
            )
            .is_err()
        );
        let warning = evaluate_remote_digest(
            RemoteDigestPolicy::PlatformSignatureOnly,
            Some("expected"),
            "actual",
        )
        .unwrap()
        .unwrap();
        assert!(warning.contains("WorkBuddy macOS"));
        assert_eq!(
            evaluate_remote_digest(
                RemoteDigestPolicy::PlatformSignatureOnly,
                Some("same"),
                "SAME"
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn download_fallback_cannot_change_the_confirmed_chatgpt_release() {
        let primary = ReleaseCandidate {
            product: ProductId::ChatGpt,
            version: "26.803.41515".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Zip,
            download_url: Url::parse(
                "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-x64-26.803.41515.zip",
            )
            .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: Some("12.0".into()),
            expected_size: Some(539_372_355),
            expected_sha256: None,
            detached_signature: Some("vendor-signature".into()),
            bootstrap_payload: None,
        };
        let mut fallback = primary.clone();
        fallback.download_url = Url::parse(
            "https://mirror.example/artifacts/chatgpt/macos/x64/26.803.41515/hash/ChatGPT-darwin-x64-26.803.41515.zip",
        )
        .unwrap();
        fallback.source = ArtifactSource::VerifiedMirror {
            synced_at_unix: 1_800_000_000,
        };
        fallback.expected_sha256 = Some("a".repeat(64));
        validate_verified_download_fallback(&primary, &fallback).unwrap();

        let mut changed_size = fallback.clone();
        changed_size.expected_size = Some(539_372_356);
        assert!(validate_verified_download_fallback(&primary, &changed_size).is_err());
        let mut no_digest = fallback;
        no_digest.expected_sha256 = None;
        assert!(validate_verified_download_fallback(&primary, &no_digest).is_err());
    }

    #[test]
    fn direct_postcheck_timeout_is_result_unknown_instead_of_false_failure() {
        let candidate = workbuddy_candidate();
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        let error = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            3,
            Duration::ZERO,
            || workbuddy_detection(Some("5.1.7")),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallOneError::ResultUnknown(message)
                if message.contains("当前仍为 5.1.7")
        ));
    }

    #[test]
    fn direct_postcheck_rejects_a_wrong_workbuddy_application_architecture() {
        let candidate = workbuddy_candidate();
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        let mut wrong_architecture = workbuddy_detection(Some("5.3.8.34705286"));
        wrong_architecture.architecture = Some(Architecture::Unsupported);
        let error = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            1,
            Duration::ZERO,
            || wrong_architecture.clone(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallOneError::Failed(message)
                if message.contains("WorkBuddy.exe 架构不匹配")
        ));
    }

    #[test]
    fn direct_postcheck_retries_when_workbuddy_application_architecture_is_unknown() {
        let candidate = workbuddy_candidate();
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        let mut unknown_architecture = workbuddy_detection(Some("5.3.8.34705286"));
        unknown_architecture.architecture = None;
        let error = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            1,
            Duration::ZERO,
            || unknown_architecture.clone(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallOneError::ResultUnknown(message)
                if message.contains("暂未确认 WorkBuddy.exe 的架构")
        ));
    }

    #[test]
    fn chatgpt_requires_the_downloaded_msix_version_to_match_the_manifest() {
        let candidate = chatgpt_candidate();
        let matching = ArtifactVerification {
            signer_subject: Some("OpenAI".into()),
            product_identity: "OpenAI.Codex".into(),
            version: Some(candidate.version.clone()),
            architecture: Some(Architecture::X64),
        };
        verify_candidate_artifact_version(&candidate, &matching).unwrap();

        let changed = ArtifactVerification {
            version: Some("26.727.6590.0".into()),
            ..matching
        };
        assert!(verify_candidate_artifact_version(&candidate, &changed).is_err());
    }

    #[test]
    fn exact_version_packages_normalize_an_optional_fourth_component() {
        let candidate = ReleaseCandidate {
            product: ProductId::Claude,
            version: "1.26832.0".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Dmg,
            download_url: Url::parse(
                "https://downloads.claude.ai/releases/darwin/universal/1.26832.0/Claude.dmg",
            )
            .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: None,
            expected_size: None,
            expected_sha256: None,
            detached_signature: None,
            bootstrap_payload: None,
        };
        let matching = ArtifactVerification {
            signer_subject: Some("Anthropic".into()),
            product_identity: "Claude".into(),
            version: Some("1.26832.0.0".into()),
            architecture: Some(Architecture::X64),
        };
        verify_candidate_artifact_version(&candidate, &matching).unwrap();

        let older = ArtifactVerification {
            version: Some("1.26831.0.0".into()),
            ..matching.clone()
        };
        assert!(verify_candidate_artifact_version(&candidate, &older).is_err());

        let missing = ArtifactVerification {
            version: None,
            ..matching
        };
        assert!(verify_candidate_artifact_version(&candidate, &missing).is_err());
    }

    #[test]
    fn chatgpt_postcheck_requires_exact_appx_identity_and_publisher() {
        let candidate = chatgpt_candidate();
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::ChatGpt,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();

        let mut wrong_identity = chatgpt_detection();
        wrong_identity.package_identity = Some("OpenAI.Other".into());
        let error = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            1,
            Duration::ZERO,
            || wrong_identity.clone(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallOneError::Failed(message) if message.contains("package identity")
        ));

        let mut wrong_publisher = chatgpt_detection();
        wrong_publisher.publisher = Some("CN=Unexpected".into());
        let error = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            1,
            Duration::ZERO,
            || wrong_publisher.clone(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallOneError::Failed(message) if message.contains("publisher")
        ));
    }

    #[test]
    fn macos_postcheck_requires_exact_bundle_team_architecture_and_version() {
        let candidate = cc_switch_macos_candidate();
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::CcSwitch,
                OperatingSystem::MacOs,
                Architecture::X64,
            )
            .unwrap();

        let artifact = ArtifactVerification {
            signer_subject: Some("R8UR22V2F9".into()),
            product_identity: "com.ccswitch.desktop".into(),
            version: Some(candidate.version.clone()),
            architecture: Some(candidate.architecture),
        };
        verify_candidate_artifact_version(&candidate, &artifact).unwrap();

        let result = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            1,
            Duration::ZERO,
            cc_switch_macos_detection,
            |_| {},
        )
        .unwrap();
        assert_eq!(result, "复检成功：3.19.1");

        let mut wrong_bundle = cc_switch_macos_detection();
        wrong_bundle.package_identity = Some("com.example.other".into());
        let error = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            1,
            Duration::ZERO,
            || wrong_bundle.clone(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallOneError::Failed(message) if message.contains("Bundle ID")
        ));

        let mut wrong_team = cc_switch_macos_detection();
        wrong_team.publisher = Some("UNEXPECTED".into());
        let error = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            1,
            Duration::ZERO,
            || wrong_team.clone(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallOneError::Failed(message) if message.contains("Team ID")
        ));

        let mut wrong_architecture = cc_switch_macos_detection();
        wrong_architecture.architecture = Some(Architecture::Arm64);
        let error = wait_for_direct_postcheck_with(
            &candidate,
            trust,
            1,
            Duration::ZERO,
            || wrong_architecture.clone(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallOneError::Failed(message) if message.contains("应用架构")
        ));

        let changed_artifact = ArtifactVerification {
            version: Some("3.19.0".into()),
            ..artifact
        };
        assert!(verify_candidate_artifact_version(&candidate, &changed_artifact).is_err());
    }
}
