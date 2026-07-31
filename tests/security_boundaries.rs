use std::fs;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use ai_client_installer::core::{
    Architecture, Detection, OperatingSystem, PackageKind, PlatformInfo, PreinstallDecision,
    ProductId, ReleaseCandidate, TrustRegistry, assess_existing_install, ensure_allowed_url,
    inspect_staged_file, run_install_batch, validate_staged_file_name, verify_minisign_file,
    verify_staged_identity, version_is_older,
};
use tempfile::tempdir;
use url::Url;

use ai_client_installer::platform::plan_install_command;

#[test]
fn embedded_registry_is_well_formed_and_fail_closed() {
    let registry = TrustRegistry::embedded().unwrap();
    assert_eq!(registry.schema_version, 1);
    let entry = registry
        .find(
            ProductId::CcSwitch,
            OperatingSystem::Windows,
            Architecture::X64,
        )
        .unwrap();
    assert!(!entry.enabled);
    assert!(entry.updater_public_key.is_some());
}

#[test]
fn host_and_path_must_both_match_the_embedded_contract() {
    let registry = TrustRegistry::embedded().unwrap();
    let entry = registry
        .find(
            ProductId::Claude,
            OperatingSystem::Windows,
            Architecture::X64,
        )
        .unwrap();
    ensure_allowed_url(
        &Url::parse("https://claude.ai/api/desktop/win32/x64/msix/latest/redirect").unwrap(),
        entry,
    )
    .unwrap();
    assert!(ensure_allowed_url(&Url::parse("http://claude.ai/bad").unwrap(), entry).is_err());
    assert!(
        ensure_allowed_url(
            &Url::parse("https://claude.ai/unrelated/package.msix").unwrap(),
            entry
        )
        .is_err()
    );
    assert!(
        ensure_allowed_url(
            &Url::parse("https://attacker.example/api/desktop/win32/x64/msix/latest/redirect")
                .unwrap(),
            entry,
        )
        .is_err()
    );
}

#[test]
fn execution_handoff_rejects_a_changed_file() {
    let root = tempdir().unwrap();
    let artifact = root.path().join("client.msi");
    fs::write(&artifact, b"verified bytes").unwrap();
    let verified = inspect_staged_file(root.path(), &artifact).unwrap();
    fs::write(&artifact, b"replaced bytes").unwrap();
    assert!(verify_staged_identity(root.path(), &artifact, &verified, None).is_err());
}

#[test]
fn execution_handoff_accepts_the_unchanged_file() {
    let root = tempdir().unwrap();
    let artifact = root.path().join("client.msix");
    fs::write(&artifact, b"verified bytes").unwrap();
    let verified = inspect_staged_file(root.path(), &artifact).unwrap();
    let rebound =
        verify_staged_identity(root.path(), &artifact, &verified, Some(&verified.sha256)).unwrap();
    assert_eq!(rebound, verified);
}

#[test]
fn installer_paths_are_passed_as_literal_process_arguments() {
    let path = std::path::Path::new(r"C:\Temp\client & calc.exe.msi");
    let plan = plan_install_command(path, PackageKind::Msi).unwrap();
    assert_eq!(plan.program, "msiexec.exe");
    assert_eq!(plan.arguments, vec!["/i", r"C:\Temp\client & calc.exe.msi"]);
    assert!(plan.environment.is_empty());
}

#[test]
fn staged_file_name_cannot_escape_the_private_directory() {
    assert!(validate_staged_file_name("Claude.msix").is_ok());
    assert!(validate_staged_file_name("..\\outside.exe").is_err());
    assert!(validate_staged_file_name("folder/client.msi").is_err());
}

#[test]
fn minisign_verification_is_enforced_and_rejects_tampering() {
    use base64::Engine;

    let root = tempdir().unwrap();
    let artifact = root.path().join("signed.bin");
    fs::write(&artifact, b"test").unwrap();
    let public_key_document = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n";
    let encoded_key = base64::engine::general_purpose::STANDARD.encode(public_key_document);
    let signature = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
    verify_minisign_file(&artifact, &encoded_key, signature).unwrap();
    fs::write(&artifact, b"tampered").unwrap();
    assert!(verify_minisign_file(&artifact, &encoded_key, signature).is_err());
}

#[test]
fn enabled_msix_entry_requires_publisher_and_family_pins() {
    let source = r#"
schema_version = 1
[[entries]]
product = "claude"
os = "windows"
architecture = "x64"
enabled = true
status_reason = "test"
entry_urls = ["https://claude.ai/redirect"]
url_rules = [{ host = "claude.ai", exact_paths = ["/redirect"] }]
package_kinds = ["msix"]
package_identity = "Claude"
"#;
    assert!(TrustRegistry::parse(source).is_err());
}

#[test]
fn sequential_batch_reports_each_disabled_product_without_network_side_effects() {
    let registry = TrustRegistry::embedded().unwrap();
    let platform = PlatformInfo {
        os: OperatingSystem::Windows,
        architecture: Architecture::X64,
        description: "test".into(),
    };
    let candidates = vec![
        ReleaseCandidate {
            product: ProductId::WorkBuddy,
            version: "1.0.0".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Exe,
            download_url: Url::parse("https://download.codebuddy.cn/workbuddy/client.exe").unwrap(),
            expected_sha256: None,
            detached_signature: None,
        },
        ReleaseCandidate {
            product: ProductId::CcSwitch,
            version: "1.0.0".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Msi,
            download_url: Url::parse("https://dl.ccswitch.io/client.msi").unwrap(),
            expected_sha256: None,
            detached_signature: Some("invalid".into()),
        },
    ];
    let updates = Mutex::new(Vec::new());
    let results = run_install_batch(
        candidates,
        platform,
        registry,
        Arc::new(AtomicBool::new(false)),
        |update| updates.lock().unwrap().push(update),
    );
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|result| result.state == ai_client_installer::core::OperationState::Failed)
    );
    assert_eq!(updates.lock().unwrap().len(), 2);
}

#[test]
fn numeric_versions_are_compared_without_lexical_errors() {
    assert!(version_is_older("3.9.0", "3.10.0"));
    assert!(!version_is_older("3.10.1", "3.10.0"));
    assert!(!version_is_older("1.2", "1.2.0"));
}

#[test]
fn existing_higher_or_managed_install_is_rejected_before_download() {
    let higher = Detection {
        installed: true,
        version: Some("4.0.0".into()),
        managed: false,
        management_known: true,
        evidence: "fixture".into(),
    };
    assert!(matches!(
        assess_existing_install(&higher, "3.19.1"),
        PreinstallDecision::Reject(message) if message.contains("拒绝降级")
    ));

    let managed = Detection {
        installed: true,
        version: Some("1.0.0".into()),
        managed: true,
        management_known: true,
        evidence: "fixture".into(),
    };
    assert!(matches!(
        assess_existing_install(&managed, "2.0.0"),
        PreinstallDecision::Reject(message) if message.contains("组织管理")
    ));

    let unknown = Detection {
        installed: true,
        version: Some("1.0.0".into()),
        managed: false,
        management_known: false,
        evidence: "fixture".into(),
    };
    assert!(matches!(
        assess_existing_install(&unknown, "2.0.0"),
        PreinstallDecision::Reject(message) if message.contains("无法确认")
    ));
}
