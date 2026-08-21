use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use url::Url;

use crate::core::{
    ArtifactSource, DistributionKind, HttpError, InstallPlan, MacOsInstallStrategy,
    MicrosoftStorePlan, OperatingSystem, PlatformInfo, ProductId, ReleaseCandidate, SecurityError,
    TrustRegistry, TrustRegistryError, ensure_allowed_url, ensure_allowed_url_against_rules,
    fetch_allowed_bytes, fetch_official_text, resolve_official_url, safe_http_client,
};

use super::{
    AdapterError, candidate_from_claude_redirect, candidate_from_verified_chatgpt_mirror,
    candidate_from_verified_claude_mirror, parse_cc_switch_manifest, parse_chatgpt_macos_appcast,
    parse_hermes_homepage, parse_workbuddy_update,
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
    #[error("official source unavailable ({official}); verified mirror unavailable ({mirror})")]
    OfficialAndMirrorUnavailable { official: String, mirror: String },
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
                    let official = (|| -> Result<ReleaseCandidate, ResolveError> {
                        let final_url = resolve_official_url(&client, &entry_url, trust)?;
                        Ok(candidate_from_claude_redirect(
                            final_url.as_str(),
                            platform.os,
                            platform.architecture,
                        )?)
                    })();
                    match official {
                        Ok(candidate) => Ok(candidate),
                        Err(official_error)
                            if trust.mirror_manifest_url.is_some()
                                && official_failure_allows_mirror(&official_error) =>
                        {
                            resolve_verified_mirror(&client, trust).map_err(|mirror_error| {
                                ResolveError::OfficialAndMirrorUnavailable {
                                    official: official_error.to_string(),
                                    mirror: mirror_error.to_string(),
                                }
                            })
                        }
                        Err(error) => Err(error),
                    }
                }
                ProductId::ChatGpt => {
                    let official = (|| -> Result<ReleaseCandidate, ResolveError> {
                        let (_, source) = fetch_official_text(&client, &entry_url, trust)?;
                        match platform.os {
                            OperatingSystem::Windows => Err(ResolveError::NoDirectResolver(
                                "ChatGPT Windows requires the Microsoft Store background strategy"
                                    .into(),
                            )),
                            OperatingSystem::MacOs => {
                                Ok(parse_chatgpt_macos_appcast(&source, platform.architecture)?)
                            }
                            OperatingSystem::Unsupported => Err(ResolveError::NoDirectResolver(
                                "unsupported operating system".into(),
                            )),
                        }
                    })();
                    match official {
                        Ok(candidate) => Ok(candidate),
                        Err(official_error)
                            if platform.os == OperatingSystem::MacOs
                                && trust.mirror_manifest_url.is_some()
                                && official_failure_allows_mirror(&official_error) =>
                        {
                            resolve_verified_mirror(&client, trust).map_err(|mirror_error| {
                                ResolveError::OfficialAndMirrorUnavailable {
                                    official: official_error.to_string(),
                                    mirror: mirror_error.to_string(),
                                }
                            })
                        }
                        Err(error) => Err(error),
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
    validate_resolved_candidate(&candidate, trust, platform)?;
    Ok(InstallPlan::DirectPackage(candidate))
}

fn validate_resolved_candidate(
    candidate: &ReleaseCandidate,
    trust: &crate::core::TrustEntry,
    platform: &PlatformInfo,
) -> Result<(), ResolveError> {
    validate_candidate_url_and_kind(candidate, trust)?;
    validate_candidate_platform_requirements(candidate, platform)?;
    if let Some(payload) = candidate.bootstrap_payload.as_deref() {
        if payload.bootstrap_payload.is_some()
            || payload.product != candidate.product
            || payload.version != candidate.version
            || payload.architecture != candidate.architecture
        {
            return Err(ResolveError::Adapter(AdapterError::Contract(
                "bootstrap payload does not match its installer candidate".into(),
            )));
        }
        validate_candidate_url_and_kind(payload, trust)?;
        validate_candidate_platform_requirements(payload, platform)?;
    }
    Ok(())
}

fn validate_candidate_platform_requirements(
    candidate: &ReleaseCandidate,
    platform: &PlatformInfo,
) -> Result<(), ResolveError> {
    let Some(minimum) = candidate.minimum_macos_version.as_deref() else {
        if candidate.product == ProductId::ChatGpt && platform.os == OperatingSystem::MacOs {
            return Err(ResolveError::Adapter(AdapterError::Contract(
                "ChatGPT release has no minimum macOS version".into(),
            )));
        }
        return Ok(());
    };
    if platform.os != OperatingSystem::MacOs || !is_numeric_dot_version(minimum) {
        return Err(ResolveError::Adapter(AdapterError::Contract(
            "release minimum macOS version is invalid for this platform".into(),
        )));
    }
    let current = platform.os_version.as_deref().ok_or_else(|| {
        ResolveError::NoDirectResolver("无法读取当前 macOS 版本，拒绝下载".into())
    })?;
    if numeric_version_is_older(current, minimum) {
        return Err(ResolveError::NoDirectResolver(format!(
            "{} {} 需要 macOS {minimum} 或更高版本，当前为 {current}",
            candidate.product, candidate.version
        )));
    }
    Ok(())
}

fn validate_candidate_url_and_kind(
    candidate: &ReleaseCandidate,
    trust: &crate::core::TrustEntry,
) -> Result<(), ResolveError> {
    match candidate.source {
        ArtifactSource::Official => ensure_allowed_url(&candidate.download_url, trust)?,
        ArtifactSource::VerifiedMirror { .. } => {
            ensure_allowed_url_against_rules(&candidate.download_url, &trust.mirror_url_rules)?
        }
    }
    if !trust.package_kinds.contains(&candidate.package_kind) {
        return Err(ResolveError::Adapter(AdapterError::Contract(
            "resolved package type is outside the trust registry".into(),
        )));
    }
    Ok(())
}

pub fn resolve_verified_download_fallback(
    primary: &ReleaseCandidate,
    platform: &PlatformInfo,
    registry: &TrustRegistry,
) -> Result<ReleaseCandidate, ResolveError> {
    let supported = primary.product == ProductId::Claude
        && matches!(
            platform.os,
            OperatingSystem::Windows | OperatingSystem::MacOs
        )
        || primary.product == ProductId::ChatGpt && platform.os == OperatingSystem::MacOs;
    if !supported
        || primary.architecture != platform.architecture
        || !matches!(primary.source, ArtifactSource::Official)
    {
        return Err(ResolveError::NoDirectResolver(
            "verified download fallback is not available for this official candidate".into(),
        ));
    }
    let trust = registry
        .find(primary.product, platform.os, platform.architecture)
        .ok_or(ResolveError::MissingTrustEntry)?;
    if !trust.enabled || trust.mirror_manifest_url.is_none() {
        return Err(ResolveError::NoDirectResolver(
            "verified download fallback is not configured".into(),
        ));
    }
    let client = safe_http_client()?;
    let fallback = resolve_verified_mirror(&client, trust)?;
    validate_resolved_candidate(&fallback, trust, platform)?;
    ensure_fallback_matches_primary(primary, &fallback)?;
    Ok(fallback)
}

fn ensure_fallback_matches_primary(
    primary: &ReleaseCandidate,
    fallback: &ReleaseCandidate,
) -> Result<(), ResolveError> {
    if !fallback.source.is_verified_mirror()
        || fallback.product != primary.product
        || fallback.version != primary.version
        || fallback.architecture != primary.architecture
        || fallback.package_kind != primary.package_kind
        || fallback.minimum_macos_version != primary.minimum_macos_version
        || fallback.expected_size.is_none()
        || primary
            .expected_size
            .is_some_and(|expected| fallback.expected_size != Some(expected))
        || fallback.expected_sha256.is_none()
        || primary
            .expected_sha256
            .as_deref()
            .is_some_and(|expected| fallback.expected_sha256.as_deref() != Some(expected))
        || primary
            .detached_signature
            .as_deref()
            .is_some_and(|expected| fallback.detached_signature.as_deref() != Some(expected))
        || !bootstrap_payloads_match(primary, fallback)
    {
        return Err(ResolveError::Adapter(AdapterError::Contract(
            "verified fallback does not describe the exact official release".into(),
        )));
    }
    Ok(())
}

fn bootstrap_payloads_match(primary: &ReleaseCandidate, fallback: &ReleaseCandidate) -> bool {
    match (
        primary.bootstrap_payload.as_deref(),
        fallback.bootstrap_payload.as_deref(),
    ) {
        (None, None) => true,
        (Some(primary), Some(fallback)) => {
            primary.bootstrap_payload.is_none()
                && fallback.bootstrap_payload.is_none()
                && primary.product == fallback.product
                && primary.version == fallback.version
                && primary.architecture == fallback.architecture
                && primary.package_kind == fallback.package_kind
                && primary.download_url == fallback.download_url
                && primary.source.is_verified_mirror()
                && fallback.source.is_verified_mirror()
                && primary.minimum_macos_version == fallback.minimum_macos_version
                && primary.expected_size == fallback.expected_size
                && primary.expected_sha256 == fallback.expected_sha256
                && primary.detached_signature == fallback.detached_signature
        }
        _ => false,
    }
}

fn resolve_verified_mirror(
    client: &reqwest::blocking::Client,
    trust: &crate::core::TrustEntry,
) -> Result<ReleaseCandidate, ResolveError> {
    let manifest_url =
        Url::parse(trust.mirror_manifest_url.as_deref().ok_or_else(|| {
            ResolveError::NoDirectResolver("mirror manifest URL is absent".into())
        })?)?;
    let signature_url = Url::parse(trust.mirror_manifest_signature_url.as_deref().ok_or_else(
        || ResolveError::NoDirectResolver("mirror manifest signature URL is absent".into()),
    )?)?;
    let (_, manifest_bytes) = fetch_allowed_bytes(client, &manifest_url, &trust.mirror_url_rules)?;
    let (_, signature_bytes) =
        fetch_allowed_bytes(client, &signature_url, &trust.mirror_url_rules)?;
    if signature_bytes.len() > 64 * 1024 {
        return Err(ResolveError::Adapter(AdapterError::Contract(
            "mirror manifest signature is too large".into(),
        )));
    }
    let signature_text = String::from_utf8(signature_bytes)
        .map_err(|_| ResolveError::Http(HttpError::InvalidUtf8))?;
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ResolveError::NoDirectResolver("system clock is before Unix epoch".into()))?
        .as_secs();
    Ok(match (trust.product, trust.os) {
        (ProductId::Claude, OperatingSystem::Windows | OperatingSystem::MacOs) => {
            candidate_from_verified_claude_mirror(
                &manifest_bytes,
                &signature_text,
                trust,
                now_unix,
            )?
        }
        (ProductId::ChatGpt, OperatingSystem::MacOs) => candidate_from_verified_chatgpt_mirror(
            &manifest_bytes,
            &signature_text,
            trust,
            now_unix,
        )?,
        _ => {
            return Err(ResolveError::NoDirectResolver(
                "verified mirror is not implemented for this product/platform".into(),
            ));
        }
    })
}

