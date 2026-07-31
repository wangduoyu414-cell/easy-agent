use serde::Deserialize;
use url::Url;

use crate::core::{Architecture, PackageKind, ProductId, ReleaseCandidate};

use super::AdapterError;

#[derive(Debug, Deserialize)]
struct WorkBuddyEnvelope {
    #[serde(default)]
    data: Option<WorkBuddyRelease>,
    #[serde(flatten)]
    direct: WorkBuddyRelease,
}

#[derive(Debug, Default, Deserialize)]
struct WorkBuddyRelease {
    #[serde(default, alias = "versionName")]
    version: String,
    #[serde(default, alias = "downloadUrl", alias = "download_url")]
    url: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    sha256hash: Option<String>,
    #[serde(default)]
    hash: Option<String>,
}

pub fn parse_workbuddy_update(
    source: &str,
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    let envelope: WorkBuddyEnvelope = serde_json::from_str(source)?;
    let release = envelope.data.unwrap_or(envelope.direct);
    if release.version.trim().is_empty() || release.url.trim().is_empty() {
        return Err(AdapterError::Contract(
            "missing version or download URL".into(),
        ));
    }
    Ok(ReleaseCandidate {
        product: ProductId::WorkBuddy,
        version: release.version,
        architecture,
        package_kind: PackageKind::Exe,
        download_url: Url::parse(&release.url)?,
        expected_sha256: [release.sha256, release.sha256hash, release.hash]
            .into_iter()
            .flatten()
            .find(|value| !value.trim().is_empty()),
        detached_signature: None,
    })
}
