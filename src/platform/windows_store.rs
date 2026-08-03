use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use regex::Regex;
use serde::Deserialize;
use tempfile::TempDir;
use url::Url;
use zip::ZipArchive;

use crate::core::{
    Architecture, Detection, DownloadControl, DownloadRequest, OperationState, OperationUpdate,
    ProductId, StableFileIdentity, download_to_private_staging_controlled, fetch_official_text,
    inspect_staged_file, safe_http_client, validate_staged_file_name, verify_staged_identity,
    version_is_older,
};

use super::{PlannedCommand, StoreInstallError, StoreInstallRequest};

const WINGET_PINNED_CERTIFICATE_MISMATCH: u32 = 0x8A15_005E;
const WINGET_UPDATE_NOT_APPLICABLE: u32 = 0x8A15_002B;
const WINGET_PACKAGE_ALREADY_INSTALLED: u32 = 0x8A15_0061;
const WINGET_INSTALL_ALREADY_INSTALLED: u32 = 0x8A15_010D;
const MAX_CAPTURE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DEPENDENCY_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_DEPENDENCY_COUNT: usize = 32;
const MAX_INNER_APPX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

const AUTHENTICODE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$signature = Get-AuthenticodeSignature -LiteralPath $env:AI_CLIENT_INSTALLER_ARTIFACT
[pscustomobject]@{
  status = [string]$signature.Status
  signer_subject = if ($signature.SignerCertificate) { [string]$signature.SignerCertificate.Subject } else { $null }
} | ConvertTo-Json -Compress
"#;

const ADD_APPX_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Import-Module Appx -ErrorAction Stop
$dependencies = @()
if ($env:AI_CLIENT_INSTALLER_DEPENDENCIES_JSON) {
  $dependencies = @($env:AI_CLIENT_INSTALLER_DEPENDENCIES_JSON | ConvertFrom-Json)
}
$parameters = @{
  Path = $env:AI_CLIENT_INSTALLER_MAIN_PACKAGE
  ForceTargetApplicationShutdown = $true
  ErrorAction = 'Stop'
}
if ($dependencies.Count -gt 0) {
  $parameters.DependencyPath = [string[]]$dependencies
}
Add-AppxPackage @parameters
if ($env:AI_CLIENT_INSTALLER_REGISTER_FAMILY) {
  Add-AppxPackage -RegisterByFamilyName -MainPackage $env:AI_CLIENT_INSTALLER_REGISTER_FAMILY -ErrorAction Stop
}
"#;

const DETECT_APPX_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Import-Module Appx -ErrorAction Stop
$family = $env:AI_CLIENT_INSTALLER_PACKAGE_FAMILY
$pkg = Get-AppxPackage -ErrorAction SilentlyContinue |
  Where-Object { $_.PackageFamilyName -eq $family } |
  Sort-Object Version -Descending |
  Select-Object -First 1
if ($pkg) {
  [pscustomobject]@{
    installed = $true
    version = $pkg.Version.ToString()
    managed = [bool]$pkg.NonRemovable
    management_known = $true
    package_identity = [string]$pkg.Name
    package_family = [string]$pkg.PackageFamilyName
    publisher = [string]$pkg.Publisher
    architecture = [string]$pkg.Architecture
    evidence = ('AppX:' + $pkg.PackageFamilyName)
  } | ConvertTo-Json -Compress
} else {
  [pscustomobject]@{
    installed = $false
    version = $null
    managed = $false
    management_known = $true
    package_identity = $null
    package_family = $null
    publisher = $null
    architecture = $null
    evidence = 'No exact AppX family found'
  } | ConvertTo-Json -Compress
}
"#;

const LOCATE_WINGET_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Import-Module Appx -ErrorAction Stop
$family = $env:AI_CLIENT_INSTALLER_APP_INSTALLER_FAMILY
$pkg = Get-AppxPackage -ErrorAction SilentlyContinue |
  Where-Object { $_.PackageFamilyName -eq $family } |
  Sort-Object Version -Descending |
  Select-Object -First 1
