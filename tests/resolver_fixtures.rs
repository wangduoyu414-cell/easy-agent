use easy_agent::adapters::{
    candidate_from_claude_redirect, candidate_from_verified_claude_mirror,
    parse_cc_switch_manifest, parse_chatgpt_macos_appcast, parse_hermes_homepage,
    parse_workbuddy_update, resolve_install_plan, resolve_verified_download_fallback,
};
use easy_agent::core::{
    Architecture, ArtifactSource, InstallPlan, OperatingSystem, PackageKind, PlatformInfo,
    ProductId, TrustRegistry,
};

#[test]
fn parses_workbuddy_structured_update() {
    let candidate = parse_workbuddy_update(
        include_str!("fixtures/workbuddy/update.json"),
        OperatingSystem::Windows,
        Architecture::X64,
    )
    .unwrap();
    assert_eq!(candidate.product, ProductId::WorkBuddy);
    assert_eq!(candidate.version, "5.3.5.34189228");
    assert_eq!(candidate.package_kind, PackageKind::Exe);
}

#[test]
fn parses_hermes_official_homepage_contract() {
    let candidate = parse_hermes_homepage(
        include_str!("fixtures/hermes/homepage.html"),
        OperatingSystem::Windows,
        Architecture::X64,
    )
    .unwrap();
    assert_eq!(candidate.version, "0.19.1");
    assert_eq!(
        candidate.download_url.host_str(),
        Some("hermes-assets.nousresearch.com")
    );
}

#[test]
fn maps_cc_switch_architecture_without_guessing() {
    let source = include_str!("fixtures/cc_switch/latest.json");
    let x64 =
        parse_cc_switch_manifest(source, OperatingSystem::Windows, Architecture::X64).unwrap();
    let arm64 =
        parse_cc_switch_manifest(source, OperatingSystem::Windows, Architecture::Arm64).unwrap();
    assert!(x64.download_url.path().ends_with("Windows.msi"));
    assert!(arm64.download_url.path().ends_with("Windows-arm64.msi"));
    assert!(x64.detached_signature.is_some());
}

#[test]
fn extracts_claude_version_from_final_official_asset() {
    let candidate = candidate_from_claude_redirect(
        include_str!("fixtures/claude/redirect.txt").trim(),
        OperatingSystem::Windows,
        Architecture::X64,
    )
    .unwrap();
    assert_eq!(candidate.version, "1.24012.9");
    assert_eq!(candidate.package_kind, PackageKind::Msix);
}

#[test]
fn rejects_changed_hermes_page_contract() {
    assert!(
        parse_hermes_homepage(
            "<html>download</html>",
            OperatingSystem::Windows,
            Architecture::X64
        )
        .is_err()
    );
}

#[test]
fn parses_workbuddy_macos_zip_and_official_hash() {
    let candidate = parse_workbuddy_update(
        include_str!("fixtures/workbuddy/update-macos-x64.json"),
        OperatingSystem::MacOs,
        Architecture::X64,
    )
    .unwrap();
    assert_eq!(candidate.package_kind, PackageKind::Zip);
    assert_eq!(candidate.architecture, Architecture::X64);
    assert_eq!(
        candidate.expected_sha256.as_deref(),
        Some("81971beb350c7062355fcaa6e553a26faf0da7e5013cf1039f9d27d70ce5de3d")
    );
}

#[test]
fn maps_cc_switch_both_macos_architectures_to_the_signed_universal_archive() {
    let source = include_str!("fixtures/cc_switch/latest.json");
    let x64 = parse_cc_switch_manifest(source, OperatingSystem::MacOs, Architecture::X64).unwrap();
    let arm64 =
        parse_cc_switch_manifest(source, OperatingSystem::MacOs, Architecture::Arm64).unwrap();
    assert_eq!(x64.package_kind, PackageKind::TarGz);
    assert_eq!(x64.download_url, arm64.download_url);
    assert!(x64.detached_signature.is_some());
}

#[test]
fn parses_hermes_apple_silicon_dmg_and_rejects_intel() {
    let source = include_str!("fixtures/hermes/homepage.html");
    let candidate =
        parse_hermes_homepage(source, OperatingSystem::MacOs, Architecture::Arm64).unwrap();
    assert_eq!(candidate.package_kind, PackageKind::Dmg);
    assert!(parse_hermes_homepage(source, OperatingSystem::MacOs, Architecture::X64).is_err());
}

