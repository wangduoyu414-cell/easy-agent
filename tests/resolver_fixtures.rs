use ai_client_installer::adapters::{
    candidate_from_claude_redirect, parse_cc_switch_manifest, parse_hermes_homepage,
    parse_workbuddy_update,
};
use ai_client_installer::core::{Architecture, PackageKind, ProductId};

#[test]
fn parses_workbuddy_structured_update() {
    let candidate = parse_workbuddy_update(
        include_str!("fixtures/workbuddy/update.json"),
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
    let x64 = parse_cc_switch_manifest(source, Architecture::X64).unwrap();
    let arm64 = parse_cc_switch_manifest(source, Architecture::Arm64).unwrap();
    assert!(x64.download_url.path().ends_with("Windows.msi"));
    assert!(arm64.download_url.path().ends_with("Windows-arm64.msi"));
    assert!(x64.detached_signature.is_some());
}

#[test]
fn extracts_claude_version_from_final_official_asset() {
    let candidate = candidate_from_claude_redirect(
        include_str!("fixtures/claude/redirect.txt").trim(),
        Architecture::X64,
    )
    .unwrap();
    assert_eq!(candidate.version, "1.24012.9");
    assert_eq!(candidate.package_kind, PackageKind::Msix);
}

#[test]
fn rejects_changed_hermes_page_contract() {
    assert!(parse_hermes_homepage("<html>download</html>", Architecture::X64).is_err());
}
