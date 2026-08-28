use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;
use url::Url;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, GetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use zip::ZipArchive;

use crate::core::{
    Architecture, Detection, DownloadControl, DownloadRequest, OperationState, OperationUpdate,
    ProductId, StableFileIdentity, TrustEntry, UrlRule, download_to_private_staging_controlled,
    verify_staged_identity, version_is_older,
};

use super::windows_store_contract::{
    WebInstallerExpectation, extract_ms_store_tag, validate_web_installer_tag,
};
use super::{StoreInstallError, StoreInstallRequest};

const WEB_INSTALLER_FILE_NAME: &str = "ChatGPT-Web-Installer.exe";
const OFFLINE_LICENSE_FILE_NAME: &str = "ChatGPT-License.xml";
const MAX_LICENSE_BYTES: u64 = 128 * 1024;
const MAX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const WEB_INSTALLER_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const OFFLINE_DEPLOYMENT_TIMEOUT: Duration = Duration::from_secs(45 * 60);

const WEB_INSTALLER_VERIFY_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$path = $env:EASY_AGENT_ARTIFACT
$signature = Get-AuthenticodeSignature -LiteralPath $path
$info = (Get-Item -LiteralPath $path).VersionInfo
[pscustomobject]@{
  signature_status = [string]$signature.Status
  signer_subject = if ($signature.SignerCertificate) { [string]$signature.SignerCertificate.Subject } else { $null }
  product = [string]$info.ProductName
  description = [string]$info.FileDescription
} | ConvertTo-Json -Compress
"#;

const OFFLINE_DEPLOY_SCRIPT_TEMPLATE: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Import-Module Dism -ErrorAction Stop
$packagePath = '__PACKAGE_PATH__'
$licensePath = '__LICENSE_PATH__'
try {
  Add-AppxProvisionedPackage -Online -PackagePath $packagePath -LicensePath $licensePath -ErrorAction Stop | Out-Null
} catch {
  $message = [string]$_.Exception.Message
  if ($_.ErrorDetails -and -not [string]::IsNullOrWhiteSpace([string]$_.ErrorDetails.Message)) {
    $message = $message + ' ' + [string]$_.ErrorDetails.Message
  }
  $hresult = 'UNKNOWN'
  if ($_.Exception) {
    try {
      $unsigned = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int32]$_.Exception.HResult), 0)
      $hresult = ('0x{0:X8}' -f $unsigned)
    } catch {}
  }
  $encoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($message))
  [Console]::Error.WriteLine("EASY_AGENT_OFFLINE_ERROR HRESULT=$hresult MESSAGE_B64=$encoded")
  exit 1
}
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
    fn fallback(message: impl Into<String>) -> Self {
        Self {
            class: FailureClass::FallbackAllowed,
            message: message.into(),
        }
    }

    fn hard(message: impl Into<String>) -> Self {
        Self {
            class: FailureClass::Hard,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryOutcome {
    Applied,
    ResultUnknown,
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
    fn run_primary(&mut self) -> Result<PrimaryOutcome, StoreFailure>;
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
    if initial.is_failed() {
        return Err(StoreFailure::hard(
            "无法确认当前 ChatGPT 安装状态，请先刷新状态后再试",
        ));
    }
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
        return Err(StoreFailure::cancelled("操作已在下载安装器前取消"));
    }

    let primary_outcome = match backend.run_primary() {
        Ok(outcome) => outcome,
        Err(error) if error.class == FailureClass::FallbackAllowed => {
            if backend.cancelled() {
                return Err(StoreFailure::cancelled("操作已取消，尚未准备完整安装包"));
            }
            backend.run_fallback()?;
            PrimaryOutcome::Applied
        }
        Err(error) => return Err(error),
    };

    let detected = backend.detect_target()?;
    match validate_store_detection(&detected, expectation) {
        Ok(()) => {}
        Err(error) if primary_outcome == PrimaryOutcome::ResultUnknown => {
            return Err(StoreFailure::result_unknown(format!(
                "微软安装仍可能在后台继续；当前尚未复检到 ChatGPT。请稍后点击“刷新状态”。{}",
                error.message
            )));
        }
        Err(error) => return Err(error),
    }
    if let (Some(before), Some(after)) = (initial.version.as_deref(), detected.version.as_deref())
        && version_is_older(after, before)
    {
        return Err(StoreFailure::hard(format!(
            "安装后版本 {after} 低于原版本 {before}，拒绝认定成功"
        )));
    }

    Ok(format!(
        "复检成功：{}",
        detected.version.as_deref().unwrap_or("版本未知")
    ))
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
    web_installer_url: Url,
    msix_url: Url,
    license_url: Url,
    web_installer_signer: String,
    package_identity: String,
    package_family: String,
    publisher: String,
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
        if store_id != request.plan.store_id || !valid_store_id(&store_id) {
            return Err(StoreFailure::hard("Store ID 与安装计划不一致或格式非法"));
        }
        if trust.entry_urls.len() != 3 {
            return Err(StoreFailure::hard(
                "ChatGPT Windows 必须固定轻量安装器、完整包和离线许可证三个入口",
            ));
        }
        let web_installer_url = parse_fixed_entry(&trust.entry_urls[0], trust, "微软安装器")?;
        let msix_url = parse_fixed_entry(&trust.entry_urls[1], trust, "ChatGPT MSIX")?;
        let license_url = parse_fixed_entry(&trust.entry_urls[2], trust, "离线许可证")?;
        let expected_msix_name = match request.plan.architecture {
            Architecture::X64 => "ChatGPT-x64.msix",
            Architecture::Arm64 => "ChatGPT-arm64.msix",
            Architecture::Unsupported => return Err(StoreFailure::hard("不支持的 Windows 架构")),
        };
        if web_installer_url.host_str() != Some("get.microsoft.com")
            || web_installer_url.path() != format!("/installer/download/{store_id}")
            || msix_url.host_str() != Some("persistent.oaistatic.com")
            || msix_url.path() != format!("/codex-app-prod/{expected_msix_name}")
            || license_url.host_str() != Some("persistent.oaistatic.com")
            || license_url.path() != "/codex-app-prod/ChatGPT-License.xml"
        {
            return Err(StoreFailure::hard(
                "ChatGPT Windows 官方入口与 Store ID 或目标架构不一致",
            ));
        }
        Ok(Self {
            store_id,
            architecture: request.plan.architecture,
            web_installer_url,
            msix_url,
            license_url,
            web_installer_signer: required(
                &trust.web_installer_signer_subject,
                "web_installer_signer_subject",
            )?,
            package_identity: required(&trust.package_identity, "package_identity")?,
            package_family: required(&trust.package_family, "package_family")?,
            publisher: required(&trust.msix_publisher, "msix_publisher")?,
        })
    }
}