#[test]
fn parses_claude_universal_macos_dmg() {
    let candidate = candidate_from_claude_redirect(
        include_str!("fixtures/claude/redirect-macos.txt").trim(),
        OperatingSystem::MacOs,
        Architecture::Arm64,
    )
    .unwrap();
    assert_eq!(candidate.version, "1.24012.9");
    assert_eq!(candidate.package_kind, PackageKind::Dmg);
}

#[test]
fn verifies_the_deployed_claude_mirror_manifest_fixture_before_parsing() {
    use base64::Engine;

    let encoded_key = base64::engine::general_purpose::STANDARD
        .encode(include_str!("../ops/claude-mirror/mirror-signing.pub"));
    let registry = TrustRegistry::parse(&format!(
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
"#,
    ))
    .unwrap();
    let trust = registry
        .find(
            ProductId::Claude,
            OperatingSystem::Windows,
            Architecture::X64,
        )
        .unwrap();
    let manifest = include_bytes!("fixtures/claude-mirror/latest.json");
    let signature = include_str!("fixtures/claude-mirror/latest.json.minisig");
    let candidate =
        candidate_from_verified_claude_mirror(manifest, signature, trust, 1786466760).unwrap();
    assert!(matches!(
        candidate.source,
        ArtifactSource::VerifiedMirror { .. }
    ));
    assert_eq!(candidate.package_kind, PackageKind::Msix);
    assert_eq!(candidate.expected_size, Some(266_210_150));
    assert_eq!(
        candidate.expected_sha256.as_deref(),
        Some("6dc210bca31b55c9fa307d11c6b13a42c7f3a3886ccc35ca2ecb7e9fceba0139")
    );
    assert!(candidate.download_url.path().ends_with("/Claude.msix"));
    assert!(candidate.bootstrap_payload.is_none());

    let mut tampered = manifest.to_vec();
    tampered[0] ^= 1;
    assert!(
        candidate_from_verified_claude_mirror(&tampered, signature, trust, 1786466760).is_err()
    );
}

#[test]
fn parses_openai_macos_appcasts_without_crossing_architectures() {
    let arm64 = parse_chatgpt_macos_appcast(
        include_str!("fixtures/chatgpt/appcast-arm64.xml"),
        Architecture::Arm64,
    )
    .unwrap();
    let x64 = parse_chatgpt_macos_appcast(
        include_str!("fixtures/chatgpt/appcast-x64.xml"),
        Architecture::X64,
    )
    .unwrap();
    assert_eq!(arm64.version, "26.727.51351");
    assert_eq!(arm64.package_kind, PackageKind::Zip);
    assert!(arm64.detached_signature.is_some());
    assert!(x64.detached_signature.is_some());
    assert!(arm64.download_url.path().contains("darwin-arm64"));
    assert!(x64.download_url.path().contains("darwin-x64"));
    assert!(
        parse_chatgpt_macos_appcast(
            include_str!("fixtures/chatgpt/appcast-arm64.xml"),
            Architecture::X64
        )
        .is_err()
    );
}

