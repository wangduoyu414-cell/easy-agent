#[cfg(windows)]
mod windows;

use std::path::Path;

use crate::core::{
    Architecture, Detection, OperatingSystem, PackageKind, PlatformInfo, ProductId,
    StableFileIdentity, TrustEntry,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerification {
    pub signer_subject: Option<String>,
    pub product_identity: String,
    pub version: Option<String>,
    pub architecture: Option<Architecture>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCommand {
    pub program: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
}

pub fn current_platform() -> PlatformInfo {
    let os = if cfg!(windows) {
        OperatingSystem::Windows
    } else if cfg!(target_os = "macos") {
        OperatingSystem::MacOs
    } else {
        OperatingSystem::Unsupported
    };
    let architecture = if cfg!(target_arch = "x86_64") {
        Architecture::X64
    } else if cfg!(target_arch = "aarch64") {
        Architecture::Arm64
    } else {
        Architecture::Unsupported
    };
    PlatformInfo {
        os,
        architecture,
        description: format!("{} / {}", os.key(), architecture.key()),
    }
}

pub fn detect_product(product: ProductId) -> Detection {
    #[cfg(windows)]
    {
        windows::detect_product(product).unwrap_or_else(|error| Detection {
            installed: false,
            version: None,
            managed: false,
            management_known: false,
            evidence: format!("检测失败：{error}"),
        })
    }
    #[cfg(not(windows))]
    {
        Detection::absent(format!("{} 检测尚未在此平台实现", product.display_name()))
    }
}

#[cfg(windows)]
pub fn verify_artifact(
    path: &Path,
    kind: PackageKind,
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<ArtifactVerification, String> {
    windows::verify_artifact(path, kind, trust, expected_architecture)
}

#[cfg(not(windows))]
pub fn verify_artifact(
    _path: &Path,
    _kind: PackageKind,
    _trust: &TrustEntry,
    _expected_architecture: Architecture,
) -> Result<ArtifactVerification, String> {
    Err("artifact verification is not implemented on this platform".into())
}

#[cfg(windows)]
pub fn plan_install_command(path: &Path, kind: PackageKind) -> Result<PlannedCommand, String> {
    windows::plan_install_command(path, kind)
}

#[cfg(not(windows))]
pub fn plan_install_command(_path: &Path, _kind: PackageKind) -> Result<PlannedCommand, String> {
    Err("installation is not implemented on this platform".into())
}

#[cfg(windows)]
pub fn execute_verified_installer(
    private_root: &Path,
    path: &Path,
    verified_identity: &StableFileIdentity,
    expected_sha256: Option<&str>,
    kind: PackageKind,
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<i32, String> {
    windows::execute_verified_installer(
        private_root,
        path,
        verified_identity,
        expected_sha256,
        kind,
        trust,
        expected_architecture,
    )
}

#[cfg(not(windows))]
pub fn execute_verified_installer(
    _private_root: &Path,
    _path: &Path,
    _verified_identity: &StableFileIdentity,
    _expected_sha256: Option<&str>,
    _kind: PackageKind,
    _trust: &TrustEntry,
    _expected_architecture: Architecture,
) -> Result<i32, String> {
    Err("installation is not implemented on this platform".into())
}
