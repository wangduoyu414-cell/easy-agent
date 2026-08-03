use std::fs;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use easy_agent::core::{
    Architecture, Detection, DistributionKind, InstallPlan, OperatingSystem, PackageKind,
    PlatformInfo, PreinstallDecision, ProductId, ReleaseCandidate, TrustRegistry, WindowsPeMachine,
    assess_existing_install, assess_existing_install_for_product, ensure_allowed_url,
    inspect_staged_file, run_install_batch, validate_staged_file_name, verify_minisign_file,
    verify_staged_identity, version_is_older, version_is_older_for_product,
};
use tempfile::tempdir;
use url::Url;

#[cfg(unix)]
use std::os::unix::fs::symlink;

#[cfg(windows)]
use easy_agent::platform::plan_install_command;

#[test]
fn embedded_registry_enables_the_configured_windows_x64_strategies() {
    let registry = TrustRegistry::embedded().unwrap();
    assert_eq!(registry.schema_version, 1);
    for product in [
        ProductId::WorkBuddy,
        ProductId::Hermes,
        ProductId::CcSwitch,
        ProductId::Claude,
        ProductId::ChatGpt,
    ] {
        let entry = registry
            .find(product, OperatingSystem::Windows, Architecture::X64)
            .unwrap();
        assert!(
            entry.enabled,
            "{product:?} should be enabled on Windows x64"
        );
    }
    let workbuddy = registry
        .find(
            ProductId::WorkBuddy,
            OperatingSystem::Windows,
            Architecture::X64,
        )
        .unwrap();
    assert_eq!(workbuddy.windows_exe_machine, Some(WindowsPeMachine::X86));
    assert_eq!(
        workbuddy.postinstall_executable.as_deref(),
        Some("WorkBuddy.exe")
    );
    let cc_switch = registry
        .find(
            ProductId::CcSwitch,
            OperatingSystem::Windows,
            Architecture::X64,
        )
        .unwrap();
    assert!(cc_switch.updater_public_key.is_some());
    assert!(cc_switch.allow_trusted_update_when_management_unknown);
    let hermes = registry
        .find(
            ProductId::Hermes,
            OperatingSystem::Windows,
            Architecture::X64,
        )
        .unwrap();
    assert_eq!(hermes.postinstall_executable.as_deref(), Some("Hermes.exe"));
    let chatgpt = registry
        .find(
            ProductId::ChatGpt,
            OperatingSystem::Windows,
            Architecture::X64,
        )
        .unwrap();
    assert!(chatgpt.enabled);
    assert_eq!(chatgpt.distribution, DistributionKind::DirectPackage);
    assert_eq!(
        chatgpt.entry_urls.as_slice(),
        ["https://persistent.oaistatic.com/codex-app-prod/windows-store-update.json"]
    );
    assert!(chatgpt.store_id.is_none());
    assert!(
        registry
            .entries
            .iter()
            .filter(|entry| entry.enabled)
            .all(|entry| entry.distribution == DistributionKind::DirectPackage)
    );
}

#[test]
fn embedded_registry_models_both_macos_architectures_fail_closed() {
    let registry = TrustRegistry::embedded().unwrap();
    for architecture in [Architecture::X64, Architecture::Arm64] {
        for product in [
            ProductId::WorkBuddy,
            ProductId::CcSwitch,
            ProductId::Claude,
            ProductId::ChatGpt,
        ] {
            let entry = registry
                .find(product, OperatingSystem::MacOs, architecture)
                .unwrap();
            assert!(
                !entry.enabled,
                "{product:?}/{architecture:?} must remain gated"
            );
        }
    }
    assert!(matches!(
        registry.support_state(ProductId::Hermes, OperatingSystem::MacOs, Architecture::X64),
        easy_agent::core::SupportState::Unsupported(_)
    ));
    let chatgpt = registry
        .find(
            ProductId::ChatGpt,
            OperatingSystem::MacOs,
            Architecture::Arm64,
        )
        .unwrap();
    assert_eq!(chatgpt.minimum_macos_version.as_deref(), Some("14.0"));
    for architecture in [Architecture::X64, Architecture::Arm64] {
        let cc_switch = registry
            .find(ProductId::CcSwitch, OperatingSystem::MacOs, architecture)
            .unwrap();
        assert_eq!(
            cc_switch.macos_bundle_id.as_deref(),
            Some("com.ccswitch.desktop")
        );
        assert_eq!(cc_switch.macos_team_id.as_deref(), Some("R8UR22V2F9"));

        let workbuddy = registry
            .find(ProductId::WorkBuddy, OperatingSystem::MacOs, architecture)
            .unwrap();
        assert_eq!(
            workbuddy.macos_application_name.as_deref(),
            Some("WorkBuddy.app")
        );
        assert_eq!(
            workbuddy.macos_bundle_id.as_deref(),
            Some("com.workbuddy.workbuddy")
        );
        assert_eq!(workbuddy.macos_team_id.as_deref(), Some("FN2V63AD2J"));

        let chatgpt = registry
            .find(ProductId::ChatGpt, OperatingSystem::MacOs, architecture)
            .unwrap();
        assert_eq!(
            chatgpt.macos_application_name.as_deref(),
            Some("ChatGPT.app")
        );
        assert_eq!(chatgpt.macos_bundle_id.as_deref(), Some("com.openai.codex"));
        assert_eq!(chatgpt.macos_team_id.as_deref(), Some("2DC432GLL2"));

        let claude = registry
            .find(ProductId::Claude, OperatingSystem::MacOs, architecture)
            .unwrap();
        assert_eq!(
            claude.macos_bundle_id.as_deref(),
            Some("com.anthropic.claudefordesktop")
        );
        assert_eq!(claude.macos_team_id.as_deref(), Some("Q6L2SF6YDW"));
    }
    let hermes = registry
        .find(
            ProductId::Hermes,
            OperatingSystem::MacOs,
            Architecture::Arm64,
        )
        .unwrap();
    assert_eq!(hermes.macos_application_name.as_deref(), Some("Hermes.app"));
    assert_eq!(
        hermes.macos_bundle_id.as_deref(),
        Some("com.nousresearch.hermes.setup")
    );
    assert_eq!(hermes.macos_team_id.as_deref(), Some("T2F6S8MF7C"));
}

