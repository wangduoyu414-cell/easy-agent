//! Save official Mac installers. This path never executes a package.
use crate::core::*;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use url::Url;

struct Asset {
    url: Url,
    name: String,
    size: Option<u64>,
    digest: Option<String>,
    version: Option<String>,
}

fn clash_asset(body: &str, arch: Architecture) -> Result<Asset, String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        draft: bool,
        prerelease: bool,
        assets: Vec<File>,
    }
    #[derive(Deserialize)]
    struct File {
        name: String,
        size: u64,
        digest: Option<String>,
        browser_download_url: String,
    }
    let r: Release = serde_json::from_str(body).map_err(|_| "Clash 发布清单格式有误")?;
    let v = r.tag_name.strip_prefix('v').ok_or("缺少版本标签")?;
    if r.draft
        || r.prerelease
        || v.len() > 32
        || v.split('.').count() != 3
        || !v
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("不是 Clash 稳定版本".into());
    }
    let cpu = match arch {
        Architecture::Arm64 => "aarch64",
        Architecture::X64 => "x64",
        _ => return Err("不支持的芯片".into()),
    };
    let name = format!("Clash.Verge_{v}_{cpu}.dmg");
    let expected =
        format!("https://github.com/clash-verge-rev/clash-verge-rev/releases/download/v{v}/{name}");
    let mut matches = r.assets.into_iter().filter(|a| a.name == name);
    let a = matches.next().ok_or("官方未提供当前架构的 DMG")?;
    let sha = a
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("官方文件缺少 SHA-256")?;
    if matches.next().is_some()
        || a.browser_download_url != expected
        || a.size == 0
        || a.size > 2 * 1024 * 1024 * 1024
        || sha.len() != 64
        || !sha.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Clash 安装包清单不匹配".into());
    }
    Ok(Asset {
        url: Url::parse(&expected).map_err(|e| e.to_string())?,
        name,
        size: Some(a.size),
        digest: Some(sha.to_ascii_lowercase()),
        version: Some(v.into()),
    })
}

