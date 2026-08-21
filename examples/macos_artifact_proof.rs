use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use easy_agent::adapters::resolve_install_plan;
use easy_agent::core::{
    Architecture, ArtifactSource, DownloadRequest, InstallPlan, OperatingSystem, PlatformInfo,
    ProductId, RemoteDigestPolicy, TrustRegistry, download_to_private_staging,
    verify_configured_updater_signature_file, version_is_older_for_product,
};
use easy_agent::platform;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("macOS artifact proof failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("this evidence probe must run on macOS".into());
    }

    let mut arguments = env::args().skip(1);
    let product = parse_product(&arguments.next().ok_or_else(|| usage("missing product"))?)?;
    let architecture = parse_architecture(
        &arguments
            .next()
            .ok_or_else(|| usage("missing architecture"))?,
    )?;
    let target = arguments.next().ok_or_else(|| {
        usage("missing artifact path, --installed, --preflight, or --download-verify")
    })?;
    if arguments.next().is_some() {
        return Err(usage("unexpected extra argument"));
    }

    let registry = TrustRegistry::embedded().map_err(|error| error.to_string())?;
    let mut trust = registry
        .find(product, OperatingSystem::MacOs, architecture)
        .cloned()
        .ok_or_else(|| "the embedded registry has no matching macOS entry".to_owned())?;

    if target == "--installed" {
        let detection = platform::detect_product(product, Some(&trust));
        println!("product={}", product.key());
        println!("architecture={}", architecture.key());
        println!("installed={}", detection.installed);
        println!(
            "version={}",
            detection.version.as_deref().unwrap_or("<unknown>")
        );
        println!(
            "bundle_id={}",
            detection.package_identity.as_deref().unwrap_or("<none>")
        );
        println!(
            "team_id={}",
            detection.publisher.as_deref().unwrap_or("<none>")
        );
        println!("evidence={}", detection.evidence);
        return Ok(());
    }
    if target == "--preflight" {
        platform::preflight_direct_install(&trust, architecture)?;
        println!("product={}", product.key());
        println!("architecture={}", architecture.key());
        println!("preflight=ready");
        return Ok(());
    }
    if target == "--download-verify" {
        let current = platform::current_platform();
        let platform = PlatformInfo {
            os: OperatingSystem::MacOs,
            architecture,
            os_version: current.os_version,
            description: "macOS artifact proof".into(),
        };
        let InstallPlan::DirectPackage(candidate) =
            resolve_install_plan(product, &platform, &registry)
                .map_err(|error| error.to_string())?
        else {
            return Err("the product did not resolve to a direct package".into());
        };
        let file_name = format!(
            "{}-proof.{}",
            product.key(),
            candidate.package_kind.extension()
        );
        let download_url_rules = match candidate.source {
            ArtifactSource::Official => &trust.url_rules,
            ArtifactSource::VerifiedMirror { .. } => &trust.mirror_url_rules,
        };
        let download = download_to_private_staging(&DownloadRequest {
            url: candidate.download_url.clone(),
            file_name,
            url_rules: download_url_rules,
            expected_size: candidate.expected_size,
        })
        .map_err(|error| format!("artifact download failed: {error}"))?;
        let remote_digest_matches = candidate
            .expected_sha256
            .as_deref()
            .map(|expected| download.identity.sha256.eq_ignore_ascii_case(expected));
        if remote_digest_matches == Some(false)
            && trust.remote_digest_policy == RemoteDigestPolicy::EnforceIfPresent
        {
            return Err("official digest does not match the downloaded artifact".into());
        }
        let signature_verified = verify_configured_updater_signature_file(
            &download.staged_path,
            trust.updater_public_key.as_deref(),
            trust.sparkle_ed25519_public_key.as_deref(),
            candidate.detached_signature.as_deref(),
        )
        .map_err(|error| format!("updater signature verification failed: {error}"))?;
        let verification = platform::verify_artifact(
            &download.staged_path,
            candidate.package_kind,
            &trust,
            architecture,
            signature_verified,
        )?;
        let artifact_version = verification
            .version
            .as_deref()
            .ok_or_else(|| "artifact has no verifiable version".to_owned())?;
        let versions_match =
            !version_is_older_for_product(product, artifact_version, &candidate.version)
                && !version_is_older_for_product(product, &candidate.version, artifact_version);
        if !versions_match {
            return Err(format!(
                "artifact version mismatch: expected {}, got {:?}",
                candidate.version, verification.version
            ));
        }
        println!("product={}", product.key());
        println!("architecture={}", architecture.key());
        println!("version={}", candidate.version);
        println!("sha256={}", download.identity.sha256);
        println!(
            "remote_digest_matches={}",
            remote_digest_matches
                .map(|matches| matches.to_string())
                .unwrap_or_else(|| "not_provided".into())
        );
        println!("updater_signature_verified={signature_verified}");
        println!("platform_verification=passed");
        println!("bundle_id={}", verification.product_identity);
        println!(
            "team_id={}",
            verification.signer_subject.as_deref().unwrap_or("<none>")
        );
        return Ok(());
    }

    let artifact = PathBuf::from(target);

    override_if_present(
        "MACOS_PROOF_APPLICATION_NAME",
        &mut trust.macos_application_name,
    )?;
    override_if_present("MACOS_PROOF_BUNDLE_ID", &mut trust.macos_bundle_id)?;
    override_if_present("MACOS_PROOF_TEAM_ID", &mut trust.macos_team_id)?;

    let package_kind = match trust.package_kinds.as_slice() {
        [kind] => *kind,
        [] => return Err("the trust entry has no package kind".into()),
        _ => return Err("the trust entry has multiple package kinds".into()),
    };
    let signature = (trust.updater_public_key.is_some()
        || trust.sparkle_ed25519_public_key.is_some())
    .then(|| required_environment("MACOS_PROOF_SIGNATURE"))
    .transpose()?;
    let updater_signature_verified = verify_configured_updater_signature_file(
        &artifact,
        trust.updater_public_key.as_deref(),
        trust.sparkle_ed25519_public_key.as_deref(),
        signature.as_deref(),
    )
    .map_err(|error| format!("updater signature verification failed: {error}"))?;

    let verification = platform::verify_artifact(
        &artifact,
        package_kind,
        &trust,
        architecture,
        updater_signature_verified,
    )?;

    println!("product={}", product.key());
    println!("architecture={}", architecture.key());
    println!("package_kind={}", package_kind.extension());
    println!("bundle_id={}", verification.product_identity);
    println!(
        "team_id={}",
        verification.signer_subject.as_deref().unwrap_or("<none>")
    );
    println!(
        "version={}",
        verification.version.as_deref().unwrap_or("<unknown>")
    );
    Ok(())
}

