use std::collections::HashMap;

use serde::Deserialize;
use url::Url;

use crate::core::{Architecture, PackageKind, ProductId, ReleaseCandidate};

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
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    let manifest: UpdateManifest = serde_json::from_str(source)?;
    let keys: &[&str] = match architecture {
        Architecture::X64 => &["windows-x86_64", "windows-x64", "windows-x86_64-msvc"],
        Architecture::Arm64 => &["windows-aarch64", "windows-arm64", "windows-aarch64-msvc"],
        Architecture::Unsupported => return Err(AdapterError::NoMatchingArtifact),
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
    let download_url = Url::parse(&platform.url)?;
    if !download_url.path().to_ascii_lowercase().ends_with(".msi") {
        return Err(AdapterError::Contract(
            "Windows artifact is not an MSI".into(),
        ));
    }

    Ok(ReleaseCandidate {
        product: ProductId::CcSwitch,
        version: manifest.version,
        architecture,
        package_kind: PackageKind::Msi,
        download_url,
        expected_sha256: None,
        detached_signature: Some(platform.signature.clone()),
    })
}