fn official_failure_allows_mirror(error: &ResolveError) -> bool {
    match error {
        ResolveError::Http(HttpError::RegionRestricted) => true,
        ResolveError::Http(HttpError::Request(error)) => {
            error.is_timeout()
                || error.is_body()
                || (error.is_connect() && !request_error_has_certificate_failure(error))
        }
        ResolveError::Http(HttpError::HttpStatus(status)) => {
            matches!(
                *status,
                reqwest::StatusCode::FORBIDDEN
                    | reqwest::StatusCode::REQUEST_TIMEOUT
                    | reqwest::StatusCode::TOO_MANY_REQUESTS
                    | reqwest::StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
            ) || status.is_server_error()
        }
        _ => false,
    }
}

fn request_error_has_certificate_failure(error: &reqwest::Error) -> bool {
    let mut messages = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(current) = source {
        messages.push_str(" | ");
        messages.push_str(&current.to_string());
        source = current.source();
    }
    looks_like_certificate_failure(&messages)
}

fn looks_like_certificate_failure(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    (message.contains("certificate")
        && [
            "verify",
            "verification",
            "invalid",
            "expired",
            "not valid",
            "hostname",
            "host name",
            "issuer",
            "self signed",
            "untrusted",
            "not trusted",
        ]
        .iter()
        .any(|marker| message.contains(marker)))
        || message.contains("cert_e_")
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

fn is_numeric_dot_version(version: &str) -> bool {
    !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{
        ResolveError, ensure_fallback_matches_primary, looks_like_certificate_failure,
        official_failure_allows_mirror, validate_candidate_platform_requirements,
    };
    use crate::core::{
        Architecture, ArtifactSource, HttpError, OperatingSystem, PackageKind, PlatformInfo,
        ProductId, ReleaseCandidate, SecurityError,
    };

    #[test]
    fn verified_mirror_fallback_accepts_only_availability_failures() {
        for status in [
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
            reqwest::StatusCode::REQUEST_TIMEOUT,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
        ] {
            assert!(official_failure_allows_mirror(&ResolveError::Http(
                HttpError::HttpStatus(status)
            )));
        }
        assert!(official_failure_allows_mirror(&ResolveError::Http(
            HttpError::RegionRestricted
        )));
        assert!(!official_failure_allows_mirror(&ResolveError::Http(
            HttpError::HttpStatus(reqwest::StatusCode::NOT_FOUND)
        )));
        assert!(!official_failure_allows_mirror(&ResolveError::Security(
            SecurityError::HostNotAllowed("unexpected.example".into())
        )));
        assert!(looks_like_certificate_failure(
            "certificate verify failed: unable to get local issuer certificate"
        ));
        assert!(looks_like_certificate_failure("CERT_E_UNTRUSTEDROOT"));
        assert!(!looks_like_certificate_failure(
            "TLS peer closed the connection without sending close_notify"
        ));
    }

    #[test]
    fn chatgpt_download_fallback_must_be_the_exact_same_release() {
        let primary = ReleaseCandidate {
            product: ProductId::ChatGpt,
            version: "26.803.41515".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Zip,
            download_url: Url::parse(
                "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-x64-26.803.41515.zip",
            )
            .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: Some("12.0".into()),
            expected_size: Some(539_372_355),
            expected_sha256: None,
            detached_signature: Some("vendor-signature".into()),
            bootstrap_payload: None,
        };
        let mut fallback = primary.clone();
        fallback.download_url = Url::parse(
            "https://mirror.example/artifacts/chatgpt/macos/x64/26.803.41515/hash/ChatGPT-darwin-x64-26.803.41515.zip",
        )
        .unwrap();
        fallback.source = ArtifactSource::VerifiedMirror {
            synced_at_unix: 1_800_000_000,
        };
        fallback.expected_sha256 = Some("a".repeat(64));
        ensure_fallback_matches_primary(&primary, &fallback).unwrap();

        let mut changed_version = fallback.clone();
        changed_version.version = "26.804.1".into();
        assert!(ensure_fallback_matches_primary(&primary, &changed_version).is_err());
        let mut changed_minimum = fallback.clone();
        changed_minimum.minimum_macos_version = Some("13.0".into());
        assert!(ensure_fallback_matches_primary(&primary, &changed_minimum).is_err());
        let mut changed_signature = fallback;
        changed_signature.detached_signature = Some("different-signature".into());
        assert!(ensure_fallback_matches_primary(&primary, &changed_signature).is_err());
    }

    #[test]
    fn chatgpt_release_minimum_is_compared_with_the_current_macos_version() {
        let candidate = ReleaseCandidate {
            product: ProductId::ChatGpt,
            version: "26.810.41047".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Zip,
            download_url: Url::parse(
                "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-x64-26.810.41047.zip",
            )
            .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: Some("13.0".into()),
            expected_size: Some(543_880_408),
            expected_sha256: None,
            detached_signature: Some("vendor-signature".into()),
            bootstrap_payload: None,
        };
        let mut platform = PlatformInfo {
            os: OperatingSystem::MacOs,
            architecture: Architecture::X64,
            os_version: Some("12.6.9".into()),
            description: "fixture".into(),
        };
        let error = validate_candidate_platform_requirements(&candidate, &platform).unwrap_err();
        assert!(matches!(
            error,
            ResolveError::NoDirectResolver(message)
                if message.contains("需要 macOS 13.0") && message.contains("当前为 12.6.9")
        ));

        platform.os_version = Some("13.0".into());
        validate_candidate_platform_requirements(&candidate, &platform).unwrap();
        platform.os_version = Some("26.4.1".into());
        validate_candidate_platform_requirements(&candidate, &platform).unwrap();
    }

    #[test]
    fn claude_msix_download_fallback_adds_signed_size_and_digest_metadata() {
        let primary = ReleaseCandidate {
            product: ProductId::Claude,
            version: "1.26832.0".into(),
            architecture: Architecture::Arm64,
            package_kind: PackageKind::Msix,
            download_url: Url::parse(
                "https://downloads.claude.ai/releases/win32/arm64/1.26832.0/Claude.msix",
            )
            .unwrap(),
            source: ArtifactSource::Official,
            minimum_macos_version: None,
            expected_size: None,
            expected_sha256: None,
            detached_signature: None,
            bootstrap_payload: None,
        };
        let mut fallback = primary.clone();
        fallback.download_url = Url::parse(
            "https://mirror.example/artifacts/claude/windows/arm64/1.26832.0/hash/Claude.msix",
        )
        .unwrap();
        fallback.source = ArtifactSource::VerifiedMirror {
            synced_at_unix: 1_800_000_000,
        };
        fallback.expected_size = Some(261_248_276);
        fallback.expected_sha256 = Some("a".repeat(64));
        ensure_fallback_matches_primary(&primary, &fallback).unwrap();

        let mut changed_digest = fallback.clone();
        changed_digest.expected_sha256 = None;
        assert!(ensure_fallback_matches_primary(&primary, &changed_digest).is_err());

        fallback.version = "1.26831.0".into();
        assert!(ensure_fallback_matches_primary(&primary, &fallback).is_err());
    }
}
