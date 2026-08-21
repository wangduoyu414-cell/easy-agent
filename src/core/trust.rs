use std::collections::HashSet;

use base64::Engine;
use minisign_verify::PublicKey;
use serde::Deserialize;
use thiserror::Error;
use url::Url;

use super::{Architecture, OperatingSystem, PackageKind, PlatformInfo, ProductId, SupportState};

const CLAUDE_MSIX_PUBLISHER: &str = "CN=\"Anthropic, PBC\", O=\"Anthropic, PBC\", L=San Francisco, S=California, C=US, SERIALNUMBER=4860621, OID.2.5.4.15=Private Organization, OID.1.3.6.1.4.1.311.60.2.1.2=Delaware, OID.1.3.6.1.4.1.311.60.2.1.3=US";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DistributionKind {
    #[default]
    DirectPackage,
    MicrosoftStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacOsInstallStrategy {
    DirectAppBundle,
    VendorBootstrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteDigestPolicy {
    #[default]
    EnforceIfPresent,
    PlatformSignatureOnly,
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
    pub sparkle_ed25519_public_key: Option<String>,
    #[serde(default)]
    pub mirror_manifest_url: Option<String>,
    #[serde(default)]
    pub mirror_manifest_signature_url: Option<String>,
    #[serde(default)]
    pub mirror_artifact_base_url: Option<String>,
    #[serde(default)]
    pub mirror_url_rules: Vec<UrlRule>,
    #[serde(default)]
    pub mirror_manifest_public_key: Option<String>,
    #[serde(default)]
    pub mirror_max_stale_seconds: Option<u64>,
    #[serde(default)]
    pub remote_digest_policy: RemoteDigestPolicy,
    #[serde(default)]
    pub store_id: Option<String>,
    #[serde(default)]
    pub web_installer_signer_subject: Option<String>,
    #[serde(default)]
    pub macos_bundle_id: Option<String>,
    #[serde(default)]
    pub macos_team_id: Option<String>,
    #[serde(default)]
    pub macos_application_name: Option<String>,
    #[serde(default)]
    pub minimum_macos_version: Option<String>,
    #[serde(default)]
    pub macos_install_strategy: Option<MacOsInstallStrategy>,
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
                || entry.minimum_macos_version.is_some()
                || entry.macos_install_strategy.is_some();
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
            if entry.os == OperatingSystem::MacOs
                && !entry.package_kinds.is_empty()
                && entry.macos_install_strategy.is_none()
            {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "macOS package entries must declare macos_install_strategy".into(),
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

            if entry.updater_public_key.is_some() && entry.sparkle_ed25519_public_key.is_some() {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "an entry cannot configure both minisign and Sparkle Ed25519 keys".into(),
                ));
            }
            if let Some(encoded_key) = entry.sparkle_ed25519_public_key.as_deref() {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(encoded_key.trim())
                    .ok();
                if decoded.as_deref().is_none_or(|value| value.len() != 32)
                    || entry.os != OperatingSystem::MacOs
                    || entry.product != ProductId::ChatGpt
                    || entry.package_kinds.as_slice() != [PackageKind::Zip]
                {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "sparkle_ed25519_public_key is limited to a base64 32-byte ChatGPT macOS ZIP key"
                            .into(),
                    ));
                }
            }

            let has_any_mirror_field = entry.mirror_manifest_url.is_some()
                || entry.mirror_manifest_signature_url.is_some()
                || entry.mirror_artifact_base_url.is_some()
                || !entry.mirror_url_rules.is_empty()
                || entry.mirror_manifest_public_key.is_some()
                || entry.mirror_max_stale_seconds.is_some();
            if has_any_mirror_field {
                let mirror_complete = entry.mirror_manifest_url.is_some()
                    && entry.mirror_manifest_signature_url.is_some()
                    && entry.mirror_artifact_base_url.is_some()
                    && !entry.mirror_url_rules.is_empty()
                    && entry.mirror_manifest_public_key.is_some()
                    && entry.mirror_max_stale_seconds.is_some();
                if !mirror_complete {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "mirror fields must be configured as one complete set".into(),
                    ));
                }
                let is_claude_windows = entry.product == ProductId::Claude
                    && entry.os == OperatingSystem::Windows
                    && matches!(entry.architecture, Architecture::X64 | Architecture::Arm64)
                    && entry.distribution == DistributionKind::DirectPackage
                    && entry.package_kinds.as_slice() == [PackageKind::Msix]
                    && entry.signer_subjects.as_slice() == ["Anthropic, PBC"]
                    && entry.package_identity.as_deref() == Some("Claude")
                    && entry.package_family.as_deref() == Some("Claude_pzs8sxrjxfjjc")
                    && entry.msix_publisher.as_deref() == Some(CLAUDE_MSIX_PUBLISHER);
                let is_claude_macos = entry.product == ProductId::Claude
                    && entry.os == OperatingSystem::MacOs
                    && matches!(entry.architecture, Architecture::X64 | Architecture::Arm64)
                    && entry.distribution == DistributionKind::DirectPackage
                    && entry.package_kinds.as_slice() == [PackageKind::Dmg]
                    && entry.macos_install_strategy == Some(MacOsInstallStrategy::DirectAppBundle)
                    && entry.macos_application_name.as_deref() == Some("Claude.app")
                    && entry.macos_bundle_id.as_deref() == Some("com.anthropic.claudefordesktop")
                    && entry.macos_team_id.as_deref() == Some("Q6L2SF6YDW")
                    && entry.minimum_macos_version.as_deref() == Some("12.0");
                let is_chatgpt_macos = entry.product == ProductId::ChatGpt
                    && entry.os == OperatingSystem::MacOs
                    && matches!(entry.architecture, Architecture::X64 | Architecture::Arm64)
                    && entry.distribution == DistributionKind::DirectPackage
                    && entry.package_kinds.as_slice() == [PackageKind::Zip]
                    && entry.macos_install_strategy == Some(MacOsInstallStrategy::DirectAppBundle)
                    && entry.macos_bundle_id.as_deref() == Some("com.openai.codex")
                    && entry.macos_team_id.as_deref() == Some("2DC432GLL2")
                    && entry.sparkle_ed25519_public_key.is_some();
                if !is_claude_windows && !is_claude_macos && !is_chatgpt_macos {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "verified mirror support is limited to pinned Claude Windows/macOS or ChatGPT macOS identities"
                            .into(),
                    ));
                }
                if entry.mirror_url_rules.iter().any(|mirror_rule| {
                    entry.url_rules.iter().any(|official_rule| {
                        mirror_rule.host.eq_ignore_ascii_case(&official_rule.host)
                    })
                }) {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "official and mirror URL rules must use separate hosts".into(),
                    ));
                }
                let manifest_url = entry
                    .mirror_manifest_url
                    .as_deref()
                    .and_then(|value| Url::parse(value).ok());
                let signature_url = entry
                    .mirror_manifest_signature_url
                    .as_deref()
                    .and_then(|value| Url::parse(value).ok());
                if manifest_url
                    .as_ref()
                    .is_none_or(|url| !url_matches_rules(url, &entry.mirror_url_rules))
                    || signature_url
                        .as_ref()
                        .is_none_or(|url| !url_matches_rules(url, &entry.mirror_url_rules))
                {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "mirror manifest URLs must be HTTPS and match mirror_url_rules".into(),
                    ));
                }
                let artifact_base = entry
                    .mirror_artifact_base_url
                    .as_deref()
                    .and_then(|value| Url::parse(value).ok());
                let valid_artifact_base = artifact_base.as_ref().is_some_and(|url| {
                    url.scheme() == "https"
                        && url.host_str().is_some()
                        && url.path().ends_with('/')
                        && url.query().is_none()
                        && url.fragment().is_none()
                        && entry.mirror_url_rules.iter().any(|rule| {
                            url.host_str()
                                .is_some_and(|host| host.eq_ignore_ascii_case(&rule.host))
                        })
                });
                if !valid_artifact_base {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "mirror_artifact_base_url must be an HTTPS directory on a mirror host"
                            .into(),
                    ));
                }
                let valid_public_key = entry
                    .mirror_manifest_public_key
                    .as_deref()
                    .and_then(|encoded| {
                        base64::engine::general_purpose::STANDARD
                            .decode(encoded.trim())
                            .ok()
                    })
                    .and_then(|document| String::from_utf8(document).ok())
                    .is_some_and(|document| PublicKey::decode(&document).is_ok());
                if !valid_public_key {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "mirror_manifest_public_key is not a valid base64 minisign public key document"
                            .into(),
                    ));
                }
                if entry
                    .mirror_max_stale_seconds
                    .is_none_or(|seconds| !(3_600..=30 * 24 * 60 * 60).contains(&seconds))
                {
                    return Err(TrustRegistryError::Invalid(
                        key.0.into(),
                        key.1.into(),
                        key.2.into(),
                        "mirror_max_stale_seconds must be between one hour and 30 days".into(),
                    ));
                }
            }
            if entry.remote_digest_policy == RemoteDigestPolicy::PlatformSignatureOnly
                && (entry.product != ProductId::WorkBuddy
                    || entry.os != OperatingSystem::MacOs
                    || entry.distribution != DistributionKind::DirectPackage
                    || entry.package_kinds.as_slice() != [PackageKind::Zip]
                    || entry.macos_install_strategy != Some(MacOsInstallStrategy::DirectAppBundle)
                    || entry.macos_bundle_id.as_deref() != Some("com.workbuddy.workbuddy")
                    || entry.macos_team_id.as_deref() != Some("FN2V63AD2J"))
            {
                return Err(TrustRegistryError::Invalid(
                    key.0.into(),
                    key.1.into(),
                    key.2.into(),
                    "platform_signature_only digest policy is limited to the pinned WorkBuddy macOS ZIP identity"
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
                } else if entry.os == OperatingSystem::MacOs
                    && entry.macos_install_strategy != Some(MacOsInstallStrategy::DirectAppBundle)
                {
                    Some("implemented macos_install_strategy")
                } else if entry.os == OperatingSystem::MacOs
                    && entry.product == ProductId::ChatGpt
                    && entry.sparkle_ed25519_public_key.is_none()
                {
                    Some("sparkle_ed25519_public_key")
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
                    && entry.web_installer_signer_subject.is_none()
                {
                    Some("web_installer_signer_subject")
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

fn url_matches_rules(url: &Url, rules: &[UrlRule]) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    rules.iter().any(|rule| {
        host.eq_ignore_ascii_case(&rule.host)
            && (rule.exact_paths.iter().any(|path| url.path() == path)
                || rule
                    .path_prefixes
                    .iter()
                    .any(|prefix| url.path().starts_with(prefix)))
    })
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
