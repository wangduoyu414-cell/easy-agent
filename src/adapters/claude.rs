use regex::Regex;
use url::Url;

use crate::core::{Architecture, PackageKind, ProductId, ReleaseCandidate};

use super::AdapterError;

pub fn candidate_from_claude_redirect(
    final_url: &str,
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    let url = Url::parse(final_url)?;
    let version_regex = Regex::new(r"(?i)(\d+\.\d+\.\d+(?:\.\d+)?)").expect("static version regex");
    let version = version_regex
        .captures(url.path())
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
        .ok_or_else(|| AdapterError::Contract("version absent from Claude asset URL".into()))?;
    if !url.path().to_ascii_lowercase().ends_with(".msix") {
        return Err(AdapterError::Contract(
            "Claude Windows artifact is not MSIX".into(),
        ));
    }

    Ok(ReleaseCandidate {
        product: ProductId::Claude,
        version,
        architecture,
        package_kind: PackageKind::Msix,
        download_url: url,
        expected_sha256: None,
        detached_signature: None,
    })
}