fn valid_store_id(value: &str) -> bool {
    value.len() == 12
        && value
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
}

fn parse_fixed_entry(value: &str, trust: &TrustEntry, label: &str) -> Result<Url, StoreFailure> {
    let url = Url::parse(value)
        .map_err(|error| StoreFailure::hard(format!("{label} URL 非法：{error}")))?;
    crate::core::ensure_allowed_url(&url, trust)
        .map_err(|error| StoreFailure::hard(format!("{label} URL 不在固定白名单：{error}")))?;
    Ok(url)
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

    fn download(
        &self,
        url: Url,
        file_name: &str,
        rules: &[UrlRule],
        label: &str,
    ) -> Result<crate::core::DownloadResult, StoreFailure> {
        let progress = |received: u64, total: Option<u64>| {
            let detail = total
                .filter(|total| *total > 0)
                .map(|total| format!("{:.1}%", received as f64 * 100.0 / total as f64))
                .unwrap_or_else(|| format!("{:.1} MiB", received as f64 / 1_048_576.0));
            self.emit(OperationState::Downloading, format!("{label}：{detail}"));
        };
        let cancelled = || self.request.cancel.load(Ordering::Relaxed);
        download_to_private_staging_controlled(
            &DownloadRequest {
                url,
                file_name: file_name.into(),
                url_rules: rules,
                expected_size: None,
            },
            &DownloadControl {
                is_cancelled: &cancelled,
                on_progress: &progress,
            },
        )
        .map_err(|error| match error {
            crate::core::DownloadError::Cancelled => StoreFailure::cancelled("下载已取消"),
            _ => StoreFailure::fallback(format!("{label}失败：{error}")),
        })
    }

    fn run_web_installer(&self) -> Result<PrimaryOutcome, StoreFailure> {
        self.emit(OperationState::Downloading, "正在下载微软 ChatGPT 安装器");
        let download = self.download(
            self.config.web_installer_url.clone(),
            WEB_INSTALLER_FILE_NAME,
            &self.request.trust.url_rules,
            "下载微软安装器",
        )?;
        self.emit(
            OperationState::Verifying,
            "正在验证微软签名和 ChatGPT 产品绑定",
        );
        verify_web_installer(&download, &self.config)?;
        verify_staged_identity(
            download.private_root.path(),
            &download.staged_path,
            &download.identity,
            Some(&download.identity.sha256),
        )
        .map_err(|error| StoreFailure::hard(format!("安装器在启动前发生变化：{error}")))?;
        self.emit(
            OperationState::AwaitingUserInstall,
            "已启动微软安装器，请在弹出的窗口中完成安装",
        );
        match run_visible_process(
            &download.staged_path,
            &[],
            WEB_INSTALLER_TIMEOUT,
            self.request.cancel,
        )? {
            VisibleProcessOutcome::Exited(0) => Ok(PrimaryOutcome::Applied),
            VisibleProcessOutcome::Exited(code) if is_primary_fallback_exit_code(code) => Err(
                StoreFailure::fallback(format!("微软安装器当前不可用，退出码 0x{code:08X}")),
            ),
            VisibleProcessOutcome::Exited(code) => Err(StoreFailure::hard(format!(
                "微软安装器未完成安装，退出码 0x{code:08X}"
            ))),
            VisibleProcessOutcome::TimedOut => Ok(PrimaryOutcome::ResultUnknown),
        }
    }

    fn run_offline_fallback(&self) -> Result<(), StoreFailure> {
        self.emit(
            OperationState::Downloading,
            "普通安装不可用，正在准备完整安装包",
        );
        let msix_name = match self.config.architecture {
            Architecture::X64 => "ChatGPT-x64.msix",
            Architecture::Arm64 => "ChatGPT-arm64.msix",
            Architecture::Unsupported => return Err(StoreFailure::hard("不支持的 Windows 架构")),
        };
        let msix = self.download(
            self.config.msix_url.clone(),
            msix_name,
            &self.request.trust.url_rules,
            "下载完整安装包",
        )?;
        let license = self.download(
            self.config.license_url.clone(),
            OFFLINE_LICENSE_FILE_NAME,
            &self.request.trust.url_rules,
            "下载离线许可证",
        )?;
        self.emit(
            OperationState::Verifying,
            "正在验证完整安装包、离线许可证、版本和架构",
        );
        let package = inspect_msix(&msix.staged_path)?;
        verify_msix_authenticode(&msix.staged_path)?;
        validate_msix_contract(&package, &self.config)?;
        validate_offline_license(&license.staged_path, &self.config)?;
        reverify_download(&msix)?;
        reverify_download(&license)?;

        let private_root = msix
            .private_root
            .path()
            .canonicalize()
            .map_err(|error| StoreFailure::hard(format!("无法核对完整包暂存目录：{error}")))?;
        let package_path = msix
            .staged_path
            .canonicalize()
            .map_err(|error| StoreFailure::hard(format!("无法核对完整包路径：{error}")))?;
        let license_path = license
            .staged_path
            .canonicalize()
            .map_err(|error| StoreFailure::hard(format!("无法核对许可证路径：{error}")))?;
        if !package_path.starts_with(&private_root) {
            return Err(StoreFailure::hard("完整包逃逸出私有暂存目录"));
        }
        let license_root = license
            .private_root
            .path()
            .canonicalize()
            .map_err(|error| StoreFailure::hard(format!("无法核对许可证暂存目录：{error}")))?;
        if !license_path.starts_with(&license_root) {
            return Err(StoreFailure::hard("许可证逃逸出私有暂存目录"));
        }

        let script_path = package_path
            .parent()
            .ok_or_else(|| StoreFailure::hard("完整包没有父目录"))?
            .join("deploy-chatgpt.ps1");
        let deploy_script = OFFLINE_DEPLOY_SCRIPT_TEMPLATE
            .replace(
                "__PACKAGE_PATH__",
                &powershell_literal_body(&path_string(&package_path, "完整包")?),
            )
            .replace(
                "__LICENSE_PATH__",
                &powershell_literal_body(&path_string(&license_path, "许可证")?),
            );
        fs::write(&script_path, deploy_script)
            .map_err(|error| StoreFailure::hard(format!("无法准备管理员部署脚本：{error}")))?;
        let powershell = super::windows::trusted_powershell_program()
            .map_err(|error| StoreFailure::hard(format!("无法定位系统 PowerShell：{error}")))?;
        let arguments = vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            quote_windows_argument(&script_path)?,
        ];
        self.emit(
            OperationState::AwaitingUserInstall,
            "需要管理员权限完成安装，请确认 Windows 提示",
        );
        match shell_execute_wait(
            Path::new(&powershell),
            &arguments,
            OFFLINE_DEPLOYMENT_TIMEOUT,
        )? {
            ElevatedProcessOutcome::Exited(0) => Ok(()),
            ElevatedProcessOutcome::Exited(ERROR_CANCELLED) => Err(StoreFailure::cancelled(
                "已取消管理员权限请求，未启动完整包部署",
            )),
            ElevatedProcessOutcome::Exited(code) => Err(StoreFailure::hard(format!(
                "完整安装包部署失败，退出码 0x{code:08X}"
            ))),
            ElevatedProcessOutcome::TimedOut => Err(StoreFailure::result_unknown(
                "Windows 仍可能在后台部署完整包；请稍后点击“刷新状态”复检",
            )),
        }
    }
}

