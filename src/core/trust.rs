use std::collections::HashSet;

use serde::Deserialize;
use thiserror::Error;

use super::{Architecture, OperatingSystem, PackageKind, ProductId, SupportState};

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
    pub enabled: bool,
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
    pub updater_public_key: Option<String>,
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

            if entry.enabled {
                let missing = if entry.entry_urls.is_empty() {
                    Some("entry_urls")
                } else if entry.url_rules.is_empty() {
                    Some("url_rules")
                } else if entry.package_kinds.is_empty() {
                    Some("package_kinds")
                } else if entry.package_kinds.contains(&PackageKind::Msix)
                    && entry.msix_publisher.is_none()
                {
                    Some("msix_publisher")
                } else if entry.package_kinds.contains(&PackageKind::Msix)
                    && entry.package_family.is_none()
                {
                    Some("package_family")
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
            Some(entry) => SupportState::Disabled(entry.status_reason.clone()),
            None => SupportState::Unsupported("没有匹配当前系统与架构的可信安装策略".into()),
        }
    }
}
