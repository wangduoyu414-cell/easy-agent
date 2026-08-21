use thiserror::Error;
use url::Url;

use crate::core::{
    DistributionKind, HttpError, InstallPlan, MacOsInstallStrategy, MicrosoftStorePlan,
    OperatingSystem, PlatformInfo, ProductId, ReleaseCandidate, SecurityError, TrustRegistry,
    TrustRegistryError, ensure_allowed_url, fetch_official_text, resolve_official_url,
    safe_http_client,
};

use super::{
    AdapterError, candidate_from_claude_redirect, parse_cc_switch_manifest,
    parse_chatgpt_macos_appcast, parse_chatgpt_windows_manifest, parse_hermes_homepage,
    parse_workbuddy_update,
};

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("no trust entry exists for this product/platform")]
    MissingTrustEntry,
    #[error("this adapter has no direct-install resolver: {0}")]
    NoDirectResolver(String),
    #[error(transparent)]
    Registry(#[from] TrustRegistryError),
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Security(#[from] SecurityError),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}

pub fn resolve_latest(
    product: ProductId,
    platform: &PlatformInfo,
    registry: &TrustRegistry,
) -> Result<ReleaseCandidate, ResolveError> {
    match resolve_install_plan(product, platform, registry)? {
        InstallPlan::DirectPackage(candidate) => Ok(candidate),
        InstallPlan::MicrosoftStore(_) => Err(ResolveError::NoDirectResolver(
            "product uses the Microsoft Store background installation strategy".into(),
        )),
    }
}

pub fn resolve_install_plan(
    product: ProductId,
    platform: &PlatformInfo,
    registry: &TrustRegistry,
) -> Result<InstallPlan, ResolveError> {
    let trust = registry
        .find(product, platform.os, platform.architecture)
        .ok_or(ResolveError::MissingTrustEntry)?;
    if !trust.enabled {
        return Err(ResolveError::NoDirectResolver(trust.status_reason.clone()));
    }
    if platform.os == OperatingSystem::MacOs
        && trust.macos_install_strategy != Some(MacOsInstallStrategy::DirectAppBundle)
    {
        return Err(ResolveError::NoDirectResolver(
            "当前 macOS 安装策略不是已实现的直接应用包安装".into(),
        ));
    }
    if platform.os == OperatingSystem::MacOs
        && let Some(minimum) = trust.minimum_macos_version.as_deref()
    {
        let current = platform.os_version.as_deref().ok_or_else(|| {
            ResolveError::NoDirectResolver("无法读取当前 macOS 版本，拒绝下载".into())
        })?;
        if numeric_version_is_older(current, minimum) {
            return Err(ResolveError::NoDirectResolver(format!(
                "需要 macOS {minimum} 或更高版本，当前为 {current}"
            )));
        }
    }
    if trust.distribution == DistributionKind::MicrosoftStore {
        let store_id = trust
            .store_id
            .clone()
            .ok_or_else(|| ResolveError::NoDirectResolver("trust entry has no Store ID".into()))?;
        return Ok(InstallPlan::MicrosoftStore(MicrosoftStorePlan {
            product,
            architecture: platform.architecture,
            store_id,
        }));
    }
    if trust.entry_urls.is_empty() {
        return Err(ResolveError::NoDirectResolver(trust.status_reason.clone()));
    }
    let client = safe_http_client()?;

    let mut last_error = None;
    let mut resolved = None;
    for entry in &trust.entry_urls {
        let attempt = (|| -> Result<ReleaseCandidate, ResolveError> {
            let entry_url = Url::parse(entry)?;
            ensure_allowed_url(&entry_url, trust)?;
            match product {
                ProductId::WorkBuddy => {
                    let (_, source) = fetch_official_text(&client, &entry_url, trust)?;
                    Ok(parse_workbuddy_update(
                        &source,
                        platform.os,
                        platform.architecture,
                    )?)
                }
                ProductId::Hermes => {
                    let (_, source) = fetch_official_text(&client, &entry_url, trust)?;
                    Ok(parse_hermes_homepage(
                        &source,
                        platform.os,
                        platform.architecture,
                    )?)
                }
                ProductId::CcSwitch => {
                    let (_, source) = fetch_official_text(&client, &entry_url, trust)?;
                    Ok(parse_cc_switch_manifest(
                        &source,
                        platform.os,
                        platform.architecture,
                    )?)
                }
                ProductId::Claude => {
                    let final_url = resolve_official_url(&client, &entry_url, trust)?;
                    Ok(candidate_from_claude_redirect(
                        final_url.as_str(),
                        platform.os,
                        platform.architecture,
                    )?)
                }
                ProductId::ChatGpt => {
                    let (_, source) = fetch_official_text(&client, &entry_url, trust)?;
                    match platform.os {
                        OperatingSystem::Windows => Ok(parse_chatgpt_windows_manifest(
                            &source,
                            platform.architecture,
                        )?),
                        OperatingSystem::MacOs => {
                            Ok(parse_chatgpt_macos_appcast(&source, platform.architecture)?)
                        }
                        OperatingSystem::Unsupported => Err(ResolveError::NoDirectResolver(
                            "unsupported operating system".into(),
                        )),
                    }
                }
            }
        })();
        match attempt {
            Ok(candidate) => {
                resolved = Some(candidate);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let candidate = resolved.ok_or_else(|| {
        last_error.unwrap_or_else(|| ResolveError::NoDirectResolver(trust.status_reason.clone()))
    })?;
    ensure_allowed_url(&candidate.download_url, trust)?;
    if !trust.package_kinds.contains(&candidate.package_kind) {
        return Err(ResolveError::Adapter(AdapterError::Contract(
            "resolved package type is outside the trust registry".into(),
        )));
    }
    Ok(InstallPlan::DirectPackage(candidate))
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