if (-not $pkg) {
  [pscustomobject]@{ installed = $false } | ConvertTo-Json -Compress
  exit 0
}
$candidate = Join-Path -Path $pkg.InstallLocation -ChildPath 'winget.exe'
[pscustomobject]@{
  installed = $true
  package_identity = [string]$pkg.Name
  package_family = [string]$pkg.PackageFamilyName
  publisher = [string]$pkg.Publisher
  version = $pkg.Version.ToString()
  architecture = [string]$pkg.Architecture
  install_location = [string]$pkg.InstallLocation
  winget_path = if (Test-Path -LiteralPath $candidate -PathType Leaf) { [string](Get-Item -LiteralPath $candidate).FullName } else { $null }
} | ConvertTo-Json -Compress
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureClass {
    FallbackAllowed,
    Hard,
    Cancelled,
    ResultUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoreFailure {
    class: FailureClass,
    message: String,
}

impl StoreFailure {
    fn hard(message: impl Into<String>) -> Self {
        Self {
            class: FailureClass::Hard,
            message: message.into(),
        }
    }

    fn fallback(message: impl Into<String>) -> Self {
        Self {
            class: FailureClass::FallbackAllowed,
            message: message.into(),
        }
    }

    fn cancelled(message: impl Into<String>) -> Self {
        Self {
            class: FailureClass::Cancelled,
            message: message.into(),
        }
    }

    fn result_unknown(message: impl Into<String>) -> Self {
        Self {
            class: FailureClass::ResultUnknown,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WingetHealth {
    Healthy(String),
    RepairRequired(String),
    Degraded(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryOutcome {
    Applied,
    AlreadyCurrent,
}

#[derive(Debug, Clone)]
struct StoreExpectation {
    product: ProductId,
    architecture: Architecture,
    package_identity: String,
    package_family: String,
    publisher: String,
}

trait StoreBackend {
    fn cancelled(&self) -> bool;
    fn probe_winget(&mut self) -> Result<WingetHealth, StoreFailure>;
    fn repair_winget(&mut self) -> Result<(), StoreFailure>;
    fn run_primary(&mut self, installed: bool) -> Result<PrimaryOutcome, StoreFailure>;
    fn run_fallback(&mut self) -> Result<(), StoreFailure>;
    fn detect_target(&mut self) -> Result<Detection, StoreFailure>;
}

pub fn execute_microsoft_store_install(
    request: &StoreInstallRequest<'_>,
) -> Result<String, StoreInstallError> {
    let config = StoreRuntimeConfig::from_request(request).map_err(store_install_error)?;
    let expectation = StoreExpectation {
        product: request.plan.product,
        architecture: request.plan.architecture,
        package_identity: config.package_identity.clone(),
        package_family: config.package_family.clone(),
        publisher: config.publisher.clone(),
    };
    let mut backend = WindowsStoreBackend { request, config };
    run_store_workflow(&mut backend, request.initial_detection, &expectation)
        .map_err(store_install_error)
}

fn store_install_error(error: StoreFailure) -> StoreInstallError {
    match error.class {
        FailureClass::Cancelled => StoreInstallError::Cancelled(error.message),
        FailureClass::ResultUnknown => StoreInstallError::ResultUnknown(error.message),
        FailureClass::FallbackAllowed | FailureClass::Hard => {
            StoreInstallError::Failed(error.message)
        }
    }
}

fn run_store_workflow<B: StoreBackend>(
    backend: &mut B,
    initial: &Detection,
    expectation: &StoreExpectation,
) -> Result<String, StoreFailure> {
    if initial.installed {
        if initial.managed {
            return Err(StoreFailure::hard(
                "检测到受组织管理的 ChatGPT 安装，拒绝自动覆盖",
            ));
        }
        if !initial.management_known {
            return Err(StoreFailure::hard(
                "无法确认现有 ChatGPT 是否受组织管理，拒绝自动覆盖",
            ));
        }
        validate_store_detection(initial, expectation)?;
    }
    if backend.cancelled() {
        return Err(StoreFailure::cancelled("操作已在后台检查前取消"));
    }

    let first_health = backend.probe_winget()?;
    if let WingetHealth::RepairRequired(_) = first_health {
        backend.repair_winget()?;
        match backend.probe_winget()? {
            WingetHealth::RepairRequired(reason) => {
                return Err(StoreFailure::hard(format!(
                    "WinGet 已自愈一次但仍不可用：{reason}"
                )));
            }
            WingetHealth::Healthy(_) | WingetHealth::Degraded(_) => {}
        }
    }

    if backend.cancelled() {
        return Err(StoreFailure::cancelled(
            "操作已取消，尚未启动 Microsoft Store 部署",
        ));
    }
    let primary = backend.run_primary(initial.installed);
    let primary_outcome = match primary {
        Ok(outcome) => Some(outcome),
        Err(error) if error.class == FailureClass::FallbackAllowed => {
            if backend.cancelled() {
                return Err(StoreFailure::cancelled(
                    "操作已取消，尚未启动本地 AppX 兜底",
                ));
            }
            backend.run_fallback()?;
            None
        }
        Err(error) => return Err(error),
    };

    let detected = backend.detect_target()?;
    validate_store_detection(&detected, expectation)?;
    if let (Some(before), Some(after)) = (initial.version.as_deref(), detected.version.as_deref())
        && version_is_older(after, before)
    {
        return Err(StoreFailure::hard(format!(
            "安装后版本 {after} 低于原版本 {before}，拒绝认定成功"
        )));
    }

    let version = detected.version.as_deref().unwrap_or("版本未知");
    Ok(match primary_outcome {
        Some(PrimaryOutcome::AlreadyCurrent) => format!("已是最新版本：{version}"),
        _ => format!("复检成功：{version}"),
    })
}

fn validate_store_detection(
    detection: &Detection,
    expectation: &StoreExpectation,
) -> Result<(), StoreFailure> {
    if !detection.installed {
        return Err(StoreFailure::hard(format!(
            "部署结束后未发现 {}：{}",
            expectation.product.display_name(),
            detection.evidence
        )));
    }
    if detection.version.as_deref().is_none_or(str::is_empty) {
        return Err(StoreFailure::hard("目标 AppX 版本未知，拒绝认定成功"));
    }
    if detection.package_identity.as_deref() != Some(expectation.package_identity.as_str()) {
        return Err(StoreFailure::hard(format!(
            "目标 Package Identity 不匹配：{:?}",
            detection.package_identity
        )));
    }
    if detection.package_family.as_deref() != Some(expectation.package_family.as_str()) {
        return Err(StoreFailure::hard(format!(
            "目标 Package Family 不匹配：{:?}",
            detection.package_family
        )));
    }
    let publisher = detection
        .publisher
        .as_deref()
        .ok_or_else(|| StoreFailure::hard("目标 AppX Publisher 未知"))?;
    if !distinguished_name_eq(publisher, &expectation.publisher) {
        return Err(StoreFailure::hard(format!(
            "目标 AppX Publisher 不匹配：{publisher}"
        )));
    }
    if detection.architecture != Some(expectation.architecture) {
        return Err(StoreFailure::hard(format!(
            "目标 AppX 架构不匹配：期望 {:?}，实际 {:?}",
            expectation.architecture, detection.architecture
        )));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct StoreRuntimeConfig {
    store_id: String,
    architecture: Architecture,
    minimum_winget_version: String,
    release_api: Url,
    bundle_asset: String,
    dependencies_asset: String,
    app_installer_identity: String,
    app_installer_family: String,
    app_installer_publisher: String,
    package_identity: String,
    package_family: String,
    publisher: String,
    dependency_identity_prefixes: Vec<String>,
    dependency_publishers: Vec<String>,
}

impl StoreRuntimeConfig {
    fn from_request(request: &StoreInstallRequest<'_>) -> Result<Self, StoreFailure> {
        let trust = request.trust;
        let required = |value: &Option<String>, field: &str| {
            value
                .clone()
                .ok_or_else(|| StoreFailure::hard(format!("信任配置缺少 {field}")))
        };
        let store_id = required(&trust.store_id, "store_id")?;
        if store_id != request.plan.store_id
            || store_id.len() != 12
            || !store_id
                .chars()
                .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        {
            return Err(StoreFailure::hard("Store ID 与安装计划不一致或格式非法"));
        }
        let release_api = Url::parse(&required(
            &trust.app_installer_release_api,
            "app_installer_release_api",
        )?)
        .map_err(|error| StoreFailure::hard(format!("App Installer Release API 非法：{error}")))?;
        if !trust
            .entry_urls
            .iter()
            .any(|entry| entry == release_api.as_str())
        {
            return Err(StoreFailure::hard(
                "App Installer Release API 未固定在 entry_urls 中",
            ));
        }
        let bundle_asset = required(
            &trust.app_installer_bundle_asset,
            "app_installer_bundle_asset",
        )?;
        let dependencies_asset = required(
            &trust.app_installer_dependencies_asset,
            "app_installer_dependencies_asset",
        )?;
        validate_staged_file_name(&bundle_asset)
            .map_err(|error| StoreFailure::hard(error.to_string()))?;
        validate_staged_file_name(&dependencies_asset)
            .map_err(|error| StoreFailure::hard(error.to_string()))?;
        if trust.dependency_identity_prefixes.is_empty()
            || trust.dependency_publishers.is_empty()
            || trust
                .dependency_identity_prefixes
                .iter()
                .any(|value| value.trim().is_empty())
            || trust
                .dependency_publishers
                .iter()
                .any(|value| value.trim().is_empty())
        {
            return Err(StoreFailure::hard("App Installer 依赖信任配置不完整"));
        }
        let minimum_winget_version =
            required(&trust.minimum_winget_version, "minimum_winget_version")?;
        if parse_winget_version(&minimum_winget_version).as_deref()
            != Some(minimum_winget_version.as_str())
        {
            return Err(StoreFailure::hard("minimum_winget_version 格式非法"));
        }
        Ok(Self {
            store_id,
            architecture: request.plan.architecture,
            minimum_winget_version,
            release_api,
            bundle_asset,
            dependencies_asset,
            app_installer_identity: required(
                &trust.app_installer_identity,
                "app_installer_identity",
            )?,
            app_installer_family: required(&trust.app_installer_family, "app_installer_family")?,
            app_installer_publisher: required(
                &trust.app_installer_publisher,
                "app_installer_publisher",
            )?,
            package_identity: required(&trust.package_identity, "package_identity")?,
            package_family: required(&trust.package_family, "package_family")?,
            publisher: required(&trust.msix_publisher, "msix_publisher")?,
            dependency_identity_prefixes: trust.dependency_identity_prefixes.clone(),
            dependency_publishers: trust.dependency_publishers.clone(),
        })
    }
}

struct WindowsStoreBackend<'a> {
    request: &'a StoreInstallRequest<'a>,
    config: StoreRuntimeConfig,
}

impl WindowsStoreBackend<'_> {
    fn emit(&self, state: OperationState, message: impl Into<String>) {
        (self.request.on_update)(OperationUpdate {
            product: self.request.plan.product,
            state,
            message: message.into(),
        });
    }

    fn run_command(
        &self,
        plan: &PlannedCommand,
        timeout: Duration,
        cancel_before_side_effect: bool,
        detach_on_timeout: bool,
    ) -> Result<CommandOutcome, CommandRunError> {
        run_command(
            plan,
            timeout,
            cancel_before_side_effect.then_some(self.request.cancel),
            detach_on_timeout,
        )
    }

    fn repair_from_release(&self) -> Result<(), StoreFailure> {
        self.emit(
            OperationState::Downloading,
            "WinGet 缺失、过旧或证书健康异常；正在下载 Microsoft 官方 App Installer",
        );
        let client = safe_http_client()
            .map_err(|error| StoreFailure::hard(format!("无法创建官方元数据客户端：{error}")))?;
        let (_, source) =
            fetch_official_text(&client, &self.config.release_api, self.request.trust).map_err(
                |error| StoreFailure::hard(format!("读取 Microsoft stable Release 失败：{error}")),
            )?;
        let release: GithubRelease = serde_json::from_str(&source).map_err(|error| {
            StoreFailure::hard(format!("Microsoft Release 元数据无效：{error}"))
        })?;
        if release.draft || release.prerelease || release.tag_name.trim().is_empty() {
            return Err(StoreFailure::hard(
                "Microsoft latest Release 不是可接受的 stable Release",
            ));
        }
        let bundle_asset = select_release_asset(&release, &self.config.bundle_asset)?;
        let dependencies_asset = select_release_asset(&release, &self.config.dependencies_asset)?;
        let bundle_digest = parse_github_digest(bundle_asset.digest.as_deref())?;
        let dependencies_digest = parse_github_digest(dependencies_asset.digest.as_deref())?;
        let bundle = self.download_release_asset(bundle_asset, &bundle_digest)?;
        let dependencies = self.download_release_asset(dependencies_asset, &dependencies_digest)?;

        self.emit(
            OperationState::Verifying,
            "正在验证 App Installer 摘要、Microsoft 签名、Identity、Publisher、架构与依赖闭包",
        );
        let bundle_info = inspect_bundle(&bundle.staged_path)?;
        verify_authenticode_subject(
            &bundle.staged_path,
            std::slice::from_ref(&self.config.app_installer_publisher),
        )?;
        if bundle_info.name != self.config.app_installer_identity
            || !distinguished_name_eq(&bundle_info.publisher, &self.config.app_installer_publisher)
        {
            return Err(StoreFailure::hard(
                "App Installer bundle Identity 或 Publisher 不匹配",
            ));
        }
        let target_package = inspect_bundle_application_package(
            &bundle.staged_path,
            &bundle_info,
            self.config.architecture,
        )?;
        let target_version = target_package.version.clone();
        let extracted =
            extract_dependency_archive(&dependencies.staged_path, self.config.architecture)?;
        let mut verified_dependencies = Vec::new();
        let mut dependency_packages = Vec::new();
        for path in &extracted.paths {
            let info = inspect_package(path)?;
            verify_dependency(&info, path, &self.config)?;
            dependency_packages.push((path.clone(), info));
            verified_dependencies.push(VerifiedLocalFile {
                root: extracted.root.path().to_path_buf(),
                path: path.clone(),
                identity: inspect_staged_file(extracted.root.path(), path)
                    .map_err(|error| StoreFailure::hard(error.to_string()))?,
                expected_sha256: None,
            });
        }
        if verified_dependencies.is_empty() {
            return Err(StoreFailure::hard(
                "App Installer dependencies ZIP 未包含当前架构依赖",
            ));
        }
        let dependency_paths =
            validate_dependency_closure(&target_package.dependencies, &dependency_packages)?;
        let verified_bundle = VerifiedLocalFile {
            root: bundle.private_root.path().to_path_buf(),
            path: bundle.staged_path.clone(),
            identity: bundle.identity.clone(),
            expected_sha256: Some(bundle_digest),
        };
        reverify_files(std::iter::once(&verified_bundle).chain(verified_dependencies.iter()))?;
        let plan = plan_add_appx_command(
            &verified_bundle.path,
            &dependency_paths,
            Some(&self.config.app_installer_family),
        )?;
        self.emit(
            OperationState::Installing,
            format!(
                "正在后台更新 Microsoft App Installer ({})",
                release.tag_name
            ),
        );
        let outcome = self
            .run_command(&plan, Duration::from_secs(15 * 60), false, true)
            .map_err(map_side_effect_command_error)?;
        if outcome.exit_code != 0 {
            return Err(StoreFailure::hard(format!(
                "App Installer 更新失败，退出码 0x{:08X}：{}",
                outcome.exit_code,
                outcome.summary()
            )));
        }
        let detected = detect_appx_family(&self.config.app_installer_family)?;
        validate_app_installer_detection(&detected, &self.config, &target_version)?;
        Ok(())
    }

    fn download_release_asset(
        &self,
        asset: &GithubAsset,
        expected_sha256: &str,
    ) -> Result<crate::core::DownloadResult, StoreFailure> {
        let url = Url::parse(&asset.browser_download_url)
            .map_err(|error| StoreFailure::hard(format!("Release asset URL 无效：{error}")))?;
        let progress = |received: u64, total: Option<u64>| {
            let detail = total
                .filter(|total| *total > 0)
                .map(|total| format!("{:.1}%", received as f64 * 100.0 / total as f64))
                .unwrap_or_else(|| format!("{:.1} MiB", received as f64 / 1_048_576.0));
            self.emit(
                OperationState::Downloading,
                format!("下载 {}：{detail}", asset.name),
            );
        };
        let is_cancelled = || self.request.cancel.load(Ordering::Relaxed);
        let download = download_to_private_staging_controlled(
            &DownloadRequest {
                url,
                file_name: asset.name.clone(),
                trust: self.request.trust,
            },
            &DownloadControl {
                is_cancelled: &is_cancelled,
                on_progress: &progress,
            },
        )
        .map_err(|error| {
            if matches!(error, crate::core::DownloadError::Cancelled) {
                StoreFailure::cancelled("App Installer 下载已取消")
            } else {
                StoreFailure::hard(format!("App Installer 资产下载失败：{error}"))
            }
        })?;
        if !download
            .identity
            .sha256
            .eq_ignore_ascii_case(expected_sha256)
        {
            return Err(StoreFailure::hard(format!(
                "{} 的 GitHub Release digest 不匹配",
                asset.name
            )));
        }
        Ok(download)
    }

    fn fallback_download_and_install(&self) -> Result<(), StoreFailure> {
        self.emit(
            OperationState::Downloading,
            "Store 主路径发生允许兜底的传输失败；正在下载一次完整 AppX 闭包",
        );
        let staging = tempfile::Builder::new()
            .prefix("ai-client-installer-store-")
            .tempdir()
            .map_err(|error| StoreFailure::hard(format!("无法创建 Store 私有暂存目录：{error}")))?;
        let winget = locate_winget_path(&self.config)?.ok_or_else(|| {
            StoreFailure::hard("Store 兜底开始前未找到受信任的 App Installer WinGet")
        })?;
        let plan = plan_winget_download_command(
            &winget,
            &self.config.store_id,
            self.config.architecture,
            staging.path(),
        )?;
        let outcome = self
            .run_command(&plan, Duration::from_secs(30 * 60), true, false)
            .map_err(map_cancellable_command_error)?;
        if outcome.exit_code != 0 {
            return Err(StoreFailure::hard(format!(
                "Store 本地闭包下载失败，退出码 0x{:08X}：{}",
                outcome.exit_code,
                outcome.summary()
            )));
        }
        self.emit(
            OperationState::Verifying,
            "正在验证目标 AppX、依赖、Microsoft/OpenAI 签名、Identity、Publisher 与架构",
        );
        let closure = verify_store_download_closure(staging, &self.config)?;
        reverify_files(closure.verified_files.iter())?;
        let plan = plan_add_appx_command(&closure.main_path, &closure.dependencies, None)?;
        self.emit(
            OperationState::Installing,
            "正在后台安装已验证的 ChatGPT AppX 闭包",
        );
        let outcome = self
            .run_command(&plan, Duration::from_secs(30 * 60), false, true)
            .map_err(map_side_effect_command_error)?;
        if outcome.exit_code != 0 {
            return Err(StoreFailure::hard(format!(
                "本地 AppX 闭包安装失败，退出码 0x{:08X}：{}",
                outcome.exit_code,
                outcome.summary()
            )));
        }
        Ok(())
    }
}

impl StoreBackend for WindowsStoreBackend<'_> {
    fn cancelled(&self) -> bool {
        self.request.cancel.load(Ordering::Relaxed)
    }

    fn probe_winget(&mut self) -> Result<WingetHealth, StoreFailure> {
        self.emit(
            OperationState::Verifying,
            "正在检查 WinGet 版本与 Microsoft Store 后台源健康状态",
        );
        let Some(winget) = locate_winget_path(&self.config)? else {
            return Ok(WingetHealth::RepairRequired(
                "未找到受信任的 Microsoft App Installer WinGet".into(),
            ));
        };
        let version_plan = PlannedCommand {
            program: program_path(&winget, "WinGet")?,
            arguments: vec!["--version".into()],
            environment: Vec::new(),
        };
        let version = match self.run_command(&version_plan, Duration::from_secs(20), true, false) {
            Ok(outcome) if outcome.exit_code == 0 => parse_winget_version(&outcome.stdout)
                .ok_or_else(|| StoreFailure::hard("无法解析 WinGet 版本输出"))?,
            Ok(outcome) => {
                return Ok(WingetHealth::RepairRequired(format!(
                    "WinGet --version 退出码 0x{:08X}：{}",
                    outcome.exit_code,
                    outcome.summary()
                )));
            }
            Err(CommandRunError::NotFound) => {
                return Ok(WingetHealth::RepairRequired("未找到 winget.exe".into()));
            }
            Err(CommandRunError::Cancelled) => {
                return Err(StoreFailure::cancelled("WinGet 健康检查已取消"));
            }
            Err(CommandRunError::Timeout { .. }) => {
                return Ok(WingetHealth::RepairRequired("WinGet 版本检查超时".into()));
            }
            Err(CommandRunError::Io(error)) => {
                return Err(StoreFailure::hard(format!(
                    "无法执行 WinGet 版本检查：{error}"
                )));
            }
        };
        if version_is_older(&version, &self.config.minimum_winget_version) {
            return Ok(WingetHealth::RepairRequired(format!(
                "WinGet {version} 低于最小测试版本 {}",
                self.config.minimum_winget_version
            )));
        }

        let show_plan = plan_winget_show_command(&winget, &self.config.store_id)?;
        let outcome = self
            .run_command(&show_plan, Duration::from_secs(90), true, false)
            .map_err(map_cancellable_command_error)?;
        if outcome.exit_code == 0 {
            return Ok(WingetHealth::Healthy(format!("WinGet {version}")));
        }
        if outcome.exit_code == WINGET_PINNED_CERTIFICATE_MISMATCH {
            return Ok(WingetHealth::RepairRequired(format!(
                "WinGet {version} 的 Microsoft Store 证书固定已过期"
            )));
        }
        if is_hard_store_code(outcome.exit_code) {
            return Err(StoreFailure::hard(format!(
                "Microsoft Store 后台源被策略、授权或安全边界拒绝，退出码 0x{:08X}：{}",
                outcome.exit_code,
                outcome.summary()
            )));
        }
        Ok(WingetHealth::Degraded(format!(
            "Store 预检未成功，将仅尝试一次主路径；退出码 0x{:08X}",
            outcome.exit_code
        )))
    }

    fn repair_winget(&mut self) -> Result<(), StoreFailure> {
        self.repair_from_release()
    }

    fn run_primary(&mut self, installed: bool) -> Result<PrimaryOutcome, StoreFailure> {
        self.emit(
            OperationState::Installing,
            if installed {
                "正在通过 Microsoft Store 后台服务更新 ChatGPT"
            } else {
                "正在通过 Microsoft Store 后台服务安装 ChatGPT"
            },
        );
        let winget = locate_winget_path(&self.config)?.ok_or_else(|| {
            StoreFailure::hard("Store 主路径开始前未找到受信任的 App Installer WinGet")
        })?;
        let plan = plan_winget_primary_command(
            &winget,
            &self.config.store_id,
            self.config.architecture,
            installed,
        )?;
        let outcome = self
            .run_command(&plan, Duration::from_secs(30 * 60), false, true)
            .map_err(map_side_effect_command_error)?;
        if outcome.exit_code == 0 {
            return Ok(PrimaryOutcome::Applied);
        }
        if outcome.exit_code == WINGET_UPDATE_NOT_APPLICABLE
            || outcome.exit_code == WINGET_PACKAGE_ALREADY_INSTALLED
            || outcome.exit_code == WINGET_INSTALL_ALREADY_INSTALLED
        {
            return Ok(PrimaryOutcome::AlreadyCurrent);
        }
        let message = format!(
            "Store 主路径退出码 0x{:08X}：{}",
            outcome.exit_code,
            outcome.summary()
        );
        if is_fallback_allowed_code(outcome.exit_code) {
            Err(StoreFailure::fallback(message))
        } else {
            Err(StoreFailure::hard(message))
        }
    }

    fn run_fallback(&mut self) -> Result<(), StoreFailure> {
        self.fallback_download_and_install()
    }

    fn detect_target(&mut self) -> Result<Detection, StoreFailure> {
        self.emit(
            OperationState::Postchecking,
            "正在重新核对 ChatGPT Package Family、Publisher、架构与版本",
        );
        super::windows::detect_product(self.request.plan.product, Some(self.request.trust))
            .map_err(|error| StoreFailure::hard(format!("ChatGPT 复检失败：{error}")))
    }
}

#[derive(Debug)]
struct CommandOutcome {
    exit_code: u32,
    stdout: String,
    stderr: String,
}

impl CommandOutcome {
    fn summary(&self) -> String {
        let combined = format!("{} {}", self.stdout.trim(), self.stderr.trim());
        let compact = combined.split_whitespace().collect::<Vec<_>>().join(" ");
        if compact.is_empty() {
            "无附加输出".into()
        } else {
            compact.chars().take(600).collect()
        }
    }
}

#[derive(Debug)]
enum CommandRunError {
    NotFound,
    Cancelled,
    Timeout { detached: bool },
    Io(String),
}

fn run_command(
    plan: &PlannedCommand,
    timeout: Duration,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    detach_on_timeout: bool,
) -> Result<CommandOutcome, CommandRunError> {
    let mut stdout_capture =
        tempfile::tempfile().map_err(|error| CommandRunError::Io(error.to_string()))?;
    let mut stderr_capture =
        tempfile::tempfile().map_err(|error| CommandRunError::Io(error.to_string()))?;
    let stdout_child = stdout_capture
        .try_clone()
        .map_err(|error| CommandRunError::Io(error.to_string()))?;
    let stderr_child = stderr_capture
        .try_clone()
        .map_err(|error| CommandRunError::Io(error.to_string()))?;
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.arguments)
        .stdout(Stdio::from(stdout_child))
        .stderr(Stdio::from(stderr_child));
    if super::windows::is_powershell_program(&plan.program) {
        command.env_remove("PSModulePath");
    }
    for (key, value) in &plan.environment {
        command.env(key, value);
    }
    super::windows::hide_console_window(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CommandRunError::NotFound);
        }
        Err(error) => return Err(CommandRunError::Io(error.to_string())),
    };
    let started = Instant::now();
    loop {
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CommandRunError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = read_capture(&mut stdout_capture)?;
                let stderr = read_capture(&mut stderr_capture)?;
                return Ok(CommandOutcome {
                    exit_code: status.code().unwrap_or(-1) as u32,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {}
            Err(error) => return Err(CommandRunError::Io(error.to_string())),
        }
        if started.elapsed() >= timeout {
            if !detach_on_timeout {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Err(CommandRunError::Timeout {
                detached: detach_on_timeout,
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn read_capture(file: &mut File) -> Result<String, CommandRunError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| CommandRunError::Io(error.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_CAPTURE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CommandRunError::Io(error.to_string()))?;
    if bytes.len() as u64 > MAX_CAPTURE_BYTES {
        bytes.truncate(MAX_CAPTURE_BYTES as usize);
        bytes.extend_from_slice(b"\n[output truncated]");
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn map_cancellable_command_error(error: CommandRunError) -> StoreFailure {
    match error {
        CommandRunError::Cancelled => StoreFailure::cancelled("后台命令已取消"),
        CommandRunError::Timeout { .. } => StoreFailure::hard("后台命令超时并已终止"),
        CommandRunError::NotFound => StoreFailure::hard("未找到所需系统命令"),
        CommandRunError::Io(error) => StoreFailure::hard(format!("系统命令执行失败：{error}")),
    }
}

fn map_side_effect_command_error(error: CommandRunError) -> StoreFailure {
    match error {
        CommandRunError::Timeout { detached: true } => StoreFailure::result_unknown(
            "系统部署超过等待时限；未强制终止部署，结果未知，请刷新后复检",
        ),
        other => map_cancellable_command_error(other),
    }
}

#[derive(Debug, Deserialize)]
struct WingetLocationOutput {
    installed: bool,
    package_identity: Option<String>,
    package_family: Option<String>,
    publisher: Option<String>,
    version: Option<String>,
    architecture: Option<String>,
    install_location: Option<String>,
    winget_path: Option<String>,
}

fn locate_winget_path(config: &StoreRuntimeConfig) -> Result<Option<PathBuf>, StoreFailure> {
    let plan = PlannedCommand {
        program: super::windows::trusted_powershell_program().map_err(StoreFailure::hard)?,
        arguments: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-OutputFormat".into(),
            "Text".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-EncodedCommand".into(),
            encode_powershell(LOCATE_WINGET_SCRIPT),
        ],
        environment: vec![(
            "AI_CLIENT_INSTALLER_APP_INSTALLER_FAMILY".into(),
            config.app_installer_family.clone(),
        )],
    };
    let outcome = run_command(&plan, Duration::from_secs(60), None, false)
        .map_err(map_cancellable_command_error)?;
    if outcome.exit_code != 0 {
        return Err(StoreFailure::hard(format!(
            "无法定位 App Installer WinGet：{}",
            outcome.summary()
        )));
    }
    let parsed: WingetLocationOutput = serde_json::from_str(&outcome.stdout)
        .map_err(|error| StoreFailure::hard(format!("WinGet 定位输出无效：{error}")))?;
    if !parsed.installed {
        return Ok(None);
    }
    if parsed.package_identity.as_deref() != Some(config.app_installer_identity.as_str())
        || parsed.package_family.as_deref() != Some(config.app_installer_family.as_str())
    {
        return Err(StoreFailure::hard(
            "注册的 App Installer Identity 或 Package Family 不匹配",
        ));
    }
    let publisher = parsed
        .publisher
        .as_deref()
        .ok_or_else(|| StoreFailure::hard("注册的 App Installer Publisher 未知"))?;
    if !distinguished_name_eq(publisher, &config.app_installer_publisher) {
        return Err(StoreFailure::hard(format!(
            "注册的 App Installer Publisher 不匹配：{publisher}"
        )));
    }
    let architecture = parsed
        .architecture
        .as_deref()
        .map(parse_appx_architecture)
        .ok_or_else(|| StoreFailure::hard("注册的 App Installer 架构未知"))?;
    if !architecture.matches(config.architecture) {
        return Err(StoreFailure::hard(format!(
            "注册的 App Installer 架构不匹配：{:?}",
            parsed.architecture
        )));
    }
    if parsed.version.as_deref().is_none_or(str::is_empty) {
        return Err(StoreFailure::hard("注册的 App Installer 版本未知"));
    }
    let install_location = parsed
        .install_location
        .as_deref()
        .ok_or_else(|| StoreFailure::hard("注册的 App Installer 安装位置未知"))?;
    let winget_path = parsed
        .winget_path
        .as_deref()
        .ok_or_else(|| StoreFailure::hard("注册的 App Installer 不包含 winget.exe"))?;
    let canonical_root = fs::canonicalize(install_location)
        .map_err(|error| StoreFailure::hard(format!("无法核对 App Installer 安装位置：{error}")))?;
    let canonical_winget = fs::canonicalize(winget_path)
        .map_err(|error| StoreFailure::hard(format!("无法核对 winget.exe 路径：{error}")))?;
    if !canonical_winget.starts_with(&canonical_root)
        || canonical_winget
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.eq_ignore_ascii_case("winget.exe"))
    {
        return Err(StoreFailure::hard(
            "winget.exe 不在已注册的 App Installer 安装目录内",
        ));
    }
    let metadata = fs::metadata(&canonical_winget)
        .map_err(|error| StoreFailure::hard(format!("无法读取 winget.exe：{error}")))?;
    if !metadata.is_file() {
        return Err(StoreFailure::hard("已定位的 winget.exe 不是普通文件"));
    }
    // winget.exe is an AppX payload and is not necessarily Authenticode-signed as an individual
    // file. Trust is anchored to the exact registered App Installer identity/family/publisher and
    // the canonical payload path under its protected InstallLocation.
    Ok(Some(canonical_winget))
}

fn program_path(path: &Path, label: &str) -> Result<String, StoreFailure> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| StoreFailure::hard(format!("{label} 路径不是有效 Unicode")))
}

fn plan_winget_show_command(winget: &Path, store_id: &str) -> Result<PlannedCommand, StoreFailure> {
    Ok(PlannedCommand {
        program: program_path(winget, "WinGet")?,
        arguments: vec![
            "show".into(),
            "--id".into(),
            store_id.into(),
            "--source".into(),
            "msstore".into(),
            "--accept-source-agreements".into(),
            "--disable-interactivity".into(),
            "--no-progress".into(),
        ],
        environment: Vec::new(),
    })
}

fn plan_winget_primary_command(
    winget: &Path,
    store_id: &str,
    architecture: Architecture,
    installed: bool,
) -> Result<PlannedCommand, StoreFailure> {
    Ok(PlannedCommand {
        program: program_path(winget, "WinGet")?,
        arguments: vec![
            if installed { "upgrade" } else { "install" }.into(),
            "--id".into(),
            store_id.into(),
            "--source".into(),
            "msstore".into(),
            "--architecture".into(),
            winget_architecture(architecture).into(),
            "--silent".into(),
            "--disable-interactivity".into(),
            "--accept-source-agreements".into(),
            "--accept-package-agreements".into(),
            "--no-progress".into(),
        ],
        environment: Vec::new(),
    })
}

fn plan_winget_download_command(
    winget: &Path,
    store_id: &str,
    architecture: Architecture,
    staging: &Path,
) -> Result<PlannedCommand, StoreFailure> {
    let staging = staging
        .to_str()
        .ok_or_else(|| StoreFailure::hard("Store 暂存路径不是有效 Unicode"))?;
    Ok(PlannedCommand {
        program: program_path(winget, "WinGet")?,
        arguments: vec![
            "download".into(),
            "--id".into(),
            store_id.into(),
            "--source".into(),
            "msstore".into(),
            "--architecture".into(),
            winget_architecture(architecture).into(),
            "--platform".into(),
            "Windows.Desktop".into(),
            "--download-directory".into(),
            staging.into(),
            "--skip-license".into(),
            "--disable-interactivity".into(),
            "--accept-source-agreements".into(),
            "--accept-package-agreements".into(),
            "--no-progress".into(),
        ],
        environment: Vec::new(),
    })
}

fn plan_add_appx_command(
    main: &Path,
    dependencies: &[PathBuf],
    register_family: Option<&str>,
) -> Result<PlannedCommand, StoreFailure> {
    let main = main
        .to_str()
        .ok_or_else(|| StoreFailure::hard("AppX 主包路径不是有效 Unicode"))?
        .to_owned();
    let dependencies: Vec<_> = dependencies
        .iter()
        .map(|path| {
            path.to_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| StoreFailure::hard("AppX 依赖路径不是有效 Unicode"))
        })
        .collect::<Result<_, _>>()?;
    let dependencies = serde_json::to_string(&dependencies)
        .map_err(|error| StoreFailure::hard(format!("无法编码 AppX 依赖参数：{error}")))?;
    let mut environment = vec![
        ("AI_CLIENT_INSTALLER_MAIN_PACKAGE".into(), main),
        ("AI_CLIENT_INSTALLER_DEPENDENCIES_JSON".into(), dependencies),
    ];
    if let Some(family) = register_family {
        environment.push(("AI_CLIENT_INSTALLER_REGISTER_FAMILY".into(), family.into()));
    }
    Ok(PlannedCommand {
        program: super::windows::trusted_powershell_program().map_err(StoreFailure::hard)?,
        arguments: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-OutputFormat".into(),
            "Text".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-EncodedCommand".into(),
            encode_powershell(ADD_APPX_SCRIPT),
        ],
        environment,
    })
}

fn winget_architecture(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X64 => "x64",
        Architecture::Arm64 => "arm64",
        Architecture::Unsupported => "unsupported",
    }
}

fn parse_winget_version(output: &str) -> Option<String> {
    Regex::new(r"(?i)\bv?(\d+(?:\.\d+){1,3})\b")
        .expect("static WinGet version regex")
        .captures(output)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
}

fn is_fallback_allowed_code(code: u32) -> bool {
    matches!(
        code,
        0x8A15_0008
            | 0x8A15_006B
            | 0x8A15_006D
            | 0x8A15_007F
            | 0x8A15_0081
            | 0x8A15_0086
            | 0x8A15_0107
    )
}

fn is_hard_store_code(code: u32) -> bool {
    matches!(
        code,
        0x8A15_0010
            | 0x8A15_0011
            | 0x8A15_001B
            | 0x8A15_001C
            | 0x8A15_002D
            | 0x8A15_003A
            | 0x8A15_0041
            | 0x8A15_0046
            | 0x8A15_004C
            | 0x8A15_0074
            | 0x8A15_0075
            | 0x8A15_0076
            | 0x8A15_0077
            | 0x8A15_0078
            | 0x8A15_0080
            | 0x8A15_0082
            | 0x8A15_0083
            | 0x8A15_0084
            | 0x8A15_0085
            | 0x8A15_010E
            | 0x8A15_010F
            | 0x8A15_0110
            | 0x8A15_0113
    )
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

fn select_release_asset<'a>(
    release: &'a GithubRelease,
    name: &str,
) -> Result<&'a GithubAsset, StoreFailure> {
    let mut matches = release.assets.iter().filter(|asset| asset.name == name);
    let asset = matches
        .next()
        .ok_or_else(|| StoreFailure::hard(format!("Microsoft Release 缺少资产 {name}")))?;
    if matches.next().is_some() {
        return Err(StoreFailure::hard(format!(
            "Microsoft Release 中资产 {name} 不唯一"
        )));
    }
    Ok(asset)
}

fn parse_github_digest(value: Option<&str>) -> Result<String, StoreFailure> {
    let digest = value
        .and_then(|value| value.strip_prefix("sha256:"))
        .ok_or_else(|| StoreFailure::hard("Microsoft Release asset 缺少 SHA-256 digest"))?;
    if digest.len() != 64
        || !digest
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(StoreFailure::hard(
            "Microsoft Release asset SHA-256 digest 格式非法",
        ));
    }
    Ok(digest.to_ascii_lowercase())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppxArchitecture {
    X64,
    Arm64,
    Neutral,
    Unsupported,
}

impl AppxArchitecture {
    fn matches(self, expected: Architecture) -> bool {
        matches!(
            (self, expected),
            (Self::X64, Architecture::X64)
                | (Self::Arm64, Architecture::Arm64)
                | (Self::Neutral, _)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageDependencyRequirement {
    name: String,
    publisher: String,
    min_version: String,
}

#[derive(Debug, Clone)]
struct PackageInfo {
    name: String,
    publisher: String,
    version: String,
    architecture: AppxArchitecture,
    dependencies: Vec<PackageDependencyRequirement>,
}

#[derive(Debug)]
struct BundleApplication {
    architecture: AppxArchitecture,
    version: String,
    file_name: String,
}

#[derive(Debug)]
struct BundleInfo {
    name: String,
    publisher: String,
    applications: Vec<BundleApplication>,
}

impl BundleInfo {
    fn application(&self, architecture: Architecture) -> Result<&BundleApplication, StoreFailure> {
        let exact = self
            .applications
            .iter()
            .filter(|application| match architecture {
                Architecture::X64 => application.architecture == AppxArchitecture::X64,
                Architecture::Arm64 => application.architecture == AppxArchitecture::Arm64,
                Architecture::Unsupported => false,
            })
            .collect::<Vec<_>>();
        let candidates = if exact.is_empty() {
            self.applications
                .iter()
                .filter(|application| application.architecture == AppxArchitecture::Neutral)
                .collect::<Vec<_>>()
        } else {
            exact
        };
        if candidates.len() != 1 {
            return Err(StoreFailure::hard(format!(
                "AppX bundle 对目标架构包含 {} 个应用包，要求恰好一个",
                candidates.len()
            )));
        }
        Ok(candidates[0])
    }

    fn supports(&self, architecture: Architecture) -> bool {
        self.application(architecture).is_ok()
    }
}

fn inspect_package(path: &Path) -> Result<PackageInfo, StoreFailure> {
    let xml = read_zip_text(path, "AppxManifest.xml")?;
    inspect_package_xml(&xml)
}

fn inspect_package_xml(xml: &str) -> Result<PackageInfo, StoreFailure> {
    let identity = Regex::new(r"(?is)<Identity\b[^>]*>")
        .expect("static AppX identity regex")
        .find(xml)
        .map(|value| value.as_str())
        .ok_or_else(|| StoreFailure::hard("AppX manifest 缺少 Identity"))?;
    Ok(PackageInfo {
        name: xml_attribute(identity, "Name")?,
        publisher: xml_attribute(identity, "Publisher")?,
        version: parse_appx_version(&xml_attribute(identity, "Version")?, "AppX Identity")?,
        architecture: parse_appx_architecture(&xml_attribute(identity, "ProcessorArchitecture")?),
        dependencies: parse_package_dependencies(xml)?,
    })
}

fn inspect_bundle(path: &Path) -> Result<BundleInfo, StoreFailure> {
    let xml = read_zip_text(path, "AppxMetadata/AppxBundleManifest.xml")?;
    let identity = Regex::new(r"(?is)<Identity\b[^>]*>")
        .expect("static bundle identity regex")
        .find(&xml)
        .map(|value| value.as_str())
        .ok_or_else(|| StoreFailure::hard("AppX bundle manifest 缺少 Identity"))?;
    let mut applications = Vec::new();
    for package in Regex::new(r"(?is)<Package\b[^>]*>")
        .expect("static bundle package regex")
        .find_iter(&xml)
        .map(|value| value.as_str())
    {
        if xml_attribute_optional(package, "Type")
            .is_some_and(|value| !value.eq_ignore_ascii_case("application"))
        {
            continue;
        }
        let Some(architecture) = xml_attribute_optional(package, "Architecture") else {
            continue;
        };
        let Some(version) = xml_attribute_optional(package, "Version") else {
            continue;
        };
        let Some(file_name) = xml_attribute_optional(package, "FileName") else {
            continue;
        };
        applications.push(BundleApplication {
            architecture: parse_appx_architecture(&architecture),
            version: parse_appx_version(&version, "bundle application")?,
            file_name,
        });
    }
    Ok(BundleInfo {
        name: xml_attribute(identity, "Name")?,
        publisher: xml_attribute(identity, "Publisher")?,
        applications,
    })
}

fn parse_package_dependencies(
    xml: &str,
) -> Result<Vec<PackageDependencyRequirement>, StoreFailure> {
    let mut dependencies = Vec::new();
    for tag in Regex::new(r"(?is)<(?:[A-Za-z_][\w.-]*:)?PackageDependency\b[^>]*>")
        .expect("static package dependency regex")
        .find_iter(xml)
        .map(|value| value.as_str())
    {
        dependencies.push(PackageDependencyRequirement {
            name: xml_attribute(tag, "Name")?,
            publisher: xml_attribute(tag, "Publisher")?,
            min_version: parse_appx_version(
                &xml_attribute(tag, "MinVersion")?,
                "PackageDependency MinVersion",
            )?,
        });
    }
    Ok(dependencies)
}

fn parse_appx_version(value: &str, label: &str) -> Result<String, StoreFailure> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 4 || parts.iter().any(|part| part.parse::<u16>().is_err()) {
        return Err(StoreFailure::hard(format!(
            "{label} 版本不是四段 0-65535 数字：{value}"
        )));
    }
    Ok(value.to_owned())
}

fn read_zip_text(path: &Path, entry_name: &str) -> Result<String, StoreFailure> {
    let file = File::open(path)
        .map_err(|error| StoreFailure::hard(format!("无法打开 AppX ZIP：{error}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| StoreFailure::hard(format!("AppX ZIP 无效：{error}")))?;
    let mut entry = archive
        .by_name(entry_name)
        .map_err(|error| StoreFailure::hard(format!("AppX 缺少 {entry_name}：{error}")))?;
    if entry.size() > 4 * 1024 * 1024 {
        return Err(StoreFailure::hard("AppX manifest 超过 4 MiB"));
    }
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|error| StoreFailure::hard(format!("无法读取 AppX manifest：{error}")))?;
    Ok(text)
}

fn inspect_bundle_application_package(
    bundle_path: &Path,
    bundle: &BundleInfo,
    architecture: Architecture,
) -> Result<PackageInfo, StoreFailure> {
    let application = bundle.application(architecture)?;
    let nested_name = application.file_name.replace('\\', "/");
    let nested_path = Path::new(&nested_name);
    if nested_path.is_absolute()
        || nested_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(StoreFailure::hard(
            "AppX bundle application FileName 包含路径逃逸",
        ));
    }
    let file = File::open(bundle_path)
        .map_err(|error| StoreFailure::hard(format!("无法打开 AppX bundle：{error}")))?;
    let mut outer = ZipArchive::new(file)
        .map_err(|error| StoreFailure::hard(format!("AppX bundle ZIP 无效：{error}")))?;
    let mut nested_entry = outer.by_name(&nested_name).map_err(|error| {
        StoreFailure::hard(format!("AppX bundle 缺少应用包 {nested_name}：{error}"))
    })?;
    if nested_entry.size() > MAX_INNER_APPX_BYTES {
        return Err(StoreFailure::hard("AppX bundle 内部应用包超过 2 GiB"));
    }
    let mut nested_file = tempfile::tempfile()
        .map_err(|error| StoreFailure::hard(format!("无法创建内部 AppX 临时文件：{error}")))?;
    std::io::copy(&mut nested_entry, &mut nested_file)
        .map_err(|error| StoreFailure::hard(format!("无法提取内部 AppX：{error}")))?;
    nested_file
        .seek(SeekFrom::Start(0))
        .map_err(|error| StoreFailure::hard(format!("无法重置内部 AppX：{error}")))?;
    let mut nested = ZipArchive::new(nested_file)
        .map_err(|error| StoreFailure::hard(format!("内部 AppX ZIP 无效：{error}")))?;
    let mut manifest = nested
        .by_name("AppxManifest.xml")
        .map_err(|error| StoreFailure::hard(format!("内部 AppX 缺少 manifest：{error}")))?;
    if manifest.size() > 4 * 1024 * 1024 {
        return Err(StoreFailure::hard("内部 AppX manifest 超过 4 MiB"));
    }
    let mut xml = String::new();
    manifest
        .read_to_string(&mut xml)
        .map_err(|error| StoreFailure::hard(format!("无法读取内部 AppX manifest：{error}")))?;
    let package = inspect_package_xml(&xml)?;
    if package.name != bundle.name
        || !distinguished_name_eq(&package.publisher, &bundle.publisher)
        || !package.architecture.matches(architecture)
        || package.version != application.version
    {
        return Err(StoreFailure::hard(
            "bundle 内部主包的 Identity、Publisher、架构或版本与 bundle manifest 不一致",
        ));
    }
    Ok(package)
}

fn xml_attribute(tag: &str, name: &str) -> Result<String, StoreFailure> {
    xml_attribute_optional(tag, name)
        .ok_or_else(|| StoreFailure::hard(format!("AppX Identity 缺少 {name}")))
}

fn xml_attribute_optional(tag: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"(?i)\b{}\s*=\s*[\"']([^\"']+)[\"']"#, regex::escape(name));
    Regex::new(&pattern)
        .ok()?
        .captures(tag)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
}

fn parse_appx_architecture(value: &str) -> AppxArchitecture {
    match value.trim().to_ascii_lowercase().as_str() {
        "x64" => AppxArchitecture::X64,
        "arm64" => AppxArchitecture::Arm64,
        "neutral" => AppxArchitecture::Neutral,
        _ => AppxArchitecture::Unsupported,
    }
}

#[derive(Debug, Deserialize)]
struct SignatureOutput {
    status: String,
    signer_subject: Option<String>,
}

fn verify_authenticode_subject(
    path: &Path,
    allowed_publishers: &[String],
) -> Result<(), StoreFailure> {
    let plan = PlannedCommand {
        program: super::windows::trusted_powershell_program().map_err(StoreFailure::hard)?,
        arguments: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-OutputFormat".into(),
            "Text".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-EncodedCommand".into(),
            encode_powershell(AUTHENTICODE_SCRIPT),
        ],
        environment: vec![(
            "AI_CLIENT_INSTALLER_ARTIFACT".into(),
            path.to_str()
                .ok_or_else(|| StoreFailure::hard("AppX 路径不是有效 Unicode"))?
                .into(),
        )],
    };
    let outcome = run_command(&plan, Duration::from_secs(60), None, false)
        .map_err(map_cancellable_command_error)?;
    if outcome.exit_code != 0 {
        return Err(StoreFailure::hard(format!(
            "AppX 签名检查失败：{}",
            outcome.summary()
        )));
    }
    let parsed: SignatureOutput = serde_json::from_str(&outcome.stdout)
        .map_err(|error| StoreFailure::hard(format!("AppX 签名输出无效：{error}")))?;
    if parsed.status != "Valid" {
        return Err(StoreFailure::hard(format!(
            "AppX 签名状态不是 Valid：{}",
            parsed.status
        )));
    }
    let subject = parsed
        .signer_subject
        .as_deref()
        .ok_or_else(|| StoreFailure::hard("AppX 有效签名没有 signer subject"))?;
    if !allowed_publishers
        .iter()
        .any(|publisher| distinguished_name_eq(subject, publisher))
    {
        return Err(StoreFailure::hard(format!(
            "AppX signer 不在固定 Publisher 集合内：{subject}"
        )));
    }
    Ok(())
}

struct ExtractedDependencies {
    root: TempDir,
    paths: Vec<PathBuf>,
}

fn extract_dependency_archive(
    archive_path: &Path,
    architecture: Architecture,
) -> Result<ExtractedDependencies, StoreFailure> {
    let file = File::open(archive_path)
        .map_err(|error| StoreFailure::hard(format!("无法打开 dependencies ZIP：{error}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| StoreFailure::hard(format!("dependencies ZIP 无效：{error}")))?;
    let root = tempfile::Builder::new()
        .prefix("ai-client-installer-winget-deps-")
        .tempdir()
        .map_err(|error| StoreFailure::hard(format!("无法创建依赖暂存目录：{error}")))?;
    let prefix = format!("{}/", winget_architecture(architecture));
    let mut total = 0_u64;
    let mut paths = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| StoreFailure::hard(format!("读取 dependencies ZIP 失败：{error}")))?;
        if entry.is_dir() || !entry.name().starts_with(&prefix) {
            continue;
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| StoreFailure::hard("dependencies ZIP 包含路径逃逸"))?;
        let file_name = enclosed
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| StoreFailure::hard("dependencies ZIP 文件名非法"))?;
        validate_staged_file_name(file_name)
            .map_err(|error| StoreFailure::hard(error.to_string()))?;
        let extension = Path::new(file_name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "appx" | "msix") {
            return Err(StoreFailure::hard(format!(
                "dependencies ZIP 当前架构目录包含非 AppX 文件：{file_name}"
            )));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_DEPENDENCY_ARCHIVE_BYTES || paths.len() >= MAX_DEPENDENCY_COUNT {
            return Err(StoreFailure::hard("dependencies ZIP 超出安全上限"));
        }
        let target = root.path().join(file_name);
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .map_err(|error| StoreFailure::hard(format!("无法提取 AppX 依赖：{error}")))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|error| StoreFailure::hard(format!("提取 AppX 依赖失败：{error}")))?;
        output
            .flush()
            .map_err(|error| StoreFailure::hard(format!("写入 AppX 依赖失败：{error}")))?;
        paths.push(target);
    }
    Ok(ExtractedDependencies { root, paths })
}

fn verify_dependency(
    info: &PackageInfo,
    path: &Path,
    config: &StoreRuntimeConfig,
) -> Result<(), StoreFailure> {
    if !info.architecture.matches(config.architecture) {
        return Err(StoreFailure::hard(format!(
            "AppX 依赖架构不匹配：{}",
            path.display()
        )));
    }
    if !config
        .dependency_identity_prefixes
        .iter()
        .any(|prefix| info.name.starts_with(prefix))
    {
        return Err(StoreFailure::hard(format!(
            "AppX 依赖 Identity 未固定：{}",
            info.name
        )));
    }
    if !config
        .dependency_publishers
        .iter()
        .any(|publisher| distinguished_name_eq(&info.publisher, publisher))
    {
        return Err(StoreFailure::hard(format!(
            "AppX 依赖 Publisher 未固定：{}",
            info.publisher
        )));
    }
    verify_authenticode_subject(path, &config.dependency_publishers)
}

fn validate_dependency_closure(
    root_requirements: &[PackageDependencyRequirement],
    packages: &[(PathBuf, PackageInfo)],
) -> Result<Vec<PathBuf>, StoreFailure> {
    let mut by_name = BTreeMap::<String, (&PathBuf, &PackageInfo)>::new();
    for (path, info) in packages {
        let key = info.name.to_ascii_lowercase();
        if by_name.insert(key, (path, info)).is_some() {
            return Err(StoreFailure::hard(format!(
                "AppX 依赖闭包包含重复 Identity：{}",
                info.name
            )));
        }
    }

    let mut pending: VecDeque<_> = root_requirements.iter().cloned().collect();
    let mut reachable = BTreeSet::new();
    while let Some(requirement) = pending.pop_front() {
        let key = requirement.name.to_ascii_lowercase();
        let (_, package) = by_name.get(&key).ok_or_else(|| {
            StoreFailure::hard(format!(
                "AppX 依赖闭包缺少 {} >= {}",
                requirement.name, requirement.min_version
            ))
        })?;
        if !distinguished_name_eq(&package.publisher, &requirement.publisher) {
            return Err(StoreFailure::hard(format!(
                "AppX 依赖 {} 的 Publisher 与主包声明不一致",
                requirement.name
            )));
        }
        if version_is_older(&package.version, &requirement.min_version) {
            return Err(StoreFailure::hard(format!(
                "AppX 依赖 {} 版本 {} 低于主包要求 {}",
                requirement.name, package.version, requirement.min_version
            )));
        }
        if reachable.insert(key) {
            pending.extend(package.dependencies.iter().cloned());
        }
    }

    let extras = by_name
        .iter()
        .filter(|(name, _)| !reachable.contains(*name))
        .map(|(_, (_, info))| info.name.clone())
        .collect::<Vec<_>>();
    if !extras.is_empty() {
        return Err(StoreFailure::hard(format!(
            "AppX 依赖闭包包含主包依赖图未引用的包：{}",
            extras.join(", ")
        )));
    }

    Ok(by_name
        .into_iter()
        .filter(|(name, _)| reachable.contains(name))
        .map(|(_, (path, _))| path.clone())
        .collect())
}

struct VerifiedLocalFile {
    root: PathBuf,
    path: PathBuf,
    identity: StableFileIdentity,
    expected_sha256: Option<String>,
}

fn reverify_files<'a>(
    files: impl IntoIterator<Item = &'a VerifiedLocalFile>,
) -> Result<(), StoreFailure> {
    for file in files {
        verify_staged_identity(
            &file.root,
            &file.path,
            &file.identity,
            file.expected_sha256.as_deref(),
        )
        .map_err(|error| {
            StoreFailure::hard(format!(
                "AppX 文件在执行交接前发生变化：{}：{error}",
                file.path.display()
            ))
        })?;
    }
    Ok(())
}

struct VerifiedStoreClosure {
    _staging: TempDir,
    main_path: PathBuf,
    dependencies: Vec<PathBuf>,
    verified_files: Vec<VerifiedLocalFile>,
}

fn verify_store_download_closure(
    staging: TempDir,
    config: &StoreRuntimeConfig,
) -> Result<VerifiedStoreClosure, StoreFailure> {
    let files = collect_files(staging.path(), 0)?;
    let mut main = None;
    let mut dependency_packages = Vec::new();
    let mut verified_files = Vec::new();
    for path in files {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "exe" | "msi") {
            return Err(StoreFailure::hard(format!(
                "Store 下载闭包出现非 AppX 可执行文件：{}",
                path.display()
            )));
        }
        if !matches!(
            extension.as_str(),
            "appx" | "msix" | "appxbundle" | "msixbundle"
        ) {
            continue;
        }
        let identity = inspect_staged_file(staging.path(), &path)
            .map_err(|error| StoreFailure::hard(error.to_string()))?;
        if matches!(extension.as_str(), "appxbundle" | "msixbundle") {
            let info = inspect_bundle(&path)?;
            verify_authenticode_subject(&path, std::slice::from_ref(&config.publisher))?;
            if info.name != config.package_identity
                || !distinguished_name_eq(&info.publisher, &config.publisher)
                || !info.supports(config.architecture)
            {
                return Err(StoreFailure::hard(
                    "Store 主 bundle 的 Identity、Publisher 或架构不匹配",
                ));
            }
            let application =
                inspect_bundle_application_package(&path, &info, config.architecture)?;
            if main
                .replace((path.clone(), application.dependencies))
                .is_some()
            {
                return Err(StoreFailure::hard("Store 下载闭包包含多个目标主包"));
            }
        } else {
            let info = inspect_package(&path)?;
            if info.name == config.package_identity
                && distinguished_name_eq(&info.publisher, &config.publisher)
            {
                verify_authenticode_subject(&path, std::slice::from_ref(&config.publisher))?;
                if !info.architecture.matches(config.architecture)
                    || info.architecture == AppxArchitecture::Neutral
                {
                    return Err(StoreFailure::hard(
                        "Store 下载闭包包含目标 Identity 的非目标架构或 neutral 附加包",
                    ));
                }
                if main.replace((path.clone(), info.dependencies)).is_some() {
                    return Err(StoreFailure::hard("Store 下载闭包包含多个目标主包"));
                }
            } else {
                verify_dependency(&info, &path, config)?;
                dependency_packages.push((path.clone(), info));
            }
        }
        verified_files.push(VerifiedLocalFile {
            root: staging.path().to_path_buf(),
            path,
            identity,
            expected_sha256: None,
        });
    }
    let (main_path, root_requirements) = main.ok_or_else(|| {
        StoreFailure::hard("Store 下载闭包未找到固定 Identity/Publisher/架构的目标主包")
    })?;
    let dependencies = validate_dependency_closure(&root_requirements, &dependency_packages)?;
    Ok(VerifiedStoreClosure {
        _staging: staging,
        main_path,
        dependencies,
        verified_files,
    })
}

fn collect_files(root: &Path, depth: usize) -> Result<Vec<PathBuf>, StoreFailure> {
    if depth > 8 {
        return Err(StoreFailure::hard("Store 下载目录嵌套层级超过安全上限"));
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|error| StoreFailure::hard(format!("无法读取 Store 下载目录：{error}")))?
    {
        let entry =
            entry.map_err(|error| StoreFailure::hard(format!("读取 Store 下载项失败：{error}")))?;
        let metadata = entry
            .file_type()
            .map_err(|error| StoreFailure::hard(format!("读取 Store 下载项类型失败：{error}")))?;
        if metadata.is_symlink() {
            return Err(StoreFailure::hard("Store 下载目录包含符号链接"));
        }
        if metadata.is_dir() {
            files.extend(collect_files(&entry.path(), depth + 1)?);
        } else if metadata.is_file() {
            files.push(entry.path());
        }
    }
    Ok(files)
}

#[derive(Debug, Deserialize)]
struct AppxDetectionOutput {
    installed: bool,
    version: Option<String>,
    managed: bool,
    management_known: bool,
    package_identity: Option<String>,
    package_family: Option<String>,
    publisher: Option<String>,
    architecture: Option<String>,
    evidence: String,
}

fn detect_appx_family(family: &str) -> Result<Detection, StoreFailure> {
    let plan = PlannedCommand {
        program: super::windows::trusted_powershell_program().map_err(StoreFailure::hard)?,
        arguments: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-OutputFormat".into(),
            "Text".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-EncodedCommand".into(),
            encode_powershell(DETECT_APPX_SCRIPT),
        ],
        environment: vec![("AI_CLIENT_INSTALLER_PACKAGE_FAMILY".into(), family.into())],
    };
    let outcome = run_command(&plan, Duration::from_secs(60), None, false)
        .map_err(map_cancellable_command_error)?;
    if outcome.exit_code != 0 {
        return Err(StoreFailure::hard(format!(
            "AppX family 复检失败：{}",
            outcome.summary()
        )));
    }
    let parsed: AppxDetectionOutput = serde_json::from_str(&outcome.stdout)
        .map_err(|error| StoreFailure::hard(format!("AppX family 复检输出无效：{error}")))?;
    Ok(Detection {
        installed: parsed.installed,
        version: parsed.version,
        managed: parsed.managed,
        management_known: parsed.management_known,
        package_identity: parsed.package_identity,
        package_family: parsed.package_family,
        publisher: parsed.publisher,
        architecture: parsed.architecture.as_deref().and_then(|value| {
            match value.trim().to_ascii_lowercase().as_str() {
                "x64" => Some(Architecture::X64),
                "arm64" => Some(Architecture::Arm64),
                _ => None,
            }
        }),
        evidence: parsed.evidence,
    })
}

fn validate_app_installer_detection(
    detection: &Detection,
    config: &StoreRuntimeConfig,
    target_version: &str,
) -> Result<(), StoreFailure> {
    if !detection.installed
        || detection.package_identity.as_deref() != Some(config.app_installer_identity.as_str())
        || detection.package_family.as_deref() != Some(config.app_installer_family.as_str())
        || detection.architecture != Some(config.architecture)
    {
        return Err(StoreFailure::hard(
            "App Installer 更新后 Identity、Family 或架构复检失败",
        ));
    }
    let publisher = detection
        .publisher
        .as_deref()
        .ok_or_else(|| StoreFailure::hard("App Installer 更新后 Publisher 未知"))?;
    if !distinguished_name_eq(publisher, &config.app_installer_publisher) {
        return Err(StoreFailure::hard("App Installer 更新后 Publisher 不匹配"));
    }
    let version = detection
        .version
        .as_deref()
        .ok_or_else(|| StoreFailure::hard("App Installer 更新后版本未知"))?;
    if version_is_older(version, target_version) {
        return Err(StoreFailure::hard(format!(
            "App Installer 更新后版本 {version} 低于目标 {target_version}"
        )));
    }
    Ok(())
}

fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn distinguished_name_eq(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        let mut components: Vec<_> = value
            .split(',')
            .map(|component| component.trim().to_ascii_lowercase())
            .filter(|component| !component.is_empty())
            .collect();
        components.sort();
        components
    };
    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::core::{MicrosoftStorePlan, OperatingSystem, TrustRegistry, sha256_file};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Probe,
        Repair,
        Primary(bool),
        Fallback,
        Detect,
    }

    struct FakeBackend {
        calls: Vec<Call>,
        health: VecDeque<Result<WingetHealth, StoreFailure>>,
        primary: Result<PrimaryOutcome, StoreFailure>,
        fallback: Result<(), StoreFailure>,
        detection: Result<Detection, StoreFailure>,
        cancelled: bool,
    }

    impl StoreBackend for FakeBackend {
        fn cancelled(&self) -> bool {
            self.cancelled
        }

        fn probe_winget(&mut self) -> Result<WingetHealth, StoreFailure> {
            self.calls.push(Call::Probe);
            self.health.pop_front().expect("health result")
        }

        fn repair_winget(&mut self) -> Result<(), StoreFailure> {
            self.calls.push(Call::Repair);
            Ok(())
        }

        fn run_primary(&mut self, installed: bool) -> Result<PrimaryOutcome, StoreFailure> {
            self.calls.push(Call::Primary(installed));
            self.primary.clone()
        }

        fn run_fallback(&mut self) -> Result<(), StoreFailure> {
            self.calls.push(Call::Fallback);
            self.fallback.clone()
        }

        fn detect_target(&mut self) -> Result<Detection, StoreFailure> {
            self.calls.push(Call::Detect);
            self.detection.clone()
        }
    }

    fn expectation() -> StoreExpectation {
        StoreExpectation {
            product: ProductId::ChatGpt,
            architecture: Architecture::X64,
            package_identity: "OpenAI.Codex".into(),
            package_family: "OpenAI.Codex_2p2nqsd0c76g0".into(),
            publisher: "CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B".into(),
        }
    }

    #[test]
    fn appx_fallback_uses_the_supported_path_parameter_and_closes_target_apps() {
        assert!(ADD_APPX_SCRIPT.contains("Path ="));
        assert!(ADD_APPX_SCRIPT.contains("ForceTargetApplicationShutdown = $true"));
        assert!(!ADD_APPX_SCRIPT.contains("LiteralPath ="));
    }

    fn absent() -> Detection {
        Detection::absent("fixture")
    }

    fn installed(version: &str) -> Detection {
        Detection {
            installed: true,
            version: Some(version.into()),
            managed: false,
            management_known: true,
            package_identity: Some("OpenAI.Codex".into()),
            package_family: Some("OpenAI.Codex_2p2nqsd0c76g0".into()),
            publisher: Some("CN=50BDFD77-8903-4850-9FFE-6E8522F64D5B".into()),
            architecture: Some(Architecture::X64),
            evidence: "fixture".into(),
        }
    }

    fn backend(health: Vec<Result<WingetHealth, StoreFailure>>) -> FakeBackend {
        FakeBackend {
            calls: Vec::new(),
            health: health.into(),
            primary: Ok(PrimaryOutcome::Applied),
            fallback: Ok(()),
            detection: Ok(installed("26.800.1.0")),
            cancelled: false,
        }
    }

    #[test]
    fn healthy_path_runs_primary_once_and_postchecks() {
        let mut fake = backend(vec![Ok(WingetHealth::Healthy("1.29".into()))]);
        let message = run_store_workflow(&mut fake, &absent(), &expectation()).unwrap();
        assert!(message.contains("复检成功"));
        assert_eq!(
            fake.calls,
            vec![Call::Probe, Call::Primary(false), Call::Detect]
        );
    }

    #[test]
    fn repair_is_bounded_to_one_attempt_before_primary() {
        let mut fake = backend(vec![
            Ok(WingetHealth::RepairRequired("old".into())),
            Ok(WingetHealth::Healthy("new".into())),
        ]);
        run_store_workflow(&mut fake, &absent(), &expectation()).unwrap();
        assert_eq!(
            fake.calls,
            vec![
                Call::Probe,
                Call::Repair,
                Call::Probe,
                Call::Primary(false),
                Call::Detect
            ]
        );
    }

    #[test]
    fn second_repair_requirement_fails_without_looping() {
        let mut fake = backend(vec![
            Ok(WingetHealth::RepairRequired("old".into())),
            Ok(WingetHealth::RepairRequired("still broken".into())),
        ]);
        let error = run_store_workflow(&mut fake, &absent(), &expectation()).unwrap_err();
        assert_eq!(error.class, FailureClass::Hard);
        assert_eq!(fake.calls, vec![Call::Probe, Call::Repair, Call::Probe]);
    }

    #[test]
    fn eligible_primary_failure_uses_exactly_one_fallback() {
        let mut fake = backend(vec![Ok(WingetHealth::Healthy("1.29".into()))]);
        fake.primary = Err(StoreFailure::fallback("transport"));
        run_store_workflow(&mut fake, &absent(), &expectation()).unwrap();
        assert_eq!(
            fake.calls,
            vec![
                Call::Probe,
                Call::Primary(false),
                Call::Fallback,
                Call::Detect
            ]
        );
    }

    #[test]
    fn hard_primary_failure_never_uses_fallback() {
        let mut fake = backend(vec![Ok(WingetHealth::Healthy("1.29".into()))]);
        fake.primary = Err(StoreFailure::hard("policy"));
        assert!(run_store_workflow(&mut fake, &absent(), &expectation()).is_err());
        assert_eq!(fake.calls, vec![Call::Probe, Call::Primary(false)]);
    }

    #[test]
    fn postcheck_identity_mismatch_rejects_success() {
        let mut fake = backend(vec![Ok(WingetHealth::Healthy("1.29".into()))]);
        let mut wrong = installed("26.800.1.0");
        wrong.package_family = Some("wrong_family".into());
        fake.detection = Ok(wrong);
        assert!(run_store_workflow(&mut fake, &absent(), &expectation()).is_err());
        assert_eq!(
            fake.calls,
            vec![Call::Probe, Call::Primary(false), Call::Detect]
        );
    }

    #[test]
    fn managed_existing_install_is_rejected_before_external_calls() {
        let mut fake = backend(vec![Ok(WingetHealth::Healthy("1.29".into()))]);
        let mut current = installed("26.700.0.0");
        current.managed = true;
        assert!(run_store_workflow(&mut fake, &current, &expectation()).is_err());
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn cancellation_before_store_work_starts_has_no_external_calls() {
        let mut fake = backend(vec![Ok(WingetHealth::Healthy("1.29".into()))]);
        fake.cancelled = true;
        let error = run_store_workflow(&mut fake, &absent(), &expectation()).unwrap_err();
        assert_eq!(error.class, FailureClass::Cancelled);
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn failed_fallback_is_not_retried_and_skips_postcheck() {
        let mut fake = backend(vec![Ok(WingetHealth::Healthy("1.29".into()))]);
        fake.primary = Err(StoreFailure::fallback("transport"));
        fake.fallback = Err(StoreFailure::hard("closure incomplete"));
        assert!(run_store_workflow(&mut fake, &absent(), &expectation()).is_err());
        assert_eq!(
            fake.calls,
            vec![Call::Probe, Call::Primary(false), Call::Fallback]
        );
    }

    #[test]
    fn postcheck_rejects_a_store_downgrade() {
        let current = installed("26.900.0.0");
        let mut fake = backend(vec![Ok(WingetHealth::Healthy("1.29".into()))]);
        fake.detection = Ok(installed("26.800.0.0"));
        let error = run_store_workflow(&mut fake, &current, &expectation()).unwrap_err();
        assert!(error.message.contains("低于原版本"));
        assert_eq!(
            fake.calls,
            vec![Call::Probe, Call::Primary(true), Call::Detect]
        );
    }

    fn requirement(name: &str, min_version: &str) -> PackageDependencyRequirement {
        PackageDependencyRequirement {
            name: name.into(),
            publisher: "CN=Microsoft Corporation, O=Microsoft Corporation, C=US".into(),
            min_version: min_version.into(),
        }
    }

    fn dependency_package(
        name: &str,
        version: &str,
        dependencies: Vec<PackageDependencyRequirement>,
    ) -> (PathBuf, PackageInfo) {
        (
            PathBuf::from(format!(r"C:\Temp\{name}.msix")),
            PackageInfo {
                name: name.into(),
                publisher: "C=US, O=Microsoft Corporation, CN=Microsoft Corporation".into(),
                version: version.into(),
                architecture: AppxArchitecture::X64,
                dependencies,
            },
        )
    }

    #[test]
    fn dependency_closure_requires_the_exact_transitive_graph_and_min_versions() {
        let root = vec![requirement("Microsoft.Framework.A", "2.0.0.0")];
        let complete = vec![
            dependency_package(
                "Microsoft.Framework.A",
                "2.1.0.0",
                vec![requirement("Microsoft.Framework.B", "1.0.0.0")],
            ),
            dependency_package("Microsoft.Framework.B", "1.0.0.0", Vec::new()),
        ];
        let paths = validate_dependency_closure(&root, &complete).unwrap();
        assert_eq!(paths.len(), 2);

        let missing_transitive = vec![complete[0].clone()];
        assert!(
            validate_dependency_closure(&root, &missing_transitive)
                .unwrap_err()
                .message
                .contains("缺少 Microsoft.Framework.B")
        );

        let too_old = vec![
            dependency_package(
                "Microsoft.Framework.A",
                "1.9.0.0",
                vec![requirement("Microsoft.Framework.B", "1.0.0.0")],
            ),
            complete[1].clone(),
        ];
        assert!(
            validate_dependency_closure(&root, &too_old)
                .unwrap_err()
                .message
                .contains("低于主包要求")
        );

        let mut over_inclusive = complete;
        over_inclusive.push(dependency_package(
            "Microsoft.Framework.Unused",
            "1.0.0.0",
            Vec::new(),
        ));
        assert!(
            validate_dependency_closure(&root, &over_inclusive)
                .unwrap_err()
                .message
                .contains("未引用")
        );
    }

    #[test]
    fn package_dependency_parser_reads_namespaced_requirements() {
        let xml = r#"
<Package xmlns:uap10="urn:test">
  <Identity Name="OpenAI.Codex" Publisher="CN=OpenAI" Version="1.2.3.4" ProcessorArchitecture="x64" />
  <Dependencies>
    <uap10:PackageDependency Name="Microsoft.WindowsAppRuntime.1.8" Publisher="CN=Microsoft Corporation" MinVersion="8000.1.2.3" />
  </Dependencies>
</Package>
"#;
        let info = inspect_package_xml(xml).unwrap();
        assert_eq!(info.version, "1.2.3.4");
        assert_eq!(info.dependencies.len(), 1);
        assert_eq!(info.dependencies[0].name, "Microsoft.WindowsAppRuntime.1.8");
        assert_eq!(info.dependencies[0].min_version, "8000.1.2.3");
    }

    #[test]
    fn winget_commands_use_structured_arguments_and_fixed_store_id() {
        let winget = Path::new(
            r"C:\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_1.29.0.0_x64__8wekyb3d8bbwe\winget.exe",
        );
        let primary =
            plan_winget_primary_command(winget, "9PLM9XGG6VKS", Architecture::X64, false).unwrap();
        assert!(Path::new(&primary.program).is_absolute());
        assert_eq!(
            Path::new(&primary.program)
                .file_name()
                .and_then(|name| name.to_str()),
            Some("winget.exe")
        );
        assert_eq!(primary.arguments[0], "install");
        assert!(
            primary
                .arguments
                .windows(2)
                .any(|pair| pair == ["--id", "9PLM9XGG6VKS"])
        );
        assert!(
            primary
                .arguments
                .windows(2)
                .any(|pair| pair == ["--source", "msstore"])
        );
        assert!(
            primary
                .arguments
                .contains(&"--disable-interactivity".into())
        );
        assert!(
            !primary
                .arguments
                .iter()
                .any(|argument| argument.contains("ms-windows-store:"))
        );

        let root = Path::new(r"C:\Temp\chatgpt & calc");
        let download =
            plan_winget_download_command(winget, "9PLM9XGG6VKS", Architecture::X64, root).unwrap();
        assert!(
            download
                .arguments
                .iter()
                .any(|argument| argument == r"C:\Temp\chatgpt & calc")
        );
        assert!(download.arguments.contains(&"--skip-license".into()));
    }

    #[test]
    fn winget_error_classes_do_not_fallback_on_policy_or_identity_failures() {
        assert!(is_fallback_allowed_code(0x8A15_0008));
        assert!(!is_fallback_allowed_code(0x8A15_001B));
        assert!(!is_fallback_allowed_code(0x8A15_002D));
        assert!(is_hard_store_code(0x8A15_001B));
        assert_eq!(WINGET_PINNED_CERTIFICATE_MISMATCH, 0x8A15_005E);
    }

    #[test]
    fn result_unknown_is_not_collapsed_into_a_hard_failure() {
        let mapped = store_install_error(StoreFailure::result_unknown("deployment continues"));
        assert!(
            matches!(mapped, StoreInstallError::ResultUnknown(message) if message == "deployment continues")
        );
    }

    #[test]
    #[ignore = "requires the current official App Installer assets in a local proof directory"]
    fn current_official_app_installer_artifacts_match_embedded_trust() {
        let proof_root = std::env::var("AI_CLIENT_INSTALLER_WINGET_PROOF_ROOT")
            .expect("AI_CLIENT_INSTALLER_WINGET_PROOF_ROOT");
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::ChatGpt,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        let plan = MicrosoftStorePlan {
            product: ProductId::ChatGpt,
            architecture: Architecture::X64,
            store_id: trust.store_id.clone().unwrap(),
        };
        let detection = Detection::absent("proof");
        let cancel = AtomicBool::new(false);
        let on_update = |_update: OperationUpdate| {};
        let request = StoreInstallRequest {
            plan: &plan,
            trust,
            initial_detection: &detection,
            cancel: &cancel,
            on_update: &on_update,
        };
        let config = StoreRuntimeConfig::from_request(&request).unwrap();
        let winget = locate_winget_path(&config).unwrap().unwrap();
        assert!(winget.is_absolute());
        assert_eq!(
            winget.file_name().and_then(|name| name.to_str()),
            Some("winget.exe")
        );
        let client = safe_http_client().unwrap();
        let (_, source) = fetch_official_text(&client, &config.release_api, trust).unwrap();
        let release: GithubRelease = serde_json::from_str(&source).unwrap();
        let bundle_asset = select_release_asset(&release, &config.bundle_asset).unwrap();
        let dependencies_asset =
            select_release_asset(&release, &config.dependencies_asset).unwrap();
        let bundle_digest = parse_github_digest(bundle_asset.digest.as_deref()).unwrap();
        let dependencies_digest =
            parse_github_digest(dependencies_asset.digest.as_deref()).unwrap();
        let bundle_path = Path::new(&proof_root).join(&config.bundle_asset);
        let dependencies_path = Path::new(&proof_root).join(&config.dependencies_asset);
        assert_eq!(sha256_file(&bundle_path).unwrap(), bundle_digest);
        assert_eq!(
            sha256_file(&dependencies_path).unwrap(),
            dependencies_digest
        );

        let bundle = inspect_bundle(&bundle_path).unwrap();
        verify_authenticode_subject(
            &bundle_path,
            std::slice::from_ref(&config.app_installer_publisher),
        )
        .unwrap();
        assert_eq!(bundle.name, config.app_installer_identity);
        assert!(distinguished_name_eq(
            &bundle.publisher,
            &config.app_installer_publisher
        ));
        assert!(bundle.supports(Architecture::X64));
        let main_package =
            inspect_bundle_application_package(&bundle_path, &bundle, Architecture::X64).unwrap();

        let extracted = extract_dependency_archive(&dependencies_path, Architecture::X64).unwrap();
        assert!(!extracted.paths.is_empty());
        let mut dependency_packages = Vec::new();
        for path in &extracted.paths {
            let info = inspect_package(path).unwrap();
            verify_dependency(&info, path, &config).unwrap();
            dependency_packages.push((path.clone(), info));
        }
        validate_dependency_closure(&main_package.dependencies, &dependency_packages).unwrap();
    }
}
