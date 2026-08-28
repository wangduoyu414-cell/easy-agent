use regex::Regex;
use serde::Deserialize;
use url::Url;

use crate::core::{
    Architecture, ArtifactSource, OperatingSystem, PackageKind, ProductId, ReleaseCandidate,
    TrustEntry, ensure_allowed_url_against_rules, verify_minisign_bytes,
};

use super::AdapterError;

const MAX_MIRROR_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MIRROR_CLOCK_SKEW_SECONDS: u64 = 5 * 60;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaudeMirrorManifest {
    schema: u32,
    product: String,
    os: String,
    architecture: String,
    version: String,
    size: u64,
    sha256: String,
    artifact_path: String,
    #[serde(default)]
    payload_size: Option<u64>,
    #[serde(default)]
    payload_sha256: Option<String>,
    #[serde(default)]
    payload_artifact_path: Option<String>,
    first_seen_at_unix: u64,
    last_successful_upstream_check_at_unix: u64,
    generated_at_unix: u64,
}

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
        OperatingSystem::Windows if url.path().to_ascii_lowercase().ends_with(".msix") => {
            PackageKind::Msix
        }
        OperatingSystem::Windows => {
            return Err(AdapterError::Contract(
                "Claude Windows artifact is not the official MSIX".into(),
            ));
        }
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
        source: ArtifactSource::Official,
        minimum_macos_version: None,
        expected_size: None,
        expected_sha256: None,
        detached_signature: None,
        bootstrap_payload: None,
    })
}

pub fn candidate_from_verified_claude_mirror(
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
    parse_verified_claude_mirror_manifest(manifest_bytes, trust, now_unix)
}