fn parse_product(value: &str) -> Result<ProductId, String> {
    match value {
        "workbuddy" | "work_buddy" => Ok(ProductId::WorkBuddy),
        "hermes" => Ok(ProductId::Hermes),
        "cc_switch" | "cc-switch" => Ok(ProductId::CcSwitch),
        "claude" => Ok(ProductId::Claude),
        "chatgpt" | "chat_gpt" | "chat-gpt" => Ok(ProductId::ChatGpt),
        _ => Err(usage("unknown product")),
    }
}

fn parse_architecture(value: &str) -> Result<Architecture, String> {
    match value {
        "x64" | "x86_64" => Ok(Architecture::X64),
        "arm64" | "aarch64" => Ok(Architecture::Arm64),
        _ => Err(usage("unknown architecture")),
    }
}

fn override_if_present(name: &str, target: &mut Option<String>) -> Result<(), String> {
    let Ok(value) = env::var(name) else {
        return Ok(());
    };
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    *target = Some(value.to_owned());
    Ok(())
}

fn required_environment(name: &str) -> Result<String, String> {
    let value = env::var(name).map_err(|_| format!("set {name} for this signed updater"))?;
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    Ok(value.to_owned())
}

fn usage(message: &str) -> String {
    format!(
        "{message}; usage: cargo run --example macos_artifact_proof -- <product> <x64|arm64> <artifact-path|--installed|--preflight|--download-verify>"
    )
}