#[test]
fn enabled_macos_entry_requires_exact_bundle_and_team_identity() {
    let source = r#"
schema_version = 1
[[entries]]
product = "claude"
os = "macos"
architecture = "arm64"
enabled = true
status_reason = "fixture"
entry_urls = ["https://downloads.claude.ai/client.dmg"]
url_rules = [{ host = "downloads.claude.ai", exact_paths = ["/client.dmg"] }]
package_kinds = ["dmg"]
macos_application_name = "Claude.app"
macos_bundle_id = "com.anthropic.claudefordesktop"
minimum_macos_version = "12.0"
"#;
    assert!(TrustRegistry::parse(source).is_err());
}

#[test]
fn enabled_macos_entry_rejects_an_older_operating_system_before_resolution() {
    let registry = TrustRegistry::parse(
        r#"
schema_version = 1
[[entries]]
product = "chat_gpt"
os = "macos"
architecture = "arm64"
enabled = true
status_reason = "fixture"
entry_urls = ["https://persistent.oaistatic.com/codex-app-prod/appcast.xml"]
url_rules = [{ host = "persistent.oaistatic.com", exact_paths = ["/codex-app-prod/appcast.xml"] }]
package_kinds = ["zip"]
macos_application_name = "ChatGPT.app"
macos_bundle_id = "com.openai.codex"
macos_team_id = "ABCDE12345"
minimum_macos_version = "14.0"
"#,
    )
    .unwrap();
    let platform = PlatformInfo {
        os: OperatingSystem::MacOs,
        architecture: Architecture::Arm64,
        os_version: Some("13.6.9".into()),
        description: "fixture".into(),
    };
    assert!(matches!(
        registry.support_state_for_platform(ProductId::ChatGpt, &platform),
        easy_agent::core::SupportState::Unsupported(_)
    ));
}

#[test]
fn cross_architecture_exe_bootstrap_requires_a_pinned_postinstall_executable() {
    let source = r#"
schema_version = 1
[[entries]]
product = "work_buddy"
os = "windows"
architecture = "x64"
enabled = false
status_reason = "fixture"
entry_urls = []
url_rules = []
package_kinds = ["exe"]
windows_exe_machine = "x86"
"#;
    assert!(TrustRegistry::parse(source).is_err());
}

