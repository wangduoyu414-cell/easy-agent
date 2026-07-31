use thiserror::Error;
use url::Url;

use crate::core::{
    HttpError, PlatformInfo, ProductId, ReleaseCandidate, SecurityError, TrustRegistry,
    TrustRegistryError, ensure_allowed_url, fetch_official_text, resolve_official_url,
    safe_http_client,
};

use super::{
    AdapterError, candidate_from_claude_redirect, parse_cc_switch_manifest, parse_hermes_homepage,
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
    let trust = registry
        .find(product, platform.os, platform.architecture)
        .ok_or(ResolveError::MissingTrustEntry)?;
    let client = safe_http_client()?;
    if product == ProductId::ChatGpt {
        return Err(ResolveError::NoDirectResolver(
            "Microsoft 目录依赖闭包与授权 proof 尚未通过，禁止回退 Store 引导器".into(),
        ));
    }
    if trust.entry_urls.is_empty() {
        return Err(ResolveError::NoDirectResolver(trust.status_reason.clone()));
    }

    let mut last_error = None;
    let mut resolved = None;
    for entry in &trust.entry_urls {
        let attempt = (|| -> Result<ReleaseCandidate, ResolveError> {
            let entry_url = Url::parse(entry)?;
            ensure_allowed_url(&entry_url, trust)?;
            match product {
                ProductId::WorkBuddy => {
                    let (_, source) = fetch_official_text(&client, &entry_url, trust)?;
                    Ok(parse_workbuddy_update(&source, platform.architecture)?)
                }
                ProductId::Hermes => {
                    let (_, source) = fetch_official_text(&client, &entry_url, trust)?;
                    Ok(parse_hermes_homepage(&source, platform.architecture)?)
                }
                ProductId::CcSwitch => {
                    let (_, source) = fetch_official_text(&client, &entry_url, trust)?;
                    Ok(parse_cc_switch_manifest(&source, platform.architecture)?)
                }
                ProductId::Claude => {
                    let final_url = resolve_official_url(&client, &entry_url, trust)?;
                    Ok(candidate_from_claude_redirect(
                        final_url.as_str(),
                        platform.architecture,
                    )?)
                }
                ProductId::ChatGpt => unreachable!("handled above"),
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
    Ok(candidate)
}
