use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use easy_agent::core::{
    Architecture, OperatingSystem, ProductId, TrustRegistry, verify_minisign_file,
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
    let target = arguments
        .next()
        .ok_or_else(|| usage("missing artifact path or --installed"))?;
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
    let updater_signature_verified = match trust.updater_public_key.as_deref() {
        Some(public_key) => {
            let signature = required_environment("MACOS_PROOF_SIGNATURE")?;
            verify_minisign_file(&artifact, public_key, &signature)
                .map_err(|error| format!("updater signature verification failed: {error}"))?;
            true
        }
        None => false,
    };

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
        "{message}; usage: cargo run --example macos_artifact_proof -- <product> <x64|arm64> <artifact-path|--installed>"
    )
}