#[test]
fn postinstall_executable_must_be_a_single_safe_windows_file_name() {
    let source = r#"
schema_version = 1
[[entries]]
product = "work_buddy"
os = "windows"
architecture = "x64"
enabled = false
status_reason = "fixture"
entry_urls = []
url_rules = []
package_kinds = ["exe"]
windows_exe_machine = "x86"
postinstall_executable = "../WorkBuddy.exe"
"#;
    assert!(TrustRegistry::parse(source).is_err());
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
fn chatgpt_allows_only_the_fixed_openai_manifest_and_release_prefix() {
    let registry = TrustRegistry::embedded().unwrap();
    let entry = registry
        .find(
            ProductId::ChatGpt,
            OperatingSystem::Windows,
            Architecture::X64,
        )
        .unwrap();
    for allowed in [
        "https://persistent.oaistatic.com/codex-app-prod/windows-store-update.json",
        "https://persistent.oaistatic.com/codex-app-prod/releases/26.727.6591.0/ChatGPT-x64.msix",
    ] {
        ensure_allowed_url(&Url::parse(allowed).unwrap(), entry).unwrap();
    }
    for rejected in [
        "http://persistent.oaistatic.com/codex-app-prod/windows-store-update.json",
        "https://persistent.oaistatic.com/other/ChatGPT-x64.msix",
        "https://example.invalid/codex-app-prod/releases/26.727.6591.0/ChatGPT-x64.msix",
        "https://get.microsoft.com/installer/download/9PLM9XGG6VKS",
    ] {
        assert!(ensure_allowed_url(&Url::parse(rejected).unwrap(), entry).is_err());
    }
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

#[cfg(unix)]
#[test]
fn execution_handoff_allows_a_symlinked_ancestor_outside_the_private_root() {
    let outer = tempdir().unwrap();
    let real_parent = outer.path().join("real-parent");
    let alias = outer.path().join("alias");
    fs::create_dir(&real_parent).unwrap();
    symlink(&real_parent, &alias).unwrap();

    let root = alias.join("private-root");
    fs::create_dir(&root).unwrap();
    let artifact = root.join("client.dmg");
    fs::write(&artifact, b"verified bytes").unwrap();

    let verified = inspect_staged_file(&root, &artifact).unwrap();
    assert_eq!(verified.canonical_path, fs::canonicalize(artifact).unwrap());
}

#[cfg(unix)]
#[test]
fn execution_handoff_rejects_a_symlink_inside_the_private_root() {
    let root = tempdir().unwrap();
    let target = root.path().join("target.dmg");
    let artifact = root.path().join("client.dmg");
    fs::write(&target, b"verified bytes").unwrap();
    symlink(&target, &artifact).unwrap();

    assert!(inspect_staged_file(root.path(), &artifact).is_err());
}

#[cfg(windows)]
#[test]
fn installer_paths_are_passed_as_literal_process_arguments() {
    let path = std::path::Path::new(r"C:\Temp\client & calc.exe.msi");
    let plan = plan_install_command(path, PackageKind::Msi).unwrap();
    assert!(std::path::Path::new(&plan.program).is_absolute());
    assert_eq!(
        std::path::Path::new(&plan.program)
            .file_name()
            .and_then(|name| name.to_str()),
        Some("msiexec.exe")
    );
    assert_eq!(
        plan.arguments,
        vec!["/i", r"C:\Temp\client & calc.exe.msi", "/qn", "/norestart"]
    );
    assert!(plan.environment.is_empty());

    let msix = plan_install_command(
        std::path::Path::new(r"C:\Temp\client & calc.exe.msix"),
        PackageKind::Msix,
    )
    .unwrap();
    assert!(std::path::Path::new(&msix.program).is_absolute());
    assert_eq!(
        std::path::Path::new(&msix.program)
            .file_name()
            .and_then(|name| name.to_str()),
        Some("powershell.exe")
    );
    assert!(
        msix.environment
            .iter()
            .any(|(key, value)| key == "EASY_AGENT_ARTIFACT"
                && value == r"C:\Temp\client & calc.exe.msix")
    );
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
fn enabled_store_entry_requires_fixed_store_and_app_installer_trust() {
    let source = r#"
schema_version = 1
[[entries]]
product = "chat_gpt"
os = "windows"
architecture = "x64"
distribution = "microsoft_store"
enabled = true
status_reason = "test"
entry_urls = ["https://api.github.com/repos/microsoft/winget-cli/releases/latest"]
url_rules = [{ host = "api.github.com", exact_paths = ["/repos/microsoft/winget-cli/releases/latest"] }]
package_kinds = ["msix"]
package_identity = "OpenAI.Codex"
package_family = "OpenAI.Codex_2p2nqsd0c76g0"
msix_publisher = "CN=fixture"
"#;
    assert!(TrustRegistry::parse(source).is_err());
}

#[test]
fn unknown_management_override_is_limited_to_the_pinned_cc_switch_msi() {
    let wrong_product = r#"
schema_version = 1
[[entries]]
product = "hermes"
os = "windows"
architecture = "x64"
enabled = false
status_reason = "fixture"
entry_urls = []
url_rules = []
package_kinds = ["msi"]
package_identity = "CC Switch"
allow_trusted_update_when_management_unknown = true
"#;
    assert!(TrustRegistry::parse(wrong_product).is_err());

    let store_distribution = r#"
schema_version = 1
[[entries]]
product = "cc_switch"
os = "windows"
architecture = "x64"
distribution = "microsoft_store"
enabled = false
status_reason = "fixture"
entry_urls = []
url_rules = []
package_kinds = ["msi"]
package_identity = "CC Switch"
allow_trusted_update_when_management_unknown = true
"#;
    assert!(TrustRegistry::parse(store_distribution).is_err());
}

#[test]
fn sequential_batch_reports_each_disabled_product_without_network_side_effects() {
    let registry = TrustRegistry::parse(
        r#"
schema_version = 1
[[entries]]
product = "work_buddy"
os = "windows"
architecture = "x64"
enabled = false
status_reason = "fixture disabled"
entry_urls = ["https://download.codebuddy.cn/workbuddy/client.exe"]
url_rules = [{ host = "download.codebuddy.cn", path_prefixes = ["/workbuddy/"] }]
package_kinds = ["exe"]

[[entries]]
product = "cc_switch"
os = "windows"
architecture = "x64"
enabled = false
status_reason = "fixture disabled"
entry_urls = ["https://dl.ccswitch.io/client.msi"]
url_rules = [{ host = "dl.ccswitch.io", exact_paths = ["/client.msi"] }]
package_kinds = ["msi"]
"#,
    )
    .unwrap();
    let platform = PlatformInfo {
        os: OperatingSystem::Windows,
        architecture: Architecture::X64,
        os_version: None,
        description: "test".into(),
    };
    let candidates = vec![
        InstallPlan::DirectPackage(ReleaseCandidate {
            product: ProductId::WorkBuddy,
            version: "1.0.0".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Exe,
            download_url: Url::parse("https://download.codebuddy.cn/workbuddy/client.exe").unwrap(),
            expected_sha256: None,
            detached_signature: None,
        }),
        InstallPlan::DirectPackage(ReleaseCandidate {
            product: ProductId::CcSwitch,
            version: "1.0.0".into(),
            architecture: Architecture::X64,
            package_kind: PackageKind::Msi,
            download_url: Url::parse("https://dl.ccswitch.io/client.msi").unwrap(),
            expected_sha256: None,
            detached_signature: Some("invalid".into()),
        }),
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
            .all(|result| result.state == easy_agent::core::OperationState::Failed)
    );
    let updates = updates.lock().unwrap();
    assert_eq!(updates.len(), 4);
    for (updates, result) in updates.chunks_exact(2).zip(&results) {
        assert_eq!(updates[0].product, result.product);
        assert_eq!(updates[0].state, easy_agent::core::OperationState::Ready);
        assert_eq!(updates[1].product, result.product);
        assert_eq!(updates[1].state, result.state);
    }
}

#[test]
fn numeric_versions_are_compared_without_lexical_errors() {
    assert!(version_is_older("3.9.0", "3.10.0"));
    assert!(!version_is_older("3.10.1", "3.10.0"));
    assert!(!version_is_older("1.2", "1.2.0"));
    assert!(!version_is_older_for_product(
        ProductId::WorkBuddy,
        "5.3.8",
        "5.3.8.34705286"
    ));
    assert!(version_is_older_for_product(
        ProductId::WorkBuddy,
        "5.3.7",
        "5.3.8.34705286"
    ));
}

#[test]
fn workbuddy_preinstall_check_accepts_the_registered_release_version_as_current() {
    let current = Detection {
        installed: true,
        version: Some("5.3.8".into()),
        managed: false,
        management_known: true,
        package_identity: None,
        package_family: None,
        publisher: Some("Tencent Technology (Shenzhen) Company Limited".into()),
        architecture: Some(Architecture::X64),
        evidence: "fixture".into(),
    };
    assert!(matches!(
        assess_existing_install_for_product(
            &current,
            ProductId::WorkBuddy,
            "5.3.8.34705286",
            false
        ),
        PreinstallDecision::AlreadyCurrent(_)
    ));
}

#[test]
fn existing_higher_or_managed_install_is_rejected_before_download() {
    let higher = Detection {
        installed: true,
        version: Some("4.0.0".into()),
        managed: false,
        management_known: true,
        package_identity: None,
        package_family: None,
        publisher: None,
        architecture: None,
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
        package_identity: None,
        package_family: None,
        publisher: None,
        architecture: None,
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
        package_identity: None,
        package_family: None,
        publisher: None,
        architecture: None,
        evidence: "fixture".into(),
    };
    assert!(matches!(
        assess_existing_install(&unknown, "2.0.0"),
        PreinstallDecision::Reject(message) if message.contains("无法确认")
    ));
    assert!(matches!(
        assess_existing_install_for_product(&unknown, ProductId::CcSwitch, "2.0.0", true),
        PreinstallDecision::Proceed
    ));
}

#[test]
fn claude_product_name_is_the_desktop_client() {
    assert_eq!(ProductId::Claude.display_name(), "Claude Desktop");
}
