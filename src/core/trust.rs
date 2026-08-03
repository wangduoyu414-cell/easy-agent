use std::collections::HashSet;

use serde::Deserialize;
use thiserror::Error;

use super::{Architecture, OperatingSystem, PackageKind, PlatformInfo, ProductId, SupportState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DistributionKind {
    #[default]
    DirectPackage,
    MicrosoftStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsPeMachine {
    X86,
    X64,
    Arm64,
}

impl WindowsPeMachine {
    pub const fn for_architecture(architecture: Architecture) -> Option<Self> {
        match architecture {
            Architecture::X64 => Some(Self::X64),
            Architecture::Arm64 => Some(Self::Arm64),
            Architecture::Unsupported => None,
        }
    }

    pub const fn architecture(self) -> Architecture {
        match self {
            Self::X86 => Architecture::Unsupported,
            Self::X64 => Architecture::X64,
            Self::Arm64 => Architecture::Arm64,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrustRegistry {
    pub schema_version: u32,
    pub entries: Vec<TrustEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrustEntry {
    pub product: ProductId,
    pub os: OperatingSystem,
    pub architecture: Architecture,
    #[serde(default)]
    pub distribution: DistributionKind,
    pub enabled: bool,
    #[serde(default)]
    pub unsupported: bool,
    pub status_reason: String,
    pub entry_urls: Vec<String>,
    pub url_rules: Vec<UrlRule>,
    pub package_kinds: Vec<PackageKind>,
    #[serde(default)]
    pub signer_subjects: Vec<String>,
    #[serde(default)]
    pub package_identity: Option<String>,
    #[serde(default)]
    pub msix_publisher: Option<String>,
    #[serde(default)]
    pub package_family: Option<String>,
    #[serde(default)]
    pub windows_exe_machine: Option<WindowsPeMachine>,
    #[serde(default)]
    pub postinstall_executable: Option<String>,
    #[serde(default)]
    pub allow_trusted_update_when_management_unknown: bool,
    #[serde(default)]
    pub updater_public_key: Option<String>,
    #[serde(default)]
    pub store_id: Option<String>,
    #[serde(default)]
    pub minimum_winget_version: Option<String>,
    #[serde(default)]
    pub app_installer_release_api: Option<String>,
    #[serde(default)]
    pub app_installer_bundle_asset: Option<String>,
    #[serde(default)]
    pub app_installer_dependencies_asset: Option<String>,
    #[serde(default)]
    pub app_installer_identity: Option<String>,
    #[serde(default)]
    pub app_installer_family: Option<String>,
    #[serde(default)]
    pub app_installer_publisher: Option<String>,
    #[serde(default)]
    pub dependency_identity_prefixes: Vec<String>,
    #[serde(default)]
    pub dependency_publishers: Vec<String>,
    #[serde(default)]
    pub macos_bundle_id: Option<String>,
    #[serde(default)]
    pub macos_team_id: Option<String>,
    #[serde(default)]
    pub macos_application_name: Option<String>,
    #[serde(default)]
    pub minimum_macos_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UrlRule {
    pub host: String,
    #[serde(default)]
    pub exact_paths: Vec<String>,
    #[serde(default)]
    pub path_prefixes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum TrustRegistryError {
    #[error("trust registry TOML is invalid: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("trust registry schema {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("duplicate trust entry for {0}/{1}/{2}")]
    Duplicate(String, String, String),
    #[error("enabled trust entry is incomplete for {0}/{1}/{2}: {3}")]
    Incomplete(String, String, String, String),
    #[error("trust entry is invalid for {0}/{1}/{2}: {3}")]
    Invalid(String, String, String, String),
}

impl TrustRegistry {
    pub fn embedded() -> Result<Self, TrustRegistryError> {
        Self::parse(include_str!("../../config/trust-registry.toml"))
    }

    pub fn parse(source: &str) -> Result<Self, TrustRegistryError> {
        let registry: Self = toml::from_str(source)?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn validate(&self) -> Result<(), TrustRegistryError> {
        if self.schema_version != 1 {
            return Err(TrustRegistryError::UnsupportedSchema(self.schema_version));
        }

        let mut keys = HashSet::new();
        for entry in &self.entries {
            let key = (
                entry.product.key(),
                entry.os.key(),
                entry.architecture.key(),
            );
            if !keys.insert(key) {
                return Err(TrustRegistryError::Duplicate(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                ));
            }

            if entry.windows_exe_machine.is_some()
                && (entry.os != OperatingSystem::Windows
                    || !entry.package_kinds.contains(&PackageKind::Exe))
            {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "windows_exe_machine is only valid for Windows EXE entries".into(),
                ));
            }

            if entry.enabled && entry.unsupported {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "an unsupported entry cannot be enabled".into(),
                ));
            }

            let has_macos_fields = entry.macos_bundle_id.is_some()
                || entry.macos_team_id.is_some()
                || entry.macos_application_name.is_some()
                || entry.minimum_macos_version.is_some();
            if has_macos_fields && entry.os != OperatingSystem::MacOs {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "macOS bundle fields are only valid for macOS entries".into(),
                ));
            }
            if let Some(application_name) = entry.macos_application_name.as_deref() {
                let valid = application_name == application_name.trim()
                    && application_name.ends_with(".app")
                    && !application_name.is_empty()
                    && !application_name
                        .chars()
                        .any(|character| matches!(character, '/' | '\\' | ':'))
                    && !matches!(application_name, "." | "..");
                if !valid {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "macos_application_name must be a single .app bundle name".into(),
                    ));
                }
            }
            if let Some(minimum) = entry.minimum_macos_version.as_deref()
                && !is_numeric_version(minimum)
            {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "minimum_macos_version must be numeric dot-separated".into(),
                ));
            }

            if let Some(configured_executable) = entry.postinstall_executable.as_deref() {
                let executable = configured_executable.trim();
                let is_safe_file_name = !executable.is_empty()
                    && executable == configured_executable
                    && !executable
                        .chars()
                        .any(|character| matches!(character, '\\' | '/' | ':'))
                    && !matches!(executable, "." | "..")
                    && executable.to_ascii_lowercase().ends_with(".exe");
                if entry.os != OperatingSystem::Windows
                    || !entry.package_kinds.contains(&PackageKind::Exe)
                    || !is_safe_file_name
                {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "postinstall_executable must be a single Windows EXE file name".into(),
                    ));
                }
            }

            if entry
                .windows_exe_machine
                .zip(WindowsPeMachine::for_architecture(entry.architecture))
                .is_some_and(|(bootstrap, target)| bootstrap != target)
                && entry.postinstall_executable.is_none()
            {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "a cross-architecture EXE bootstrap requires postinstall_executable".into(),
                ));
            }

            if entry.allow_trusted_update_when_management_unknown
                && (entry.os != OperatingSystem::Windows
                    || entry.distribution != DistributionKind::DirectPackage)
            {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "allow_trusted_update_when_management_unknown is only valid for Windows direct-package entries"
                        .into(),
                ));
            }

            if entry.allow_trusted_update_when_management_unknown
                && (entry.product != ProductId::CcSwitch
                    || entry.package_kinds.as_slice() != [PackageKind::Msi]
                    || entry.package_identity.as_deref() != Some("CC Switch"))
            {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "unknown-management update policy is limited to the pinned CC Switch MSI identity"
                        .into(),
                ));
            }

            if entry.enabled {
                let missing = if entry.entry_urls.is_empty() {
                    Some("entry_urls")
                } else if entry.url_rules.is_empty() {
                    Some("url_rules")
                } else if entry.package_kinds.is_empty() {
                    Some("package_kinds")
                } else if entry.os == OperatingSystem::MacOs
                    && !entry.package_kinds.iter().all(|kind| {
                        matches!(
                            kind,
                            PackageKind::Dmg | PackageKind::TarGz | PackageKind::Zip
                        )
                    })
                {
                    Some("macOS package_kinds")
                } else if entry.os == OperatingSystem::MacOs && entry.macos_bundle_id.is_none() {
                    Some("macos_bundle_id")
                } else if entry.os == OperatingSystem::MacOs && entry.macos_team_id.is_none() {
                    Some("macos_team_id")
                } else if entry.os == OperatingSystem::MacOs
                    && entry.macos_application_name.is_none()
                {
                    Some("macos_application_name")
                } else if entry.package_kinds.contains(&PackageKind::Msix)
                    && entry.msix_publisher.is_none()
                {
                    Some("msix_publisher")
                } else if entry.package_kinds.contains(&PackageKind::Msix)
                    && entry.package_family.is_none()
                {
                    Some("package_family")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.store_id.is_none()
                {
                    Some("store_id")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.minimum_winget_version.is_none()
                {
                    Some("minimum_winget_version")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.app_installer_release_api.is_none()
                {
                    Some("app_installer_release_api")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.app_installer_bundle_asset.is_none()
                {
                    Some("app_installer_bundle_asset")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.app_installer_dependencies_asset.is_none()
                {
                    Some("app_installer_dependencies_asset")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.app_installer_identity.is_none()
                {
                    Some("app_installer_identity")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.app_installer_family.is_none()
                {
                    Some("app_installer_family")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.app_installer_publisher.is_none()
                {
                    Some("app_installer_publisher")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.dependency_identity_prefixes.is_empty()
                {
                    Some("dependency_identity_prefixes")
                } else if entry.distribution == DistributionKind::MicrosoftStore
                    && entry.dependency_publishers.is_empty()
                {
                    Some("dependency_publishers")
                } else {
                    None
                };
                if let Some(field) = missing {
                    return Err(TrustRegistryError::Incomplete(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        field.into(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn find(
        &self,
        product: ProductId,
        os: OperatingSystem,
        architecture: Architecture,
    ) -> Option<&TrustEntry> {
        self.entries.iter().find(|entry| {
            entry.product == product && entry.os == os && entry.architecture == architecture
        })
    }

    pub fn support_state(
        &self,
        product: ProductId,
        os: OperatingSystem,
        architecture: Architecture,
    ) -> SupportState {
        match self.find(product, os, architecture) {
            Some(entry) if entry.enabled => SupportState::Ready,
            Some(entry) if entry.unsupported => {
                SupportState::Unsupported(entry.status_reason.clone())
            }
            Some(entry) => SupportState::Disabled(entry.status_reason.clone()),
            None => SupportState::Unsupported("没有匹配当前系统与架构的可信安装策略".into()),
        }
    }

    pub fn support_state_for_platform(
        &self,
        product: ProductId,
        platform: &PlatformInfo,
    ) -> SupportState {
        let state = self.support_state(product, platform.os, platform.architecture);
        if !matches!(state, SupportState::Ready) || platform.os != OperatingSystem::MacOs {
            return state;
        }
        let Some(entry) = self.find(product, platform.os, platform.architecture) else {
            return state;
        };
        let Some(minimum) = entry.minimum_macos_version.as_deref() else {
            return state;
        };
        let Some(current) = platform.os_version.as_deref() else {
            return SupportState::Disabled("无法读取当前 macOS 版本，拒绝安装".into());
        };
        if numeric_version_is_older(current, minimum) {
            SupportState::Unsupported(format!("需要 macOS {minimum} 或更高版本，当前为 {current}"))
        } else {
            state
        }
    }
}

fn is_numeric_version(version: &str) -> bool {
    !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn numeric_version_is_older(current: &str, minimum: &str) -> bool {
    let mut current = current
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    let mut minimum = minimum
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    loop {
        match (current.next(), minimum.next()) {
            (None, None) => return false,
            (left, right) => match left.unwrap_or(0).cmp(&right.unwrap_or(0)) {
                std::cmp::Ordering::Less => return true,
                std::cmp::Ordering::Greater => return false,
                std::cmp::Ordering::Equal => {}
            },
        }
    }
}
