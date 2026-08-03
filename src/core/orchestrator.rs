use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::platform::{
    ArtifactVerification, StoreInstallError, StoreInstallRequest, VerifiedInstallRequest,
    detect_product, execute_microsoft_store_install, execute_verified_installer, verify_artifact,
};

use super::{
    Detection, DownloadControl, DownloadRequest, InstallPlan, OperationState, OperationUpdate,
    PackageKind, PlatformInfo, ProductOperationResult, ReleaseCandidate, TrustRegistry,
    download_to_private_staging_controlled, verify_minisign_file,
};

const DIRECT_POSTCHECK_ATTEMPTS: usize = 46;
const DIRECT_POSTCHECK_INTERVAL: Duration = Duration::from_secs(2);

pub fn run_install_batch(
    plans: Vec<InstallPlan>,
    platform: PlatformInfo,
    registry: TrustRegistry,
    cancel: Arc<AtomicBool>,
    on_update: impl Fn(OperationUpdate),
) -> Vec<ProductOperationResult> {
    let mut results = Vec::with_capacity(plans.len());
    for plan in plans {
        let product = plan.product();
        if cancel.load(Ordering::Relaxed) {
            let result = ProductOperationResult {
                product,
                state: OperationState::Cancelled,
                message: "批次已取消，未启动后续产品".into(),
            };
            on_update(OperationUpdate {
                product: result.product,
                state: result.state,
                message: result.message.clone(),
            });
            results.push(result);
            continue;
        }
        on_update(OperationUpdate {
            product,
            state: OperationState::Ready,
            message: "用户已确认，开始执行官方安装计划".into(),
        });
        let result = match install_one(&plan, &platform, &registry, &cancel, &on_update) {
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
            product: result.product,
            state: result.state,
            message: result.message.clone(),
        });
        results.push(result);
    }
    results
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
        InstallPlan::DirectPackage(candidate) => {
            install_direct_package(candidate, platform, registry, cancel, on_update, &current)
        }
        InstallPlan::MicrosoftStore(store_plan) => {
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
    platform: &PlatformInfo,
    registry: &TrustRegistry,
    cancel: &Arc<AtomicBool>,
    on_update: &impl Fn(OperationUpdate),
    current: &Detection,
) -> Result<String, InstallOneError> {
    let trust = registry
        .find(candidate.product, platform.os, platform.architecture)
        .ok_or_else(|| InstallOneError::Failed("缺少当前平台的信任条目".into()))?;
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
        "从已固定的官方来源下载",
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
    let download = download_to_private_staging_controlled(
        &DownloadRequest {
            url: candidate.download_url.clone(),
            file_name,
            trust,
        },
        &DownloadControl {
            is_cancelled: &is_cancelled,
            on_progress: &progress,
        },
    )
    .map_err(|error| {
        if matches!(error, super::DownloadError::Cancelled) {
            InstallOneError::Cancelled("下载已取消，未启动安装器".into())
        } else {
            InstallOneError::Failed(format!("下载失败：{error}"))
        }
    })?;

    emit(
        on_update,
        candidate,
        OperationState::Verifying,
        "正在核对摘要、平台签名、产品身份和架构",
    );
    if let Some(expected) = candidate.expected_sha256.as_deref()
        && !download.identity.sha256.eq_ignore_ascii_case(expected)
    {
        return Err(InstallOneError::Failed("官方摘要不匹配".into()));
    }
    let updater_signature_verified = match (
        trust.updater_public_key.as_deref(),
        candidate.detached_signature.as_deref(),
    ) {
        (Some(public_key), Some(signature)) => {
            verify_minisign_file(&download.staged_path, public_key, signature)
                .map_err(|error| InstallOneError::Failed(format!("更新器签名验证失败：{error}")))?;
            true
        }
        (None, None) => false,
        _ => {
            return Err(InstallOneError::Failed(
                "信任注册表公钥与候选包签名不完整".into(),
            ));
        }
    };
    let verification = verify_artifact(
        &download.staged_path,
        candidate.package_kind,
        trust,
        candidate.architecture,
        updater_signature_verified,
    )
    .map_err(|error| InstallOneError::Failed(format!("平台验证失败：{error}")))?;
    verify_candidate_artifact_version(candidate, &verification).map_err(InstallOneError::Failed)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallOneError::Cancelled("已在启动安装器前取消".into()));
    }

    let (handoff_message, installing_message) = match candidate.package_kind {
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
    let execution = execute_verified_installer(&VerifiedInstallRequest {
        private_root: download.private_root.path(),
        path: &download.staged_path,
        verified_identity: &download.identity,
        expected_sha256: candidate.expected_sha256.as_deref(),
        kind: candidate.package_kind,
        trust,
        expected_architecture: candidate.architecture,
        detached_signature: candidate.detached_signature.as_deref(),
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
    let requires_exact_bundle_version = matches!(
        candidate.package_kind,
        PackageKind::Dmg | PackageKind::TarGz | PackageKind::Zip
    );
    if candidate.product != super::ProductId::ChatGpt && !requires_exact_bundle_version {
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
    use std::time::Duration;

    use url::Url;

    use super::{
        InstallOneError, verify_candidate_artifact_version, wait_for_direct_postcheck_with,
    };
    use crate::core::{
        Architecture, Detection, OperatingSystem, PackageKind, ProductId, ReleaseCandidate,
        TrustRegistry,
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
            expected_sha256: None,
            detached_signature: None,
        }
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
            expected_sha256: None,
            detached_signature: None,
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
            expected_sha256: None,
            detached_signature: Some("untrusted comment: fixture".into()),
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
