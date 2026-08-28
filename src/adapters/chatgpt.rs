use serde::Deserialize;
use url::Url;

use crate::core::{
    Architecture, ArtifactSource, PackageKind, ProductId, ReleaseCandidate, TrustEntry,
    ensure_allowed_url_against_rules, verify_minisign_bytes,
};

use super::AdapterError;

const MAX_CHATGPT_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MIRROR_CLOCK_SKEW_SECONDS: u64 = 5 * 60;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatGptMirrorManifest {
    schema: u32,
    product: String,
    os: String,
    architecture: String,
    version: String,
    minimum_macos_version: String,
    size: u64,
    sha256: String,
    sparkle_ed25519_signature: String,
    artifact_path: String,
    first_seen_at_unix: u64,
    last_successful_upstream_check_at_unix: u64,
    generated_at_unix: u64,
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
    let minimum_macos_version = item
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "minimumSystemVersion")
        .and_then(|node| node.text())
        .map(str::trim)
        .filter(|value| is_numeric_dot_version(value))
        .ok_or_else(|| {
            AdapterError::Contract("ChatGPT appcast minimum macOS version is invalid".into())
        })?;
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
    if enclosure.attribute("type") != Some("application/octet-stream") {
        return Err(AdapterError::Contract(
            "ChatGPT appcast enclosure content type changed".into(),
        ));
    }
    let expected_size = enclosure
        .attribute("length")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|size| *size > 0 && *size <= MAX_CHATGPT_ARTIFACT_BYTES)
        .ok_or_else(|| AdapterError::Contract("ChatGPT appcast size is invalid".into()))?;
    let signature = enclosure
        .attributes()
        .find(|attribute| attribute.name() == "edSignature")
        .map(|attribute| attribute.value().trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AdapterError::Contract("ChatGPT appcast signature is absent".into()))?;
    if base64::Engine::decode(&base64::engine::general_purpose::STANDARD, signature)
        .ok()
        .is_none_or(|decoded| decoded.len() != 64)
    {
        return Err(AdapterError::Contract(
            "ChatGPT appcast signature is not a 64-byte Ed25519 signature".into(),
        ));
    }

    Ok(ReleaseCandidate {
        product: ProductId::ChatGpt,
        version: version.to_owned(),
        architecture,
        package_kind: PackageKind::Zip,
        download_url,
        source: ArtifactSource::Official,
        minimum_macos_version: Some(minimum_macos_version.to_owned()),
        expected_size: Some(expected_size),
        expected_sha256: None,
        detached_signature: Some(signature.to_owned()),
        bootstrap_payload: None,
    })
}

pub fn candidate_from_verified_chatgpt_mirror(
    manifest_bytes: &[u8],
    signature_text: &str,
    trust: &TrustEntry,
    now_unix: u64,
) -> Result<ReleaseCandidate, AdapterError> {
    let public_key = trust
        .mirror_manifest_public_key
        .as_deref()
        .ok_or_else(|| AdapterError::Contract("mirror manifest public key is absent".into()))?;
    verify_minisign_bytes(manifest_bytes, public_key, signature_text)
        .map_err(|_| AdapterError::Contract("mirror manifest signature is invalid".into()))?;
    parse_verified_chatgpt_mirror_manifest(manifest_bytes, trust, now_unix)
}