#[test]
#[ignore = "live official network contract smoke test"]
fn live_macos_metadata_contracts_parse_from_pinned_official_entries() {
    use easy_agent::core::{fetch_official_text, safe_http_client};
    use url::Url;

    let registry = TrustRegistry::embedded().unwrap();
    let client = safe_http_client().unwrap();

    for architecture in [Architecture::X64, Architecture::Arm64] {
        let trust = registry
            .find(ProductId::WorkBuddy, OperatingSystem::MacOs, architecture)
            .unwrap();
        let entry = Url::parse(&trust.entry_urls[0]).unwrap();
        let (_, source) = fetch_official_text(&client, &entry, trust).unwrap();
        parse_workbuddy_update(&source, OperatingSystem::MacOs, architecture).unwrap();
    }

    let cc_trust = registry
        .find(
            ProductId::CcSwitch,
            OperatingSystem::MacOs,
            Architecture::Arm64,
        )
        .unwrap();
    let (_, cc_source) = fetch_official_text(
        &client,
        &Url::parse(&cc_trust.entry_urls[0]).unwrap(),
        cc_trust,
    )
    .unwrap();
    parse_cc_switch_manifest(&cc_source, OperatingSystem::MacOs, Architecture::X64).unwrap();
    parse_cc_switch_manifest(&cc_source, OperatingSystem::MacOs, Architecture::Arm64).unwrap();

    let hermes_trust = registry
        .find(
            ProductId::Hermes,
            OperatingSystem::MacOs,
            Architecture::Arm64,
        )
        .unwrap();
    let (_, hermes_source) = fetch_official_text(
        &client,
        &Url::parse(&hermes_trust.entry_urls[0]).unwrap(),
        hermes_trust,
    )
    .unwrap();
    parse_hermes_homepage(&hermes_source, OperatingSystem::MacOs, Architecture::Arm64).unwrap();

    for architecture in [Architecture::X64, Architecture::Arm64] {
        let trust = registry
            .find(ProductId::ChatGpt, OperatingSystem::MacOs, architecture)
            .unwrap();
        let entry = Url::parse(&trust.entry_urls[0]).unwrap();
        let (_, source) = fetch_official_text(&client, &entry, trust).unwrap();
        parse_chatgpt_macos_appcast(&source, architecture).unwrap();
    }
}

#[test]
#[ignore = "live official resolver smoke test; metadata only, no artifact download"]
fn live_enabled_macos_install_plans_resolve_for_both_architectures() {
    let registry = TrustRegistry::embedded().unwrap();
    for architecture in [Architecture::X64, Architecture::Arm64] {
        let platform = PlatformInfo {
            os: OperatingSystem::MacOs,
            architecture,
            os_version: Some("14.0".into()),
            description: "live resolver fixture".into(),
        };
        for product in [
            ProductId::WorkBuddy,
            ProductId::CcSwitch,
            ProductId::ChatGpt,
        ] {
            let plan = resolve_install_plan(product, &platform, &registry).unwrap();
            let InstallPlan::DirectPackage(candidate) = plan else {
                panic!("macOS enabled product unexpectedly resolved to a non-direct plan");
            };
            assert_eq!(candidate.product, product);
            assert_eq!(candidate.architecture, architecture);
            assert!(!candidate.version.trim().is_empty());
            assert!(matches!(
                candidate.package_kind,
                PackageKind::TarGz | PackageKind::Zip
            ));
        }
    }
}

#[test]
#[ignore = "live verified ChatGPT macOS download fallback smoke test"]
fn live_chatgpt_macos_download_fallback_matches_the_confirmed_release() {
    use url::Url;

    let registry = TrustRegistry::embedded().unwrap();
    for architecture in [Architecture::X64, Architecture::Arm64] {
        let platform = PlatformInfo {
            os: OperatingSystem::MacOs,
            architecture,
            os_version: Some("14.0".into()),
            description: "live ChatGPT download fallback fixture".into(),
        };
        let InstallPlan::DirectPackage(candidate) =
            resolve_install_plan(ProductId::ChatGpt, &platform, &registry).unwrap()
        else {
            panic!("ChatGPT macOS unexpectedly resolved to a non-direct plan");
        };
        let mut primary = candidate.clone();
        primary.source = ArtifactSource::Official;
        primary.expected_sha256 = None;
        primary.download_url = Url::parse(&format!(
            "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-{}-{}.zip",
            architecture.key(),
            primary.version
        ))
        .unwrap();

        let fallback = resolve_verified_download_fallback(&primary, &platform, &registry).unwrap();
        assert!(fallback.source.is_verified_mirror());
        assert_eq!(fallback.version, primary.version);
        assert_eq!(fallback.architecture, primary.architecture);
        assert_eq!(fallback.expected_size, primary.expected_size);
        assert_eq!(fallback.detached_signature, primary.detached_signature);
        assert!(fallback.expected_sha256.is_some());
    }
}

