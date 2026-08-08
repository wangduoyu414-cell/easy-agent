use serde::Deserialize;
use url::Url;

use crate::core::{Architecture, PackageKind, ProductId, ReleaseCandidate};

use super::AdapterError;

const CHATGPT_RELEASE_ROOT: &str = "https://persistent.oaistatic.com/codex-app-prod/";
const CHATGPT_PACKAGE_IDENTITY: &str = "OpenAI.Codex";

#[derive(Debug, Deserialize)]
struct WindowsUpdateManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "buildVersion")]
    build_version: String,
    #[serde(rename = "packageIdentity")]
    package_identity: String,
}

pub fn parse_chatgpt_windows_manifest(
    source: &str,
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    let manifest: WindowsUpdateManifest = serde_json::from_str(source)?;
    if manifest.schema_version != 1 {
        return Err(AdapterError::Contract(format!(
            "unsupported ChatGPT manifest schema {}",
            manifest.schema_version
        )));
    }
    if manifest.package_identity != CHATGPT_PACKAGE_IDENTITY {
        return Err(AdapterError::Contract(format!(
            "unexpected ChatGPT package identity {}",
            manifest.package_identity
        )));
    }
    if !is_appx_version(&manifest.build_version) {
        return Err(AdapterError::Contract(
            "ChatGPT buildVersion is not a four-part AppX version".into(),
        ));
    }
    let architecture_key = match architecture {
        Architecture::X64 => "x64",
        Architecture::Arm64 => "arm64",
        Architecture::Unsupported => return Err(AdapterError::NoMatchingArtifact),
    };
    let download_url = Url::parse(CHATGPT_RELEASE_ROOT)?.join(&format!(
        "releases/{}/ChatGPT-{architecture_key}.msix",
        manifest.build_version
    ))?;

    Ok(ReleaseCandidate {
        product: ProductId::ChatGpt,
        version: manifest.build_version,
        architecture,
        package_kind: PackageKind::Msix,
        download_url,
        expected_sha256: None,
        detached_signature: None,
    })
}

pub fn parse_chatgpt_macos_appcast(
    source: &str,
    architecture: Architecture,
) -> Result<ReleaseCandidate, AdapterError> {
    let document = roxmltree::Document::parse(source)
        .map_err(|error| AdapterError::Contract(format!("invalid ChatGPT appcast XML: {error}")))?;
    let item = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "item")
        .ok_or_else(|| AdapterError::Contract("ChatGPT appcast has no release item".into()))?;
    let version = item
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "shortVersionString")
        .and_then(|node| node.text())
        .or_else(|| {
            item.children()
                .find(|node| node.is_element() && node.tag_name().name() == "title")
                .and_then(|node| node.text())
        })
        .map(str::trim)
        .filter(|value| is_numeric_dot_version(value))
        .ok_or_else(|| AdapterError::Contract("ChatGPT appcast version is invalid".into()))?;
    let enclosure = item
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "enclosure")
        .ok_or_else(|| AdapterError::Contract("ChatGPT appcast has no enclosure".into()))?;
    let download_url = Url::parse(
        enclosure
            .attribute("url")
            .ok_or_else(|| AdapterError::Contract("ChatGPT enclosure URL is absent".into()))?,
    )?;
    let architecture_key = match architecture {
        Architecture::X64 => "x64",
        Architecture::Arm64 => "arm64",
        Architecture::Unsupported => return Err(AdapterError::NoMatchingArtifact),
    };
    let expected_file = format!("ChatGPT-darwin-{architecture_key}-{version}.zip");
    if !download_url.path().ends_with(&expected_file) {
        return Err(AdapterError::Contract(format!(
            "ChatGPT appcast enclosure does not match {architecture_key}/{version}"
        )));
    }
    let signature = enclosure
        .attributes()
        .find(|attribute| attribute.name() == "edSignature")
        .map(|attribute| attribute.value().trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AdapterError::Contract("ChatGPT appcast signature is absent".into()))?;
    if base64::Engine::decode(&base64::engine::general_purpose::STANDARD, signature).is_err() {
        return Err(AdapterError::Contract(
            "ChatGPT appcast signature is not valid base64".into(),
        ));
    }

    Ok(ReleaseCandidate {
        product: ProductId::ChatGpt,
        version: version.to_owned(),
        architecture,
        package_kind: PackageKind::Zip,
        download_url,
        expected_sha256: None,
        detached_signature: Some(signature.to_owned()),
    })
}

fn is_appx_version(version: &str) -> bool {
    let mut parts = version.split('.');
    let valid = (0..4).all(|_| {
        parts.next().is_some_and(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u16>().is_ok()
        })
    });
    valid && parts.next().is_none()
}

fn is_numeric_dot_version(version: &str) -> bool {
    !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{
      "schemaVersion": 1,
      "buildVersion": "26.727.6591.0",
      "storeProductId": "9PLM9XGG6VKS",
      "packageIdentity": "OpenAI.Codex"
    }"#;

    #[test]
    fn maps_only_supported_windows_architectures_to_fixed_openai_assets() {
        let x64 = parse_chatgpt_windows_manifest(VALID, Architecture::X64).unwrap();
        let arm64 = parse_chatgpt_windows_manifest(VALID, Architecture::Arm64).unwrap();
        assert_eq!(x64.version, "26.727.6591.0");
        assert_eq!(x64.package_kind, PackageKind::Msix);
        assert_eq!(
            x64.download_url.as_str(),
            "https://persistent.oaistatic.com/codex-app-prod/releases/26.727.6591.0/ChatGPT-x64.msix"
        );
        assert!(arm64.download_url.path().ends_with("/ChatGPT-arm64.msix"));
        assert!(parse_chatgpt_windows_manifest(VALID, Architecture::Unsupported).is_err());
    }

    #[test]
    fn fails_closed_when_the_openai_manifest_contract_changes() {
        for changed in [
            VALID.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2"),
            VALID.replace("OpenAI.Codex", "OpenAI.Other"),
            VALID.replace("26.727.6591.0", "26.727.latest.0"),
            VALID.replace("26.727.6591.0", "26.727.6591"),
            VALID.replace("26.727.6591.0", "26.727.70000.0"),
        ] {
            assert!(parse_chatgpt_windows_manifest(&changed, Architecture::X64).is_err());
        }
    }
}
