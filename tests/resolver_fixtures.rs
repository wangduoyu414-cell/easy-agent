use easy_agent::adapters::{
    candidate_from_claude_redirect, parse_cc_switch_manifest, parse_chatgpt_macos_appcast,
    parse_chatgpt_windows_manifest, parse_hermes_homepage, parse_workbuddy_update,
};
use easy_agent::core::{Architecture, OperatingSystem, PackageKind, ProductId};

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
    use easy_agent::core::{TrustRegistry, fetch_official_text, safe_http_client};
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
fn parses_chatgpt_official_manifest_into_a_direct_msix_candidate() {
    let candidate = parse_chatgpt_windows_manifest(
        include_str!("fixtures/chatgpt/windows-store-update.json"),
        Architecture::X64,
    )
    .unwrap();
    assert_eq!(candidate.product, ProductId::ChatGpt);
    assert_eq!(candidate.version, "26.727.6591.0");
    assert_eq!(candidate.package_kind, PackageKind::Msix);
    assert_eq!(
        candidate.download_url.as_str(),
        "https://persistent.oaistatic.com/codex-app-prod/releases/26.727.6591.0/ChatGPT-x64.msix"
    );
}