#[test]
#[ignore = "run on a real Mac; Claude may challenge automated Windows requests"]
fn live_claude_macos_redirect_resolves_to_a_universal_dmg() {
    use easy_agent::core::{TrustRegistry, resolve_official_url, safe_http_client};
    use url::Url;

    let registry = TrustRegistry::embedded().unwrap();
    let trust = registry
        .find(
            ProductId::Claude,
            OperatingSystem::MacOs,
            Architecture::Arm64,
        )
        .unwrap();
    let client = safe_http_client().unwrap();
    let final_url =
        resolve_official_url(&client, &Url::parse(&trust.entry_urls[0]).unwrap(), trust).unwrap();
    let candidate = candidate_from_claude_redirect(
        final_url.as_str(),
        OperatingSystem::MacOs,
        Architecture::Arm64,
    )
    .unwrap();
    assert_eq!(candidate.package_kind, PackageKind::Dmg);
}

#[test]
#[ignore = "live Claude four-platform official/fallback smoke test"]
fn live_claude_all_platforms_resolve_and_verified_fallbacks_match() {
    use url::Url;

    let registry = TrustRegistry::embedded().unwrap();
    for (os, architecture, package_kind) in [
        (
            OperatingSystem::Windows,
            Architecture::X64,
            PackageKind::Msix,
        ),
        (
            OperatingSystem::Windows,
            Architecture::Arm64,
            PackageKind::Msix,
        ),
        (OperatingSystem::MacOs, Architecture::X64, PackageKind::Dmg),
        (
            OperatingSystem::MacOs,
            Architecture::Arm64,
            PackageKind::Dmg,
        ),
    ] {
        let platform = PlatformInfo {
            os,
            architecture,
            os_version: Some(if os == OperatingSystem::MacOs {
                "14.0".into()
            } else {
                "10.0.26100".into()
            }),
            description: "live Claude mirror fallback fixture".into(),
        };

        let InstallPlan::DirectPackage(candidate) =
            resolve_install_plan(ProductId::Claude, &platform, &registry).unwrap()
        else {
            panic!("Claude {os:?}/{architecture:?} unexpectedly resolved to a non-direct plan");
        };

        assert_eq!(candidate.package_kind, package_kind);
        assert_eq!(candidate.architecture, architecture);
        if candidate.source.is_verified_mirror() {
            assert!(candidate.expected_size.is_some_and(|size| size > 0));
            assert!(candidate.expected_sha256.as_deref().is_some_and(|sha256| {
                sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            }));
            assert_eq!(candidate.download_url.host_str(), Some("43.161.214.205"));
        }

        let mut official_candidate = candidate.clone();
        official_candidate.source = ArtifactSource::Official;
        official_candidate.expected_size = None;
        official_candidate.expected_sha256 = None;
        let official_url = match os {
            OperatingSystem::Windows => format!(
                "https://downloads.claude.ai/releases/win32/{}/{}/Claude.msix",
                architecture.key(),
                candidate.version
            ),
            OperatingSystem::MacOs => format!(
                "https://downloads.claude.ai/releases/darwin/universal/{}/Claude.dmg",
                candidate.version
            ),
            OperatingSystem::Unsupported => unreachable!(),
        };
        official_candidate.download_url = Url::parse(&official_url).unwrap();
        let download_fallback =
            resolve_verified_download_fallback(&official_candidate, &platform, &registry).unwrap();
        assert_eq!(download_fallback.version, official_candidate.version);
        assert_eq!(download_fallback.architecture, architecture);
        assert!(download_fallback.expected_size.is_some());
        assert!(download_fallback.expected_sha256.is_some());
    }
}

#[test]
fn resolves_chatgpt_windows_to_the_fixed_store_product() {
    let registry = TrustRegistry::embedded().unwrap();
    let platform = PlatformInfo {
        os: OperatingSystem::Windows,
        architecture: Architecture::X64,
        os_version: None,
        description: "fixture".into(),
    };
    let plan = resolve_install_plan(ProductId::ChatGpt, &platform, &registry).unwrap();
    let InstallPlan::MicrosoftStore(plan) = plan else {
        panic!("ChatGPT Windows must use the Store background workflow");
    };
    assert_eq!(plan.product, ProductId::ChatGpt);
    assert_eq!(plan.architecture, Architecture::X64);
    assert_eq!(plan.store_id, "9PLM9XGG6VKS");
}