fn parse_verified_chatgpt_mirror_manifest(
    manifest_bytes: &[u8],
    trust: &TrustEntry,
    now_unix: u64,
) -> Result<ReleaseCandidate, AdapterError> {
    let manifest: ChatGptMirrorManifest = serde_json::from_slice(manifest_bytes)?;
    let architecture_key = match trust.architecture {
        Architecture::X64 => "x64",
        Architecture::Arm64 => "arm64",
        Architecture::Unsupported => return Err(AdapterError::NoMatchingArtifact),
    };
    if manifest.schema != 1
        || manifest.product != "chatgpt"
        || manifest.os != "macos"
        || manifest.architecture != architecture_key
    {
        return Err(AdapterError::Contract(
            "mirror manifest product or platform contract changed".into(),
        ));
    }
    if !is_numeric_dot_version(&manifest.version) {
        return Err(AdapterError::Contract(
            "mirror manifest version is invalid".into(),
        ));
    }
    if !is_numeric_dot_version(&manifest.minimum_macos_version) {
        return Err(AdapterError::Contract(
            "mirror manifest minimum macOS version is invalid".into(),
        ));
    }
    if trust.minimum_macos_version.is_none() {
        return Err(AdapterError::Contract(
            "trust entry minimum macOS version is absent".into(),
        ));
    }
    if manifest.size == 0 || manifest.size > MAX_CHATGPT_ARTIFACT_BYTES {
        return Err(AdapterError::Contract(
            "mirror manifest artifact size is invalid".into(),
        ));
    }
    if manifest.sha256.len() != 64
        || !manifest
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AdapterError::Contract(
            "mirror manifest SHA-256 is invalid".into(),
        ));
    }
    if base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &manifest.sparkle_ed25519_signature,
    )
    .ok()
    .is_none_or(|decoded| decoded.len() != 64)
    {
        return Err(AdapterError::Contract(
            "mirror manifest Sparkle signature is invalid".into(),
        ));
    }
    let expected_file = format!("ChatGPT-darwin-{architecture_key}-{}.zip", manifest.version);
    let expected_path = format!(
        "artifacts/chatgpt/macos/{architecture_key}/{}/{}/{expected_file}",
        manifest.version, manifest.sha256
    );
    if manifest.artifact_path != expected_path {
        return Err(AdapterError::Contract(
            "mirror artifact path is not the immutable expected path".into(),
        ));
    }
    if manifest.first_seen_at_unix > manifest.last_successful_upstream_check_at_unix
        || manifest.last_successful_upstream_check_at_unix > manifest.generated_at_unix
        || manifest.generated_at_unix > now_unix.saturating_add(MIRROR_CLOCK_SKEW_SECONDS)
    {
        return Err(AdapterError::Contract(
            "mirror manifest timestamps are inconsistent".into(),
        ));
    }
    let max_stale = trust
        .mirror_max_stale_seconds
        .ok_or_else(|| AdapterError::Contract("mirror maximum age is absent".into()))?;
    if now_unix.saturating_sub(manifest.last_successful_upstream_check_at_unix) > max_stale {
        return Err(AdapterError::Contract(
            "mirror manifest is older than the configured maximum age".into(),
        ));
    }

    let base_url = Url::parse(
        trust
            .mirror_artifact_base_url
            .as_deref()
            .ok_or_else(|| AdapterError::Contract("mirror artifact base URL is absent".into()))?,
    )?;
    let download_url = base_url.join(&manifest.artifact_path)?;
    ensure_allowed_url_against_rules(&download_url, &trust.mirror_url_rules).map_err(|error| {
        AdapterError::Contract(format!("mirror artifact URL rejected: {error}"))
    })?;

    Ok(ReleaseCandidate {
        product: ProductId::ChatGpt,
        version: manifest.version,
        architecture: trust.architecture,
        package_kind: PackageKind::Zip,
        download_url,
        source: ArtifactSource::VerifiedMirror {
            synced_at_unix: manifest.last_successful_upstream_check_at_unix,
        },
        minimum_macos_version: Some(manifest.minimum_macos_version),
        expected_size: Some(manifest.size),
        expected_sha256: Some(manifest.sha256),
        detached_signature: Some(manifest.sparkle_ed25519_signature),
        bootstrap_payload: None,
    })
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
    use crate::core::{OperatingSystem, TrustRegistry};

    #[test]
    fn macos_appcast_carries_dynamic_minimum_version_and_pins_artifact_metadata() {
        let fixture = include_str!("../../tests/fixtures/chatgpt/appcast-x64.xml");
        let candidate = parse_chatgpt_macos_appcast(fixture, Architecture::X64).unwrap();
        assert_eq!(candidate.expected_size, Some(548_904_195));
        assert_eq!(candidate.minimum_macos_version.as_deref(), Some("12.0"));
        assert_eq!(
            parse_chatgpt_macos_appcast(&fixture.replace(">12.0<", ">15.0<"), Architecture::X64)
                .unwrap()
                .minimum_macos_version
                .as_deref(),
            Some("15.0")
        );
        assert!(
            parse_chatgpt_macos_appcast(&fixture.replace(">12.0<", ">latest<"), Architecture::X64)
                .is_err()
        );
        assert!(
            parse_chatgpt_macos_appcast(
                &fixture.replace("length=\"548904195\"", "length=\"0\""),
                Architecture::X64
            )
            .is_err()
        );
        assert!(
            parse_chatgpt_macos_appcast(
                &fixture.replace(
                    "GrRLwV5k6XXH/gSjUG/eC81wSn76ij2HYjlkc96/PEbAg8rZiOZWnKyqSarO6hhyb9HzTIyb/aVRAgW4QO7vCg==",
                    "AA=="
                ),
                Architecture::X64
            )
            .is_err()
        );
    }

    #[test]
    fn verified_macos_mirror_manifest_is_fresh_immutable_and_architecture_scoped() {
        let mirror_public_key = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n",
        );
        let registry = TrustRegistry::parse(&format!(
            r#"
schema_version = 1
[[entries]]
product = "chat_gpt"
os = "macos"
architecture = "x64"
enabled = true
status_reason = "fixture"
entry_urls = ["https://persistent.oaistatic.com/codex-app-prod/appcast-x64.xml"]
url_rules = [{{ host = "persistent.oaistatic.com", exact_paths = ["/codex-app-prod/appcast-x64.xml"], path_prefixes = ["/codex-app-prod/ChatGPT-darwin-x64-"] }}]
package_kinds = ["zip"]
sparkle_ed25519_public_key = "mNfr1v9t63BfgDtlw4C8lRvSY6uMggIXABDOCi3tS6k="
macos_install_strategy = "direct_app_bundle"
macos_application_name = "ChatGPT.app"
macos_bundle_id = "com.openai.codex"
macos_team_id = "2DC432GLL2"
minimum_macos_version = "12.0"
mirror_manifest_url = "https://mirror.example/manifests/chatgpt/macos/x64/latest.json"
mirror_manifest_signature_url = "https://mirror.example/manifests/chatgpt/macos/x64/latest.json.minisig"
mirror_artifact_base_url = "https://mirror.example/"
mirror_url_rules = [{{ host = "mirror.example", exact_paths = ["/manifests/chatgpt/macos/x64/latest.json", "/manifests/chatgpt/macos/x64/latest.json.minisig"], path_prefixes = ["/artifacts/chatgpt/macos/x64/"] }}]
mirror_manifest_public_key = "{mirror_public_key}"
mirror_max_stale_seconds = 604800
"#
        ))
        .unwrap();
        let trust = registry
            .find(
                ProductId::ChatGpt,
                OperatingSystem::MacOs,
                Architecture::X64,
            )
            .unwrap();
        let now = 1_800_000_000_u64;
        let sha256 = "a".repeat(64);
        let vendor_signature =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0_u8; 64]);
        let manifest = serde_json::json!({
            "schema": 1,
            "product": "chatgpt",
            "os": "macos",
            "architecture": "x64",
            "version": "26.803.41515",
            "minimum_macos_version": "13.0",
            "size": 539_372_355,
            "sha256": sha256,
            "sparkle_ed25519_signature": vendor_signature,
            "artifact_path": format!(
                "artifacts/chatgpt/macos/x64/26.803.41515/{sha256}/ChatGPT-darwin-x64-26.803.41515.zip"
            ),
            "first_seen_at_unix": now - 120,
            "last_successful_upstream_check_at_unix": now - 60,
            "generated_at_unix": now - 60
        });
        let candidate = parse_verified_chatgpt_mirror_manifest(
            &serde_json::to_vec(&manifest).unwrap(),
            trust,
            now,
        )
        .unwrap();
        assert_eq!(candidate.architecture, Architecture::X64);
        assert_eq!(candidate.minimum_macos_version.as_deref(), Some("13.0"));
        assert_eq!(candidate.expected_sha256.as_deref(), Some(sha256.as_str()));
        assert!(candidate.source.is_verified_mirror());

        let mut wrong_path = manifest.clone();
        wrong_path["artifact_path"] = serde_json::Value::String("artifacts/other.zip".into());
        assert!(
            parse_verified_chatgpt_mirror_manifest(
                &serde_json::to_vec(&wrong_path).unwrap(),
                trust,
                now
            )
            .is_err()
        );
        let mut invalid_minimum = manifest.clone();
        invalid_minimum["minimum_macos_version"] = serde_json::Value::String("latest".into());
        assert!(
            parse_verified_chatgpt_mirror_manifest(
                &serde_json::to_vec(&invalid_minimum).unwrap(),
                trust,
                now
            )
            .is_err()
        );
        let mut stale = manifest;
        stale["last_successful_upstream_check_at_unix"] = serde_json::Value::from(now - 604_801);
        assert!(
            parse_verified_chatgpt_mirror_manifest(
                &serde_json::to_vec(&stale).unwrap(),
                trust,
                now
            )
            .is_err()
        );
    }
}
