use regex::Regex;
use url::Url;

use crate::core::{Architecture, OperatingSystem, PackageKind, ProductId, ReleaseCandidate};

use super::AdapterError;

pub fn candidate_from_claude_redirect(
    final_url: &str,
    os: OperatingSystem,
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    let url = Url::parse(final_url)?;
    let version_regex = Regex::new(r"(?i)(\d+\.\d+\.\d+(?:\.\d+)?)").expect("static version regex");
    let version = version_regex
        .captures(url.path())
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
        .ok_or_else(|| AdapterError::Contract("version absent from Claude asset URL".into()))?;
    let package_kind = match os {
        OperatingSystem::Windows => PackageKind::Msix,
        OperatingSystem::MacOs => PackageKind::Dmg,
        OperatingSystem::Unsupported => return Err(AdapterError::NoMatchingArtifact),
    };
    let expected_suffix = format!(".{}", package_kind.extension());
    if !url.path().to_ascii_lowercase().ends_with(&expected_suffix) {
        return Err(AdapterError::Contract(format!(
            "Claude artifact is not a {}",
            package_kind.extension()
        )));
    }

    Ok(ReleaseCandidate {
        product: ProductId::Claude,
        version,
        architecture,
        package_kind,
        download_url: url,
        expected_sha256: None,
        detached_signature: None,
    })
}