impl StoreBackend for WindowsStoreBackend<'_> {
    fn cancelled(&self) -> bool {
        self.request.cancel.load(Ordering::Relaxed)
    }

    fn run_primary(&mut self) -> Result<PrimaryOutcome, StoreFailure> {
        self.run_web_installer()
    }

    fn run_fallback(&mut self) -> Result<(), StoreFailure> {
        self.run_offline_fallback()
    }

    fn detect_target(&mut self) -> Result<Detection, StoreFailure> {
        self.emit(
            OperationState::Postchecking,
            "正在核对 ChatGPT 版本、身份和架构",
        );
        super::windows::detect_product(self.request.plan.product, Some(self.request.trust))
            .map_err(|error| StoreFailure::hard(format!("ChatGPT 复检失败：{error}")))
    }
}

#[derive(Debug, Deserialize)]
struct WebInstallerVerification {
    signature_status: String,
    signer_subject: Option<String>,
    product: String,
    description: String,
}

fn verify_web_installer(
    download: &crate::core::DownloadResult,
    config: &StoreRuntimeConfig,
) -> Result<(), StoreFailure> {
    let output = run_powershell_json(
        WEB_INSTALLER_VERIFY_SCRIPT,
        &[(
            "EASY_AGENT_ARTIFACT",
            path_string(&download.staged_path, "微软安装器")?,
        )],
        Duration::from_secs(45),
    )?;
    let parsed: WebInstallerVerification = serde_json::from_slice(&output)
        .map_err(|error| StoreFailure::hard(format!("微软安装器验证结果无效：{error}")))?;
    if parsed.signature_status != "Valid" {
        return Err(StoreFailure::hard(format!(
            "微软安装器签名无效：{}",
            parsed.signature_status
        )));
    }
    let subject = parsed
        .signer_subject
        .as_deref()
        .ok_or_else(|| StoreFailure::hard("微软安装器没有签名证书"))?;
    if !certificate_subject_contains(subject, &config.web_installer_signer) {
        return Err(StoreFailure::hard(format!(
            "微软安装器签名者不匹配：{subject}"
        )));
    }
    let product_text = format!("{} {}", parsed.product, parsed.description).to_ascii_lowercase();
    if !product_text.contains("app installer") && !product_text.contains("installer") {
        return Err(StoreFailure::hard("下载内容不是 Microsoft 应用安装器"));
    }
    let tag = read_ms_store_tag(&download.staged_path)?;
    validate_web_installer_tag(
        &tag,
        &WebInstallerExpectation {
            store_id: &config.store_id,
            package_family: &config.package_family,
        },
    )
    .map_err(StoreFailure::hard)?;
    Ok(())
}