pub(crate) fn clash_candidate(
    body: &str,
    architecture: Architecture,
) -> Result<ReleaseCandidate, String> {
    let a = clash_asset(body, architecture)?;
    Ok(ReleaseCandidate {
        product: ProductId::ClashVergeRev,
        architecture,
        version: a.version.ok_or("缺少版本")?,
        package_kind: PackageKind::Dmg,
        download_url: a.url,
        source: ArtifactSource::Official,
        minimum_macos_version: None,
        expected_size: a.size,
        expected_sha256: a.digest,
        detached_signature: None,
        bootstrap_payload: None,
    })
}
fn hermes_asset(body: &str) -> Result<Asset, String> {
    let re=regex::Regex::new(r#"href=["'](https://hermes-assets\.nousresearch\.com/Hermes-Setup\.dmg(?:\?build=[0-9A-Za-z._-]{1,80})?)["']"#).expect("static Hermes link pattern");
    let link = re.captures(body).ok_or("官网未提供 Apple Silicon 安装器")?;
    Ok(Asset {
        url: Url::parse(&link[1]).map_err(|e| e.to_string())?,
        name: "Hermes-Setup-arm64.dmg".into(),
        size: None,
        digest: None,
        version: None,
    })
}

pub fn save(
    product: ProductId,
    arch: Architecture,
    cancel: &AtomicBool,
    notify: impl Fn(OperationUpdate),
) -> Result<PathBuf, String> {
    run(product, arch, cancel, notify, false)
}
pub(crate) fn start_hermes(
    arch: Architecture,
    cancel: &AtomicBool,
    notify: impl Fn(OperationUpdate),
) -> Result<PathBuf, String> {
    run(ProductId::Hermes, arch, cancel, notify, true)
}
fn run(
    product: ProductId,
    arch: Architecture,
    cancel: &AtomicBool,
    notify: impl Fn(OperationUpdate),
    launch: bool,
) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("需要 macOS".into());
    }
    let registry = TrustRegistry::embedded().map_err(|e| e.to_string())?;
    let t = registry
        .find(product, OperatingSystem::MacOs, arch)
        .filter(|t| {
            t.download_only || (product == ProductId::ClashVergeRev && t.enabled && !launch)
        })
        .ok_or("无适用的官方下载包")?;
    let emit = |state, message: String| {
        notify(OperationUpdate {
            product,
            state,
            message,
        })
    };
    emit(OperationState::Downloading, "正在查找官方安装包".into());
    let (_, body) = fetch_official_text(
        &safe_http_client().map_err(|e| e.to_string())?,
        &Url::parse(&t.entry_urls[0]).map_err(|e| e.to_string())?,
        t,
    )
    .map_err(|e| e.to_string())?;
    let a = match product {
        ProductId::ClashVergeRev => clash_asset(&body, arch)?,
        ProductId::Hermes => hermes_asset(&body)?,
        _ => return Err("不支持的产品".into()),
    };
    let cancelled = || cancel.load(Ordering::Relaxed);
    let d = download_to_private_staging_controlled(
        &DownloadRequest {
            url: a.url,
            file_name: a.name,
            url_rules: &t.url_rules,
            expected_size: a.size,
        },
        &DownloadControl {
            is_cancelled: &cancelled,
            on_progress: &|bytes, total| {
                emit(
                    OperationState::Downloading,
                    match total.filter(|t| *t > 0) {
                        Some(t) => format!("已下载 {:.1}%", 100.0 * bytes as f64 / t as f64),
                        None => format!("已下载 {} MiB", bytes / 1048576),
                    },
                )
            },
        },
    )
    .map_err(|e| e.to_string())?;
    if cancelled() {
        return Err("下载已取消".into());
    }
    emit(
        OperationState::Verifying,
        "正在验证安装器身份、签名和架构".into(),
    );
    if a.digest
        .as_ref()
        .is_some_and(|sha| *sha != d.identity.sha256)
    {
        return Err("官方 SHA-256 不匹配".into());
    }
    let verified =
        crate::platform::verify_artifact(&d.staged_path, PackageKind::Dmg, t, arch, false)?;
    if a.version.is_some() && verified.version != a.version {
        return Err("包内版本与发布清单不符".into());
    }
    if cancelled() {
        return Err("下载已取消".into());
    }
    let visible = save_verified_download_copy(&d, &crate::platform::downloads_directory()?)
        .map_err(|e| e.to_string())?;
    if launch {
        if cancelled() {
            return Err("已取消，未打开安装器".into());
        }
        emit(
            OperationState::AwaitingUserInstall,
            "请在官方 Hermes 安装向导中确认；助手不会代为确认或关闭向导".into(),
        );
        #[cfg(target_os = "macos")]
        crate::platform::launch_hermes_setup(&d, t, arch)?;
    }
    Ok(visible)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn release_asset_is_bound_to_architecture_digest_and_stable_tag() {
        let name = "Clash.Verge_2.5.10_aarch64.dmg";
        let file = json!({"name":name,"size":100,"digest":format!("sha256:{}","ab".repeat(32)),"browser_download_url":format!("https://github.com/clash-verge-rev/clash-verge-rev/releases/download/v2.5.10/{name}")});
        let release =
            json!({"tag_name":"v2.5.10","draft":false,"prerelease":false,"assets":[file.clone()]});
        let parse = |r: &serde_json::Value| clash_asset(&r.to_string(), Architecture::Arm64);
        assert!(parse(&release).is_ok());
        assert!(clash_asset(&release.to_string(), Architecture::X64).is_err());
        for (key, val) in [
            ("prerelease", json!(true)),
            ("tag_name", json!("v2.5.11")),
            ("assets", json!([file.clone(), file])),
        ] {
            let mut bad = release.clone();
            bad[key] = val;
            assert!(parse(&bad).is_err());
        }
        for (key, val) in [
            ("digest", json!(null)),
            (
                "browser_download_url",
                json!("https://example.org/other.dmg"),
            ),
        ] {
            let mut bad = release.clone();
            bad["assets"][0][key] = val;
            assert!(parse(&bad).is_err());
        }
    }
    #[test]
    fn hermes_requires_the_official_mac_setup_link() {
        let url = "https://hermes-assets.nousresearch.com/Hermes-Setup.dmg?build=7215d9493b76";
        assert_eq!(
            hermes_asset(&format!(r#"<a href="{url}">Mac OS</a>"#))
                .unwrap()
                .url
                .as_str(),
            url
        );
        for bad in [
            url.replace("https:", "http:"),
            url.replace(".com/", ".com.evil/"),
            url.replace(".dmg", ".exe"),
            format!("{url}&redirect=other"),
        ] {
            assert!(hermes_asset(&format!(r#"<a href="{bad}">Mac OS</a>"#)).is_err());
        }
    }
}
