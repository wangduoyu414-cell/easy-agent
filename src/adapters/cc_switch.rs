use std::collections::HashMap;

use base64::Engine;
use serde::Deserialize;
use url::Url;

use crate::core::{Architecture, OperatingSystem, PackageKind, ProductId, ReleaseCandidate};

use super::AdapterError;

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    version: String,
    platforms: HashMap<String, UpdatePlatform>,
}

#[derive(Debug, Deserialize)]
struct UpdatePlatform {
    url: String,
    signature: String,
}

pub fn parse_cc_switch_manifest(
    source: &str,
    os: OperatingSystem,
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    let manifest: UpdateManifest = serde_json::from_str(source)?;
    let (keys, package_kind): (&[&str], PackageKind) = match (os, architecture) {
        (OperatingSystem::Windows, Architecture::X64) => (
            &["windows-x86_64", "windows-x64", "windows-x86_64-msvc"],
            PackageKind::Msi,
        ),
        (OperatingSystem::Windows, Architecture::Arm64) => (
            &["windows-aarch64", "windows-arm64", "windows-aarch64-msvc"],
            PackageKind::Msi,
        ),
        (OperatingSystem::MacOs, Architecture::X64) => {
            (&["darwin-x86_64", "darwin-x64"], PackageKind::TarGz)
        }
        (OperatingSystem::MacOs, Architecture::Arm64) => {
            (&["darwin-aarch64", "darwin-arm64"], PackageKind::TarGz)
        }
        _ => return Err(AdapterError::NoMatchingArtifact),
    };
    let platform = keys
        .iter()
        .find_map(|key| manifest.platforms.get(*key))
        .ok_or(AdapterError::NoMatchingArtifact)?;
    if manifest.version.trim().is_empty() || platform.signature.trim().is_empty() {
        return Err(AdapterError::Contract(
            "missing release version or signature".into(),
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
            "CC Switch artifact is not a {}",
            package_kind.extension()
        )));
    }

    Ok(ReleaseCandidate {
        product: ProductId::CcSwitch,
        version: manifest.version,
        architecture,
        package_kind,
        download_url,
        expected_sha256: None,
        detached_signature: Some(signature),
    })
}
