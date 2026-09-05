//! Read-only audit of the embedded four-platform distribution matrix.
//! Resolves metadata using production rules; never downloads or installs apps.
use easy_agent::adapters::{resolve_install_plan, resolve_verified_download_fallback};
use easy_agent::core::{
    Architecture, InstallPlan, OperatingSystem, PlatformInfo, ProductId, TrustRegistry,
    inspect_staged_file, verify_configured_updater_signature_file,
};
use serde_json::json;

fn main() {
    let registry = TrustRegistry::embedded().expect("valid embedded registry");
    for product in ProductId::ALL {
        std::thread::scope(|scope| {
            let mut jobs = Vec::new();
            for os in [OperatingSystem::Windows, OperatingSystem::MacOs] {
                for architecture in [Architecture::X64, Architecture::Arm64] {
                    let registry = &registry;
                    jobs.push(scope.spawn(move || {
                        let trust = registry.find(product, os, architecture).unwrap();
                        let mut row = json!({
                            "product": product.key(), "os": os.key(),
                            "architecture": architecture.key(), "enabled": trust.enabled,
                        });
                        if !trust.enabled {
                            row["status"] = json!("disabled");
                            row["reason"] = json!(trust.status_reason);
                            return row;
                        }
                        let platform = PlatformInfo {
                            os,
                            architecture,
                            os_version: match os {
                                OperatingSystem::Windows => Some("10.0.26100".into()),
                                OperatingSystem::MacOs => Some("26.6.2".into()),
                                OperatingSystem::Unsupported => None,
                            },
                            description: "read-only distribution audit".into(),
                        };
                        match resolve_install_plan(product, &platform, registry) {
                            Ok(InstallPlan::DirectPackage(candidate)) => {
                                let mut url = candidate.download_url.clone();
                                url.set_query(None);
                                url.set_fragment(None);
                                row["status"] = json!("metadata_resolved");
                                row["version"] = json!(candidate.version);
                                row["package"] = json!(candidate.package_kind.extension());
                                row["artifact_url_without_query"] = json!(url.as_str());
                                row["source"] = json!(format!("{:?}", candidate.source));
                                row["has_digest"] = json!(candidate.expected_sha256.is_some());
                                row["has_updater_signature"] =
                                    json!(candidate.detached_signature.is_some());
                                if let Some(root) = std::env::var_os("EASY_AGENT_AUDIT_ARTIFACT_ROOT") {
                                    let root = std::path::PathBuf::from(root);
                                    let file = root.join(format!("{}-{}-{}.{}", product.key(), os.key(), architecture.key(), candidate.package_kind.extension()));
                                    if file.is_file() {
                                        let verification = inspect_staged_file(&root, &file)
                                            .map_err(|error| error.to_string())
                                            .and_then(|identity| {
                                                let signature = verify_configured_updater_signature_file(&file, trust.updater_public_key.as_deref(), trust.sparkle_ed25519_public_key.as_deref(), candidate.detached_signature.as_deref()).map_err(|error| error.to_string())?;
                                                Ok(json!({"sha256": identity.sha256, "bytes": identity.length, "updater_signature_verified": signature,
                                                    "remote_digest_matches": candidate.expected_sha256.as_deref().map(|expected| expected.eq_ignore_ascii_case(&identity.sha256)),
                                                    "platform_signature_checked": false}))
                                            });
                                        row["local_artifact"] = match verification {
                                            Ok(result) => result,
                                            Err(error) => json!({"error": error}),
                                        };
                                    }
                                }
                                if std::env::var("EASY_AGENT_AUDIT_FALLBACK").as_deref() == Ok("1")
                                    && trust.mirror_manifest_url.is_some()
                                    && !candidate.source.is_verified_mirror()
                                {
                                    row["fallback"] = match resolve_verified_download_fallback(&candidate, &platform, registry) {
                                        Ok(fallback) => {
                                            let mut url = fallback.download_url;
                                            url.set_query(None);
                                            url.set_fragment(None);
                                            json!({"status": "signed_manifest_matches_primary", "version": fallback.version, "sha256": fallback.expected_sha256,
                                                "artifact_url_without_query": url.as_str()})
                                        }
                                        Err(error) => json!({"status": "failed", "error": sanitized_error(&error.to_string())}),
                                    };
                                }
                            }
                            Ok(InstallPlan::MicrosoftStore(plan)) => {
                                row["status"] = json!("store_plan_only");
                                row["store_id"] = json!(plan.store_id);
                            }
                            Err(error) => {
                                row["status"] = json!("failed");
                                row["error"] = json!(sanitized_error(&error.to_string()));
                            }
                        }
                        row
                    }));
                }
            }
            for job in jobs {
                println!("{}", job.join().expect("audit worker"));
            }
        });
    }
}

fn sanitized_error(message: &str) -> String {
    // Network errors can contain signed URL query parameters.
    let urls = regex::Regex::new(r#"(?i)https?://[^\s\"'<>]+"#).expect("static URL pattern");
    urls.replace_all(message, "[URL redacted]").into_owned()
}
