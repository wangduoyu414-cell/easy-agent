#[cfg(any(target_os = "macos", test))]
#[cfg_attr(all(test, not(target_os = "macos")), allow(dead_code))]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_store;

use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::core::{
    Architecture, Detection, MicrosoftStorePlan, OperatingSystem, OperationUpdate, PackageKind,
    PlatformInfo, ProductId, StableFileIdentity, TrustEntry,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerification {
    pub signer_subject: Option<String>,
    pub product_identity: String,
    pub version: Option<String>,
    pub architecture: Option<Architecture>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerExecution {
    pub exit_code: i32,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCommand {
    pub program: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
}

pub struct VerifiedInstallRequest<'a> {
    pub private_root: &'a Path,
    pub path: &'a Path,
    pub verified_identity: &'a StableFileIdentity,
    pub expected_sha256: Option<&'a str>,
    pub kind: PackageKind,
    pub trust: &'a TrustEntry,
    pub expected_architecture: Architecture,
    pub detached_signature: Option<&'a str>,
}

pub struct StoreInstallRequest<'a> {
    pub plan: &'a MicrosoftStorePlan,
    pub trust: &'a TrustEntry,
    pub initial_detection: &'a Detection,
    pub cancel: &'a AtomicBool,
    pub on_update: &'a dyn Fn(OperationUpdate),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreInstallError {
    Cancelled(String),
    ResultUnknown(String),
    Failed(String),
}

pub fn current_platform() -> PlatformInfo {
    let os = if cfg!(windows) {
        OperatingSystem::Windows
    } else if cfg!(target_os = "macos") {
        OperatingSystem::MacOs
    } else {
        OperatingSystem::Unsupported
    };
    #[cfg(target_os = "macos")]
    let architecture = macos::hardware_architecture();
    #[cfg(not(target_os = "macos"))]
    let architecture = if cfg!(target_arch = "x86_64") {
        Architecture::X64
    } else if cfg!(target_arch = "aarch64") {
        Architecture::Arm64
    } else {
        Architecture::Unsupported
    };
    #[cfg(target_os = "macos")]
    let os_version = macos::operating_system_version();
    #[cfg(not(target_os = "macos"))]
    let os_version = None;
    PlatformInfo {
        os,
        architecture,
        os_version: os_version.clone(),
        description: match os_version {
            Some(version) => format!("{} {} / {}", os.key(), version, architecture.key()),
            None => format!("{} / {}", os.key(), architecture.key()),
        },
    }
}

pub fn detect_product(product: ProductId, trust: Option<&TrustEntry>) -> Detection {
    #[cfg(windows)]
    {
        windows::detect_product(product, trust).unwrap_or_else(|error| Detection {
            installed: false,
            version: None,
            managed: false,
            management_known: false,
            package_identity: None,
            package_family: None,
            publisher: None,
            architecture: None,
            evidence: format!("检测失败：{error}"),
        })
    }
    #[cfg(target_os = "macos")]
    {
        macos::detect_product(product, trust).unwrap_or_else(|error| Detection {
            installed: false,
            version: None,
            managed: false,
            management_known: false,
            package_identity: None,
            package_family: None,
            publisher: None,
            architecture: None,
            evidence: format!("检测失败：{error}"),
        })
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = trust;
        Detection::absent(format!("{} 检测尚未在此平台实现", product.display_name()))
    }
}

#[cfg(target_os = "macos")]
pub fn preflight_direct_install(
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<(), String> {
    macos::preflight_direct_install(trust, expected_architecture)
}

#[cfg(not(target_os = "macos"))]
pub fn preflight_direct_install(
    _trust: &TrustEntry,
    _expected_architecture: Architecture,
) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub fn verify_artifact(
    path: &Path,
    kind: PackageKind,
    trust: &TrustEntry,
    expected_architecture: Architecture,
    updater_signature_verified: bool,
) -> Result<ArtifactVerification, String> {
    windows::verify_artifact(
        path,
        kind,
        trust,
        expected_architecture,
        updater_signature_verified,
    )
}

#[cfg(target_os = "macos")]
pub fn verify_artifact(
    path: &Path,
    kind: PackageKind,
    trust: &TrustEntry,
    expected_architecture: Architecture,
    updater_signature_verified: bool,
) -> Result<ArtifactVerification, String> {
    macos::verify_artifact(
        path,
        kind,
        trust,
        expected_architecture,
        updater_signature_verified,
    )
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn verify_artifact(
    _path: &Path,
    _kind: PackageKind,
    _trust: &TrustEntry,
    _expected_architecture: Architecture,
    _updater_signature_verified: bool,
) -> Result<ArtifactVerification, String> {
    Err("artifact verification is not implemented on this platform".into())
}

#[cfg(windows)]
pub fn plan_install_command(path: &Path, kind: PackageKind) -> Result<PlannedCommand, String> {
    windows::plan_install_command(path, kind)
}

#[cfg(target_os = "macos")]
pub fn plan_install_command(path: &Path, kind: PackageKind) -> Result<PlannedCommand, String> {
    macos::plan_install_command(path, kind)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn plan_install_command(_path: &Path, _kind: PackageKind) -> Result<PlannedCommand, String> {
    Err("installation is not implemented on this platform".into())
}

#[cfg(windows)]
pub fn execute_verified_installer(
    request: &VerifiedInstallRequest<'_>,
) -> Result<InstallerExecution, String> {
    windows::execute_verified_installer(request)
}

#[cfg(target_os = "macos")]
pub fn execute_verified_installer(
    request: &VerifiedInstallRequest<'_>,
) -> Result<InstallerExecution, String> {
    macos::execute_verified_installer(request)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn execute_verified_installer(
    _request: &VerifiedInstallRequest<'_>,
) -> Result<InstallerExecution, String> {
    Err("installation is not implemented on this platform".into())
}

#[cfg(windows)]
pub fn execute_microsoft_store_install(
    request: &StoreInstallRequest<'_>,
) -> Result<String, StoreInstallError> {
    windows_store::execute_microsoft_store_install(request)
}

#[cfg(not(windows))]
pub fn execute_microsoft_store_install(
    _request: &StoreInstallRequest<'_>,
) -> Result<String, StoreInstallError> {
    Err(StoreInstallError::Failed(
        "Microsoft Store installation is only available on Windows".into(),
    ))
}