fn parse_verified_claude_mirror_manifest(
    manifest_bytes: &[u8],
    trust: &TrustEntry,
    now_unix: u64,
) -> Result<ReleaseCandidate, AdapterError> {
    let (expected_os, expected_architecture) = claude_mirror_contract(trust)?;
    let manifest: ClaudeMirrorManifest = serde_json::from_slice(manifest_bytes)?;
    let expected_schema = if trust.os == OperatingSystem::Windows {
        2
    } else {
        1
    };
    if manifest.schema != expected_schema
        || manifest.product != "claude"
        || manifest.os != expected_os
        || manifest.architecture != expected_architecture.key()
    {
        return Err(AdapterError::Contract(
            "mirror manifest product or platform contract changed".into(),
        ));
    }
    if !is_claude_release_version(&manifest.version, trust.os) {
        return Err(AdapterError::Contract(
            "mirror manifest version is invalid".into(),
        ));
    }
    if !valid_mirror_artifact_size(manifest.size) {
        return Err(AdapterError::Contract(
            "mirror manifest artifact size is invalid".into(),
        ));
    }
    if !valid_sha256(&manifest.sha256) {
        return Err(AdapterError::Contract(
            "mirror manifest SHA-256 is invalid".into(),
        ));
    }
    let artifact_name = match trust.os {
        OperatingSystem::Windows => "ClaudeSetup.exe",
        OperatingSystem::MacOs => "Claude.dmg",
        OperatingSystem::Unsupported => return Err(AdapterError::NoMatchingArtifact),
    };
    let expected_path = format!(
        "artifacts/claude/{}/{}/{}/{}/{}",
        expected_os,
        expected_architecture.key(),
        manifest.version,
        manifest.sha256,
        artifact_name
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
    let synced_at_unix = manifest.last_successful_upstream_check_at_unix;
    match trust.os {
        OperatingSystem::Windows => {
            let payload_size = manifest.payload_size.ok_or_else(|| {
                AdapterError::Contract("Claude Windows mirror payload size is absent".into())
            })?;
            let payload_sha256 = manifest.payload_sha256.as_deref().ok_or_else(|| {
                AdapterError::Contract("Claude Windows mirror payload SHA-256 is absent".into())
            })?;
            let payload_artifact_path =
                manifest.payload_artifact_path.as_deref().ok_or_else(|| {
                    AdapterError::Contract("Claude Windows mirror payload path is absent".into())
                })?;
            if !valid_mirror_artifact_size(payload_size) || !valid_sha256(payload_sha256) {
                return Err(AdapterError::Contract(
                    "Claude Windows mirror payload metadata is invalid".into(),
                ));
            }
            let expected_payload_path = format!(
                "artifacts/claude/windows/{}/{}/{}/Claude.msix",
                expected_architecture.key(),
                manifest.version,
                payload_sha256
            );
            if payload_artifact_path != expected_payload_path {
                return Err(AdapterError::Contract(
                    "Claude Windows mirror payload path is not immutable".into(),
                ));
            }
            let payload_url = checked_mirror_artifact_url(&base_url, payload_artifact_path, trust)?;
            Ok(ReleaseCandidate {
                product: ProductId::Claude,
                version: manifest.version.clone(),
                architecture: expected_architecture,
                package_kind: PackageKind::Msix,
                download_url: payload_url,
                source: ArtifactSource::VerifiedMirror { synced_at_unix },
                minimum_macos_version: None,
                expected_size: Some(payload_size),
                expected_sha256: Some(payload_sha256.to_owned()),
                detached_signature: None,
                bootstrap_payload: None,
            })
        }
        OperatingSystem::MacOs => {
            if manifest.payload_size.is_some()
                || manifest.payload_sha256.is_some()
                || manifest.payload_artifact_path.is_some()
            {
                return Err(AdapterError::Contract(
                    "Claude macOS mirror unexpectedly contains a bootstrap payload".into(),
                ));
            }
            let download_url =
                checked_mirror_artifact_url(&base_url, &manifest.artifact_path, trust)?;
            Ok(ReleaseCandidate {
                product: ProductId::Claude,
                version: manifest.version,
                architecture: expected_architecture,
                package_kind: PackageKind::Dmg,
                download_url,
                source: ArtifactSource::VerifiedMirror { synced_at_unix },
                minimum_macos_version: None,
                expected_size: Some(manifest.size),
                expected_sha256: Some(manifest.sha256),
                detached_signature: None,
                bootstrap_payload: None,
            })
        }
        OperatingSystem::Unsupported => Err(AdapterError::NoMatchingArtifact),
    }
}

fn valid_mirror_artifact_size(size: u64) -> bool {
    size > 0 && size <= MAX_MIRROR_ARTIFACT_BYTES
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn checked_mirror_artifact_url(
    base_url: &Url,
    artifact_path: &str,
    trust: &TrustEntry,
) -> Result<Url, AdapterError> {
    let url = base_url.join(artifact_path)?;
    ensure_allowed_url_against_rules(&url, &trust.mirror_url_rules).map_err(|error| {
        AdapterError::Contract(format!("mirror artifact URL rejected: {error}"))
    })?;
    Ok(url)
}

fn claude_mirror_contract(
    trust: &TrustEntry,
) -> Result<(&'static str, Architecture), AdapterError> {
    if trust.product != ProductId::Claude
        || !matches!(trust.architecture, Architecture::X64 | Architecture::Arm64)
    {
        return Err(AdapterError::Contract(
            "mirror trust entry is not a supported Claude platform".into(),
        ));
    }
    match trust.os {
        OperatingSystem::Windows if trust.package_kinds.as_slice() == [PackageKind::Msix] => {
            Ok(("windows", trust.architecture))
        }
        OperatingSystem::MacOs if trust.package_kinds.as_slice() == [PackageKind::Dmg] => {
            Ok(("macos", trust.architecture))
        }
        _ => Err(AdapterError::Contract(
            "mirror trust entry has an unsupported Claude package contract".into(),
        )),
    }
}

fn is_claude_release_version(version: &str, os: OperatingSystem) -> bool {
    let parts = version.split('.').collect::<Vec<_>>();
    (3..=4).contains(&parts.len())
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && match os {
                    OperatingSystem::Windows => part.parse::<u16>().is_ok(),
                    OperatingSystem::MacOs => part.parse::<u32>().is_ok(),
                    OperatingSystem::Unsupported => false,
                }
        })
}

