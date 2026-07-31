use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::platform::{detect_product, execute_verified_installer, verify_artifact};

use super::{
    Detection, DownloadControl, DownloadRequest, OperationState, OperationUpdate, PlatformInfo,
    ProductOperationResult, ReleaseCandidate, TrustRegistry,
    download_to_private_staging_controlled, verify_minisign_file,
};

pub fn run_install_batch(
    candidates: Vec<ReleaseCandidate>,
    platform: PlatformInfo,
    registry: TrustRegistry,
    cancel: Arc<AtomicBool>,
    on_update: impl Fn(OperationUpdate),
) -> Vec<ProductOperationResult> {
    let mut results = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if cancel.load(Ordering::Relaxed) {
            let result = ProductOperationResult {
                product: candidate.product,
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
        let result = match install_one(&candidate, &platform, &registry, &cancel, &on_update) {
            Ok(message) => ProductOperationResult {
                product: candidate.product,
                state: OperationState::Succeeded,
                message,
            },
            Err(InstallOneError::Cancelled(message)) => ProductOperationResult {
                product: candidate.product,
                state: OperationState::Cancelled,
                message,
            },
            Err(InstallOneError::Failed(message)) => ProductOperationResult {
                product: candidate.product,
                state: OperationState::Failed,
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

enum InstallOneError {
    Cancelled(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreinstallDecision {
    Proceed,
    AlreadyCurrent(String),
    Reject(String),
}

fn install_one(
    candidate: &ReleaseCandidate,
    platform: &PlatformInfo,
    registry: &TrustRegistry,
    cancel: &Arc<AtomicBool>,
    on_update: &impl Fn(OperationUpdate),
) -> Result<String, InstallOneError> {
    let trust = registry
        .find(candidate.product, platform.os, platform.architecture)
        .ok_or_else(|| InstallOneError::Failed("缺少当前平台的信任条目".into()))?;
    if !trust.enabled {
        return Err(InstallOneError::Failed(format!(
            "信任条目尚未启用：{}",
            trust.status_reason
        )));
    }
    if candidate.architecture != platform.architecture {
        return Err(InstallOneError::Failed("候选包架构与当前设备不一致".into()));
    }
    let current = detect_product(candidate.product);
    match assess_existing_install(&current, &candidate.version) {
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
    match (
        trust.updater_public_key.as_deref(),
        candidate.detached_signature.as_deref(),
    ) {
        (Some(public_key), Some(signature)) => {
            verify_minisign_file(&download.staged_path, public_key, signature)
                .map_err(|error| InstallOneError::Failed(format!("更新器签名验证失败：{error}")))?;
        }
        (None, None) => {}
        _ => {
            return Err(InstallOneError::Failed(
                "信任注册表公钥与候选包签名不完整".into(),
            ));
        }
    }
    verify_artifact(
        &download.staged_path,
        candidate.package_kind,
        trust,
        candidate.architecture,
    )
    .map_err(|error| InstallOneError::Failed(format!("平台验证失败：{error}")))?;
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallOneError::Cancelled("已在启动安装器前取消".into()));
    }

    emit(
        on_update,
        candidate,
        OperationState::AwaitingUserInstall,
        "即将启动厂商安装界面；安装器运行后不强制终止",
    );
    emit(
        on_update,
        candidate,
        OperationState::Installing,
        "等待厂商安装器退出",
    );
    let exit_code = execute_verified_installer(
        download.private_root.path(),
        &download.staged_path,
        &download.identity,
        candidate.expected_sha256.as_deref(),
        candidate.package_kind,
        trust,
        candidate.architecture,
    )
    .map_err(|error| InstallOneError::Failed(format!("无法启动安装：{error}")))?;
    if exit_code != 0 {
        return Err(InstallOneError::Failed(format!(
            "厂商安装器退出码为 {exit_code}"
        )));
    }

    emit(
        on_update,
        candidate,
        OperationState::Postchecking,
        "重新检测产品身份与版本",
    );
    let detection = detect_product(candidate.product);
    if !detection.installed {
        return Err(InstallOneError::Failed(format!(
            "安装器返回成功，但复检未发现产品：{}",
            detection.evidence
        )));
    }
    if let Some(expected_family) = &trust.package_family
        && !detection.evidence.contains(expected_family)
    {
        return Err(InstallOneError::Failed(format!(
            "安装后 package family 不匹配：{}",
            detection.evidence
        )));
    }
    if let Some(installed_version) = detection.version.as_deref()
        && version_is_older(installed_version, &candidate.version)
    {
        return Err(InstallOneError::Failed(format!(
            "安装后版本 {installed_version} 低于目标 {}",
            candidate.version
        )));
    }
    Ok(format!(
        "复检成功：{}",
        detection.version.as_deref().unwrap_or("版本未知")
    ))
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

pub fn assess_existing_install(detection: &Detection, target: &str) -> PreinstallDecision {
    if !detection.installed {
        return PreinstallDecision::Proceed;
    }
    if detection.managed {
        return PreinstallDecision::Reject("检测到受组织管理的安装，拒绝自动覆盖".into());
    }
    if !detection.management_known {
        return PreinstallDecision::Reject(
            "无法确认现有安装是否受组织管理，按失败关闭策略拒绝覆盖".into(),
        );
    }
    let Some(installed) = detection.version.as_deref() else {
        return PreinstallDecision::Reject("现有安装版本未知，拒绝自动覆盖".into());
    };
    match compare_versions(installed, target) {
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
    let parse = |value: &str| -> Vec<u64> {
        value
            .split(|character: char| !character.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let mut installed = parse(installed);
    let mut target = parse(target);
    let length = installed.len().max(target.len());
    installed.resize(length, 0);
    target.resize(length, 0);
    installed.cmp(&target)
}