fn read_ms_store_tag(path: &Path) -> Result<Vec<u8>, StoreFailure> {
    let mut file = File::open(path)
        .map_err(|error| StoreFailure::hard(format!("无法打开微软安装器：{error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| StoreFailure::hard(format!("无法读取微软安装器大小：{error}")))?;
    if metadata.len() < 256 || metadata.len() > 64 * 1024 * 1024 {
        return Err(StoreFailure::hard("微软安装器大小异常"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| StoreFailure::hard(format!("无法读取微软安装器：{error}")))?;
    extract_ms_store_tag(&bytes).map_err(StoreFailure::hard)
}

#[derive(Debug)]
struct MsixContract {
    identity: String,
    publisher: String,
    version: String,
    architecture: Architecture,
    dependencies: Vec<String>,
}

fn inspect_msix(path: &Path) -> Result<MsixContract, StoreFailure> {
    let file = File::open(path)
        .map_err(|error| StoreFailure::hard(format!("无法打开 ChatGPT MSIX：{error}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| StoreFailure::hard(format!("ChatGPT MSIX 不是有效包：{error}")))?;
    let mut manifest = archive
        .by_name("AppxManifest.xml")
        .map_err(|error| StoreFailure::hard(format!("ChatGPT MSIX 缺少清单：{error}")))?;
    if manifest.size() > MAX_MANIFEST_BYTES {
        return Err(StoreFailure::hard("ChatGPT MSIX 清单超过安全上限"));
    }
    let mut xml = String::new();
    manifest
        .read_to_string(&mut xml)
        .map_err(|error| StoreFailure::hard(format!("无法读取 ChatGPT MSIX 清单：{error}")))?;
    inspect_msix_xml(&xml)
}

fn inspect_msix_xml(xml: &str) -> Result<MsixContract, StoreFailure> {
    let document = roxmltree::Document::parse(xml)
        .map_err(|error| StoreFailure::hard(format!("ChatGPT MSIX 清单无效：{error}")))?;
    let identity = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "Identity")
        .ok_or_else(|| StoreFailure::hard("ChatGPT MSIX 清单缺少 Identity"))?;
    let attribute = |name: &str| {
        identity
            .attribute(name)
            .map(ToOwned::to_owned)
            .ok_or_else(|| StoreFailure::hard(format!("ChatGPT MSIX Identity 缺少 {name}")))
    };
    let architecture = match attribute("ProcessorArchitecture")?
        .to_ascii_lowercase()
        .as_str()
    {
        "x64" => Architecture::X64,
        "arm64" => Architecture::Arm64,
        value => return Err(StoreFailure::hard(format!("不支持的 MSIX 架构：{value}"))),
    };
    let dependencies = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "PackageDependency")
        .filter_map(|node| node.attribute("Name").map(ToOwned::to_owned))
        .collect();
    Ok(MsixContract {
        identity: attribute("Name")?,
        publisher: attribute("Publisher")?,
        version: attribute("Version")?,
        architecture,
        dependencies,
    })
}

fn validate_msix_contract(
    package: &MsixContract,
    config: &StoreRuntimeConfig,
) -> Result<(), StoreFailure> {
    if package.identity != config.package_identity {
        return Err(StoreFailure::hard(format!(
            "完整包 Identity 不匹配：{}",
            package.identity
        )));
    }
    if !distinguished_name_eq(&package.publisher, &config.publisher) {
        return Err(StoreFailure::hard(format!(
            "完整包 Publisher 不匹配：{}",
            package.publisher
        )));
    }
    if package.architecture != config.architecture {
        return Err(StoreFailure::hard(format!(
            "完整包架构不匹配：期望 {:?}，实际 {:?}",
            config.architecture, package.architecture
        )));
    }
    if parse_appx_version(&package.version).is_none() {
        return Err(StoreFailure::hard(format!(
            "完整包版本格式非法：{}",
            package.version
        )));
    }
    if !package.dependencies.is_empty() {
        return Err(StoreFailure::hard(format!(
            "完整包新增了未固定的框架依赖：{}",
            package.dependencies.join(", ")
        )));
    }
    Ok(())
}

fn parse_appx_version(value: &str) -> Option<[u16; 4]> {
    let parts = value
        .split('.')
        .map(str::parse::<u16>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    parts.try_into().ok()
}

fn verify_msix_authenticode(path: &Path) -> Result<(), StoreFailure> {
    let script = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$signature = Get-AuthenticodeSignature -LiteralPath $env:EASY_AGENT_ARTIFACT
[pscustomobject]@{
  status = [string]$signature.Status
  signer_subject = if ($signature.SignerCertificate) { [string]$signature.SignerCertificate.Subject } else { $null }
} | ConvertTo-Json -Compress
"#;
    let output = run_powershell_json(
        script,
        &[("EASY_AGENT_ARTIFACT", path_string(path, "完整包")?)],
        Duration::from_secs(90),
    )?;
    let parsed: SignatureOutput = serde_json::from_slice(&output)
        .map_err(|error| StoreFailure::hard(format!("完整包签名结果无效：{error}")))?;
    if parsed.status != "Valid" || parsed.signer_subject.as_deref().is_none_or(str::is_empty) {
        return Err(StoreFailure::hard(format!(
            "完整包签名无效：{}",
            parsed.status
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct SignatureOutput {
    status: String,
    signer_subject: Option<String>,
}

fn validate_offline_license(path: &Path, config: &StoreRuntimeConfig) -> Result<(), StoreFailure> {
    let metadata = path
        .metadata()
        .map_err(|error| StoreFailure::hard(format!("无法读取离线许可证：{error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_LICENSE_BYTES {
        return Err(StoreFailure::hard("离线许可证大小异常"));
    }
    let source = fs::read_to_string(path)
        .map_err(|error| StoreFailure::hard(format!("无法读取离线许可证：{error}")))?;
    let document = roxmltree::Document::parse(&source)
        .map_err(|error| StoreFailure::hard(format!("离线许可证 XML 无效：{error}")))?;
    let text = |name: &str| {
        document
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == name)
            .and_then(|node| node.text())
            .map(str::trim)
    };
    if text("ProductID") != Some(config.store_id.as_str())
        || text("PFM") != Some(config.package_family.to_ascii_lowercase().as_str())
        || text("LeaseRequired") != Some("False")
    {
        return Err(StoreFailure::hard(
            "离线许可证没有绑定固定 ChatGPT 产品或声明需要租约",
        ));
    }
    let license_info = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "LicenseInfo")
        .ok_or_else(|| StoreFailure::hard("离线许可证缺少 LicenseInfo"))?;
    if license_info.attribute("LicenseUsage") != Some("Offline")
        || license_info.attribute("Type") != Some("Full")
    {
        return Err(StoreFailure::hard("离线许可证不是 Full/Offline 合同"));
    }
    if document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "Signature")
        .is_none()
    {
        return Err(StoreFailure::hard("离线许可证缺少 XML 签名"));
    }
    Ok(())
}

fn reverify_download(
    download: &crate::core::DownloadResult,
) -> Result<StableFileIdentity, StoreFailure> {
    verify_staged_identity(
        download.private_root.path(),
        &download.staged_path,
        &download.identity,
        Some(&download.identity.sha256),
    )
    .map_err(|error| StoreFailure::hard(format!("已验证文件在部署前发生变化：{error}")))
}

fn run_powershell_json(
    script: &str,
    environment: &[(&str, String)],
    timeout: Duration,
) -> Result<Vec<u8>, StoreFailure> {
    let powershell = super::windows::trusted_powershell_program()
        .map_err(|error| StoreFailure::hard(format!("无法定位系统 PowerShell：{error}")))?;
    let encoded = encode_powershell(script);
    let mut command = Command::new(powershell);
    super::windows::hide_console_window(&mut command);
    command
        .env_remove("PSModulePath")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-OutputFormat",
            "Text",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in environment {
        command.env(key, value);
    }
    let mut child = command
        .spawn()
        .map_err(|error| StoreFailure::hard(format!("无法启动 Windows 验证器：{error}")))?;
    let start = Instant::now();
    loop {
        match child
            .try_wait()
            .map_err(|error| StoreFailure::hard(format!("无法读取验证器状态：{error}")))?
        {
            Some(_) => break,
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                return Err(StoreFailure::hard("Windows 验证器超时"));
            }
            None => thread::sleep(Duration::from_millis(100)),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| StoreFailure::hard(format!("无法读取验证器输出：{error}")))?;
    if !output.status.success() {
        return Err(StoreFailure::hard(format!(
            "Windows 验证器失败：{}",
            summarize_bytes(&output.stderr)
        )));
    }
    Ok(output.stdout)
}

enum VisibleProcessOutcome {
    Exited(u32),
    TimedOut,
}

fn run_visible_process(
    program: &Path,
    arguments: &[String],
    timeout: Duration,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<VisibleProcessOutcome, StoreFailure> {
    let mut command = Command::new(program);
    command.args(arguments);
    let mut child = command
        .spawn()
        .map_err(|error| StoreFailure::fallback(format!("无法启动微软安装器：{error}")))?;
    let start = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| StoreFailure::hard(format!("无法读取微软安装器状态：{error}")))?
        {
            return Ok(VisibleProcessOutcome::Exited(
                status.code().unwrap_or(-1) as u32
            ));
        }
        if start.elapsed() >= timeout {
            return Ok(VisibleProcessOutcome::TimedOut);
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(StoreFailure::result_unknown(
                "微软安装器已经启动，取消按钮不会强行终止系统安装；请在微软窗口中取消或稍后刷新状态",
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn is_primary_fallback_exit_code(code: u32) -> bool {
    matches!(
        code,
        0x0000_064C
            | 0x8007_2EE2
            | 0x8007_2EFD
            | 0x8007_2EFE
            | 0x8007_2F8F
            | 0x8024_400C
            | 0x8024_4040
            | 0x8024_502C
    )
}

enum ElevatedProcessOutcome {
    Exited(u32),
    TimedOut,
}

fn shell_execute_wait(
    program: &Path,
    arguments: &[String],
    timeout: Duration,
) -> Result<ElevatedProcessOutcome, StoreFailure> {
    let verb = wide("runas");
    let file = wide_os(program.as_os_str());
    let params = wide(&arguments.join(" "));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: params.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: all UTF-16 buffers live for the call, the structure size and flags are valid, and
    // the returned process handle is closed on every successful path below.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = unsafe { GetLastError() };
        return if error == ERROR_CANCELLED {
            Err(StoreFailure::cancelled(
                "已取消管理员权限请求，未启动完整包部署",
            ))
        } else {
            Err(StoreFailure::hard(format!(
                "无法启动管理员部署，Windows 错误 {error}"
            )))
        };
    }
    if info.hProcess.is_null() {
        return Err(StoreFailure::hard("管理员部署没有返回进程句柄"));
    }
    let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
    // SAFETY: hProcess is a valid handle returned by ShellExecuteExW.
    let wait = unsafe { WaitForSingleObject(info.hProcess, milliseconds) };
    if wait == WAIT_TIMEOUT {
        unsafe { CloseHandle(info.hProcess) };
        return Ok(ElevatedProcessOutcome::TimedOut);
    }
    if wait != WAIT_OBJECT_0 {
        unsafe { CloseHandle(info.hProcess) };
        return Err(StoreFailure::hard(format!(
            "等待管理员部署失败，Windows 状态 {wait}"
        )));
    }
    let mut exit_code = 0_u32;
    // SAFETY: hProcess is signaled and exit_code points to writable storage.
    let success = unsafe { GetExitCodeProcess(info.hProcess, &mut exit_code) };
    unsafe { CloseHandle(info.hProcess) };
    if success == 0 {
        return Err(StoreFailure::hard("无法读取管理员部署退出码"));
    }
    Ok(ElevatedProcessOutcome::Exited(exit_code))
}

fn quote_windows_argument(path: &Path) -> Result<String, StoreFailure> {
    let value = path_string(path, "路径")?;
    if value.contains('"') || value.contains('\0') {
        return Err(StoreFailure::hard("Windows 路径包含非法字符"));
    }
    Ok(format!("\"{value}\""))
}

fn path_string(path: &Path, label: &str) -> Result<String, StoreFailure> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| StoreFailure::hard(format!("{label}路径不是有效 Unicode")))
}

fn powershell_literal_body(value: &str) -> String {
    value.replace('\'', "''")
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

fn wide_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn summarize_bytes(bytes: &[u8]) -> String {
    let value = String::from_utf8_lossy(bytes);
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(2048).collect()
}

fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn certificate_subject_contains(subject: &str, expected: &str) -> bool {
    let normalize = |value: &str| {
        value
            .to_ascii_lowercase()
            .chars()
            .filter(|character| !character.is_ascii_whitespace() && *character != '"')
            .collect::<String>()
    };
    normalize(subject).contains(&normalize(expected))
}

fn distinguished_name_eq(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        let mut parts = value
            .split(',')
            .map(|part| part.trim().trim_matches('"').to_ascii_lowercase())
            .collect::<Vec<_>>();
        parts.sort();
        parts.join(",")
    };
    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Primary,
        Fallback,
        Detect,
    }

    struct FakeBackend {
        calls: Vec<Call>,
        primary: Result<PrimaryOutcome, StoreFailure>,
        fallback: Result<(), StoreFailure>,
        detection: Result<Detection, StoreFailure>,
        cancelled: bool,
    }

    impl StoreBackend for FakeBackend {
        fn cancelled(&self) -> bool {
            self.cancelled
        }

        fn run_primary(&mut self) -> Result<PrimaryOutcome, StoreFailure> {
            self.calls.push(Call::Primary);
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

    fn backend() -> FakeBackend {
        FakeBackend {
            calls: Vec::new(),
            primary: Ok(PrimaryOutcome::Applied),
            fallback: Ok(()),
            detection: Ok(installed("26.803.10989.0")),
            cancelled: false,
        }
    }

    #[test]
    fn primary_success_is_postchecked() {
        let mut fake = backend();
        let message = run_store_workflow(&mut fake, &absent(), &expectation()).unwrap();
        assert!(message.contains("复检成功"));
        assert_eq!(fake.calls, [Call::Primary, Call::Detect]);
    }

    #[test]
    fn eligible_primary_failure_uses_one_offline_fallback() {
        let mut fake = backend();
        fake.primary = Err(StoreFailure::fallback("transport unavailable"));
        run_store_workflow(&mut fake, &absent(), &expectation()).unwrap();
        assert_eq!(fake.calls, [Call::Primary, Call::Fallback, Call::Detect]);
    }

    #[test]
    fn cancellation_and_hard_failure_never_use_fallback() {
        for failure in [
            StoreFailure::cancelled("cancelled"),
            StoreFailure::hard("identity failure"),
        ] {
            let mut fake = backend();
            fake.primary = Err(failure);
            assert!(run_store_workflow(&mut fake, &absent(), &expectation()).is_err());
            assert_eq!(fake.calls, [Call::Primary]);
        }
    }

    #[test]
    fn primary_timeout_with_no_detection_is_result_unknown() {
        let mut fake = backend();
        fake.primary = Ok(PrimaryOutcome::ResultUnknown);
        fake.detection = Ok(absent());
        let error = run_store_workflow(&mut fake, &absent(), &expectation()).unwrap_err();
        assert_eq!(error.class, FailureClass::ResultUnknown);
        assert_eq!(fake.calls, [Call::Primary, Call::Detect]);
    }

    #[test]
    fn managed_install_is_rejected_before_external_work() {
        let mut current = installed("26.700.0.0");
        current.managed = true;
        let mut fake = backend();
        assert!(run_store_workflow(&mut fake, &current, &expectation()).is_err());
        assert!(fake.calls.is_empty());
    }

    #[test]
    fn postcheck_rejects_identity_mismatch_and_downgrade() {
        let mut fake = backend();
        let mut wrong = installed("26.803.10989.0");
        wrong.package_family = Some("wrong".into());
        fake.detection = Ok(wrong);
        assert!(run_store_workflow(&mut fake, &absent(), &expectation()).is_err());

        let mut fake = backend();
        fake.detection = Ok(installed("26.700.0.0"));
        assert!(run_store_workflow(&mut fake, &installed("26.900.0.0"), &expectation()).is_err());
    }

    #[test]
    fn msix_contract_rejects_dependencies_and_wrong_architecture() {
        let xml = r#"
<Package xmlns:uap10="urn:test">
  <Identity Name="OpenAI.Codex" Publisher="CN=OpenAI" Version="1.2.3.4" ProcessorArchitecture="x64" />
  <Dependencies><uap10:PackageDependency Name="Microsoft.Framework" /></Dependencies>
</Package>
"#;
        let package = inspect_msix_xml(xml).unwrap();
        assert_eq!(package.architecture, Architecture::X64);
        assert_eq!(package.dependencies, ["Microsoft.Framework"]);
    }

    #[test]
    fn store_tag_reader_finds_the_fixed_product_id() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"header");
        bytes.extend_from_slice(&[
            0x06, 0x0b, 0x2b, 0x06, 0x01, 0x04, 0x01, 0xd6, 0x79, 0x02, 0x01, 0xce, 0x0f,
        ]);
        bytes.extend_from_slice(&[0x04, 0x82, 0x40, 0x00]);
        let mut tag = vec![0_u8; 16 * 1024];
        tag[..13].copy_from_slice(b"MSStoreTag001");
        bytes.extend_from_slice(&tag);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("installer.exe");
        fs::write(&path, bytes).unwrap();
        let tag = read_ms_store_tag(&path).unwrap();
        assert!(tag.starts_with(b"MSStoreTag001"));
    }

    #[test]
    fn fallback_exit_class_is_narrow() {
        assert!(is_primary_fallback_exit_code(0x0000_064C));
        assert!(is_primary_fallback_exit_code(0x8007_2EFD));
        assert!(!is_primary_fallback_exit_code(ERROR_CANCELLED));
        assert!(!is_primary_fallback_exit_code(0x800B_0109));
        assert!(!is_primary_fallback_exit_code(0x8007_3CF1));
    }

    #[test]
    fn store_id_and_version_parsers_fail_closed() {
        assert!(valid_store_id("9PLM9XGG6VKS"));
        assert!(!valid_store_id("bad"));
        assert_eq!(
            parse_appx_version("26.803.10989.0"),
            Some([26, 803, 10989, 0])
        );
        assert!(parse_appx_version("26.803.latest.0").is_none());
    }

    #[test]
    fn result_unknown_mapping_is_preserved() {
        let mapped = store_install_error(StoreFailure::result_unknown("still running"));
        assert!(
            matches!(mapped, StoreInstallError::ResultUnknown(message) if message == "still running")
        );
    }

    #[test]
    #[ignore = "downloads the current Microsoft web installer; read-only product-binding proof"]
    fn current_web_installer_matches_the_embedded_chatgpt_contract() {
        use std::sync::atomic::AtomicBool;

        use crate::core::{MicrosoftStorePlan, OperatingSystem, TrustRegistry};

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
        let initial = Detection::absent("proof");
        let cancel = AtomicBool::new(false);
        let request = StoreInstallRequest {
            plan: &plan,
            trust,
            initial_detection: &initial,
            cancel: &cancel,
            on_update: &|_| {},
        };
        let config = StoreRuntimeConfig::from_request(&request).unwrap();
        let download = download_to_private_staging_controlled(
            &DownloadRequest {
                url: config.web_installer_url.clone(),
                file_name: WEB_INSTALLER_FILE_NAME.into(),
                url_rules: &trust.url_rules,
                expected_size: None,
            },
            &DownloadControl {
                is_cancelled: &|| false,
                on_progress: &|_, _| {},
            },
        )
        .unwrap();
        let tag = read_ms_store_tag(&download.staged_path).unwrap();
        validate_web_installer_tag(
            &tag,
            &WebInstallerExpectation {
                store_id: &config.store_id,
                package_family: &config.package_family,
            },
        )
        .unwrap();
    }
}