#[cfg(test)]
mod tests {
    use super::parse_verified_claude_mirror_manifest;
    use crate::core::{Architecture, OperatingSystem, PackageKind, ProductId, TrustRegistry};

    fn mirror_registry() -> TrustRegistry {
        let public_key_document = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n";
        let encoded_key = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            public_key_document,
        );
        TrustRegistry::parse(&format!(
            r#"
schema_version = 1
[[entries]]
product = "claude"
os = "windows"
architecture = "x64"
enabled = true
status_reason = "fixture"
entry_urls = ["https://claude.ai/api/desktop/win32/x64/msix/latest/redirect"]
url_rules = [
  {{ host = "claude.ai", exact_paths = ["/api/desktop/win32/x64/msix/latest/redirect"] }},
  {{ host = "downloads.claude.ai", path_prefixes = ["/releases/"] }}
]
package_kinds = ["msix"]
signer_subjects = ["Anthropic, PBC"]
package_identity = "Claude"
package_family = "Claude_pzs8sxrjxfjjc"
msix_publisher = 'CN="Anthropic, PBC", O="Anthropic, PBC", L=San Francisco, S=California, C=US, SERIALNUMBER=4860621, OID.2.5.4.15=Private Organization, OID.1.3.6.1.4.1.311.60.2.1.2=Delaware, OID.1.3.6.1.4.1.311.60.2.1.3=US'
mirror_manifest_url = "https://mirror.example/manifests/claude/windows/x64/latest.json"
mirror_manifest_signature_url = "https://mirror.example/manifests/claude/windows/x64/latest.json.minisig"
mirror_artifact_base_url = "https://mirror.example/"
mirror_url_rules = [
  {{ host = "mirror.example", exact_paths = ["/manifests/claude/windows/x64/latest.json", "/manifests/claude/windows/x64/latest.json.minisig"], path_prefixes = ["/artifacts/claude/windows/x64/"] }}
]
mirror_manifest_public_key = "{encoded_key}"
mirror_max_stale_seconds = 604800
"#
        ))
        .unwrap()
    }

    #[test]
    fn verified_mirror_manifest_is_narrow_and_fresh() {
        let registry = mirror_registry();
        let trust = registry
            .find(
                ProductId::Claude,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        let now = 1_800_000_000_u64;
        let sha = "a".repeat(64);
        let payload_sha = "c".repeat(64);
        let manifest = format!(
            r#"{{
  "schema": 2,
  "product": "claude",
  "os": "windows",
  "architecture": "x64",
  "version": "1.26832.0",
  "size": 7020704,
  "sha256": "{sha}",
  "artifact_path": "artifacts/claude/windows/x64/1.26832.0/{sha}/ClaudeSetup.exe",
  "payload_size": 266210150,
  "payload_sha256": "{payload_sha}",
  "payload_artifact_path": "artifacts/claude/windows/x64/1.26832.0/{payload_sha}/Claude.msix",
  "first_seen_at_unix": {},
  "last_successful_upstream_check_at_unix": {},
  "generated_at_unix": {}
}}"#,
            now - 3600,
            now - 60,
            now - 60
        );
        let candidate =
            parse_verified_claude_mirror_manifest(manifest.as_bytes(), trust, now).unwrap();
        assert_eq!(candidate.version, "1.26832.0");
        assert_eq!(candidate.package_kind, PackageKind::Msix);
        assert_eq!(candidate.expected_size, Some(266_210_150));
        assert_eq!(
            candidate.expected_sha256.as_deref(),
            Some(payload_sha.as_str())
        );
        assert_eq!(candidate.download_url.host_str(), Some("mirror.example"));
        assert!(candidate.download_url.path().ends_with("/Claude.msix"));
        assert!(candidate.bootstrap_payload.is_none());

        let stale = manifest.replace(
            &format!("\"last_successful_upstream_check_at_unix\": {}", now - 60),
            &format!(
                "\"last_successful_upstream_check_at_unix\": {}",
                now - 604801
            ),
        );
        assert!(parse_verified_claude_mirror_manifest(stale.as_bytes(), trust, now).is_err());

        let legacy_windows_schema = manifest.replace("\"schema\": 2", "\"schema\": 1");
        assert!(
            parse_verified_claude_mirror_manifest(legacy_windows_schema.as_bytes(), trust, now)
                .is_err()
        );
        let mutable_payload_path = manifest.replace("/Claude.msix\"", "/latest.msix\"");
        assert!(
            parse_verified_claude_mirror_manifest(mutable_payload_path.as_bytes(), trust, now)
                .is_err()
        );
    }

    #[test]
    fn verified_macos_manifest_is_architecture_scoped_even_for_a_universal_dmg() {
        let public_key_document = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n";
        let encoded_key = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            public_key_document,
        );
        let registry = TrustRegistry::parse(&format!(
            r#"
schema_version = 1
[[entries]]
product = "claude"
os = "macos"
architecture = "arm64"
enabled = true
status_reason = "fixture"
entry_urls = ["https://claude.ai/api/desktop/darwin/universal/dmg/latest/redirect"]
url_rules = [
  {{ host = "claude.ai", exact_paths = ["/api/desktop/darwin/universal/dmg/latest/redirect"] }},
  {{ host = "downloads.claude.ai", path_prefixes = ["/releases/darwin/universal/"] }}
]
package_kinds = ["dmg"]
macos_install_strategy = "direct_app_bundle"
macos_application_name = "Claude.app"
macos_bundle_id = "com.anthropic.claudefordesktop"
macos_team_id = "Q6L2SF6YDW"
minimum_macos_version = "12.0"
mirror_manifest_url = "https://mirror.example/manifests/claude/macos/arm64/latest.json"
mirror_manifest_signature_url = "https://mirror.example/manifests/claude/macos/arm64/latest.json.minisig"
mirror_artifact_base_url = "https://mirror.example/"
mirror_url_rules = [
  {{ host = "mirror.example", exact_paths = ["/manifests/claude/macos/arm64/latest.json", "/manifests/claude/macos/arm64/latest.json.minisig"], path_prefixes = ["/artifacts/claude/macos/arm64/"] }}
]
mirror_manifest_public_key = "{encoded_key}"
mirror_max_stale_seconds = 604800
"#,
        ))
        .unwrap();
        let trust = registry
            .find(
                ProductId::Claude,
                OperatingSystem::MacOs,
                Architecture::Arm64,
            )
            .unwrap();
        let now = 1_800_000_000_u64;
        let sha = "b".repeat(64);
        let manifest = serde_json::json!({
            "schema": 1,
            "product": "claude",
            "os": "macos",
            "architecture": "arm64",
            "version": "1.26832.0",
            "size": 348_265_472,
            "sha256": sha,
            "artifact_path": format!("artifacts/claude/macos/arm64/1.26832.0/{sha}/Claude.dmg"),
            "first_seen_at_unix": now - 3600,
            "last_successful_upstream_check_at_unix": now - 60,
            "generated_at_unix": now - 60,
        });
        let candidate = parse_verified_claude_mirror_manifest(
            &serde_json::to_vec(&manifest).unwrap(),
            trust,
            now,
        )
        .unwrap();
        assert_eq!(candidate.architecture, Architecture::Arm64);
        assert_eq!(candidate.package_kind, PackageKind::Dmg);

        let mut wrong_architecture = manifest;
        wrong_architecture["architecture"] = serde_json::Value::String("x64".into());
        assert!(
            parse_verified_claude_mirror_manifest(
                &serde_json::to_vec(&wrong_architecture).unwrap(),
                trust,
                now,
            )
            .is_err()
        );
    }
}
