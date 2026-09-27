use std::collections::HashMap;

use base64::Engine;
use serde::Deserialize;
use url::Url;

use crate::core::{
    Architecture, ArtifactSource, OperatingSystem, PackageKind, ProductId, ReleaseCandidate,
};

use super::AdapterError;

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    name: String,
    platforms: HashMap<String, UpdatePlatform>,
}

#[derive(Debug, Deserialize)]
struct UpdatePlatform {
    url: String,
    signature: String,
}

pub fn parse_clash_verge_rev_manifest(
    source: &str,
    os: OperatingSystem,
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    let manifest: UpdateManifest = serde_json::from_str(source)?;
    let version = manifest
        .name
        .trim()
        .strip_prefix('v')
        .unwrap_or(manifest.name.trim());
    if version.is_empty()
        || !version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(AdapterError::Contract(
            "release name is not a v-prefixed numeric version".into(),
        ));
    }
    let (keys, package_kind): (&[&str], PackageKind) = match (os, architecture) {
        (OperatingSystem::Windows, Architecture::X64) => {
            (&["windows-x86_64", "windows-x86_64-nsis"], PackageKind::Exe)
        }
        (OperatingSystem::Windows, Architecture::Arm64) => (
            &["windows-aarch64", "windows-aarch64-nsis"],
            PackageKind::Exe,
        ),
        (OperatingSystem::MacOs, Architecture::X64) => {
            (&["darwin-x86_64", "darwin-x86_64-app"], PackageKind::TarGz)
        }
        (OperatingSystem::MacOs, Architecture::Arm64) => (
            &["darwin-aarch64", "darwin-aarch64-app"],
            PackageKind::TarGz,
        ),
        _ => return Err(AdapterError::NoMatchingArtifact),
    };
    let platform = keys
        .iter()
        .find_map(|key| manifest.platforms.get(*key))
        .ok_or(AdapterError::NoMatchingArtifact)?;
    if platform.signature.trim().is_empty() {
        return Err(AdapterError::Contract(
            "missing release minisign signature".into(),
        ));
    }
    let signature = base64::engine::general_purpose::STANDARD
        .decode(platform.signature.trim())
        .map_err(|_| AdapterError::Contract("updater signature is not valid base64".into()))?;
    let signature = String::from_utf8(signature).map_err(|_| {
        AdapterError::Contract("updater signature is not UTF-8 minisign text".into())
    })?;
    let download_url = Url::parse(&platform.url)?;
    let expected_suffix = format!(".{}", package_kind.extension());
    if !download_url
        .path()
        .to_ascii_lowercase()
        .ends_with(&expected_suffix)
    {
        return Err(AdapterError::Contract(format!(
            "Clash Verge Rev artifact is not a {}",
            package_kind.extension()
        )));
    }
    let version_marker = format!("/download/v{version}/");
    if !download_url.path().contains(&version_marker) {
        return Err(AdapterError::Contract(
            "Clash Verge Rev download URL does not match the manifest version".into(),
        ));
    }

    Ok(ReleaseCandidate {
        product: ProductId::ClashVergeRev,
        version: version.to_string(),
        architecture,
        package_kind,
        download_url,
        source: ArtifactSource::Official,
        minimum_macos_version: None,
        expected_size: None,
        expected_sha256: None,
        detached_signature: Some(signature),
        bootstrap_payload: None,
    })
}
