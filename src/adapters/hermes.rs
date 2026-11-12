use regex::Regex;
use url::Url;

use crate::core::{
    Architecture, ArtifactSource, OperatingSystem, PackageKind, ProductId, ReleaseCandidate,
};

use super::AdapterError;

pub fn parse_hermes_homepage(
    source: &str,
    os: OperatingSystem,
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    parse_with_version(source, os, architecture, None)
}

pub(super) fn parse_with_version(
    source: &str,
    os: OperatingSystem,
    architecture: Architecture,
    release_version: Option<&str>,
) -> Result<ReleaseCandidate, AdapterError> {
    let version_regex =
        Regex::new(r"(?i)Hermes(?:\s+Agent)?\s+v?(\d+\.\d+\.\d+)").expect("static version regex");
    let (package_kind, url_regex) = match os {
        OperatingSystem::Windows => (
            PackageKind::Exe,
            Regex::new(
                r#"https://hermes-assets\.nousresearch\.com/Hermes-Setup\.exe(?:\?[^\"'<>\s]+)?"#,
            )
            .expect("static Windows URL regex"),
        ),
        OperatingSystem::MacOs if architecture == Architecture::Arm64 => (
            PackageKind::Dmg,
            Regex::new(
                r#"https://hermes-assets\.nousresearch\.com/Hermes-Setup\.dmg(?:\?[^\"'<>\s]+)?"#,
            )
            .expect("static macOS URL regex"),
        ),
        _ => return Err(AdapterError::NoMatchingArtifact),
    };
    let version = version_regex
        .captures(source)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str())
        .or(release_version)
        .map(str::to_owned)
        .ok_or_else(|| AdapterError::Contract("Hermes version marker not found".into()))?;
    let url = url_regex
        .find(source)
        .map(|value| value.as_str())
        .ok_or_else(|| AdapterError::Contract("Hermes platform asset link not found".into()))?;

    Ok(ReleaseCandidate {
        product: ProductId::Hermes,
        version,
        architecture,
        package_kind,
        download_url: Url::parse(url)?,
        source: ArtifactSource::Official,
        minimum_macos_version: None,
        expected_size: None,
        expected_sha256: None,
        detached_signature: None,
        bootstrap_payload: None,
    })
}

pub(super) fn stable_version(body: &str) -> Result<String, AdapterError> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
        draft: bool,
        prerelease: bool,
    }
    let release: Release = serde_json::from_str(body)?;
    if !release.draft
        && !release.prerelease
        && let Some(v) = release.tag_name.strip_prefix('v')
        && v.len() <= 32
        && v.split('.').count() == 3
        && v.split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Ok(v.to_owned());
    }
    Err(AdapterError::Contract(
        "unexpected Hermes stable release tag".into(),
    ))
}
