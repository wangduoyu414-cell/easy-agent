//! Read-only observations, deliberately separate from trusted installation detection.
//! No scripts, Git commands, application processes or health endpoints are invoked.
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::core::{Architecture, OperatingSystem};

const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct DesktopIdentityObservation {
    pub bundle_id: String,
    pub version: String,
    pub target_architecture_matches: bool,
    pub signature_valid: bool,
    pub signature_team: Option<String>,
    pub vendor_publisher_verified: bool,
    pub gatekeeper_checked: bool,
    pub runtime_health_checked: bool,
}

/// Inspect a local, potentially self-built desktop. This does not authorize an
/// installation: an ad-hoc signature cannot establish Nous Research provenance.
pub fn observe_desktop_identity(
    app: &Path,
    architecture: Architecture,
) -> Result<DesktopIdentityObservation, String> {
    #[cfg(target_os = "macos")]
    {
        super::macos::observe_hermes_desktop_identity(app, architecture)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, architecture);
        Err("desktop signature observation currently requires macOS".into())
    }
}

#[derive(Debug, Serialize)]
pub struct InstallationObservation {
    pub setup_present: bool,
    pub desktop_executable_present: bool,
    pub runtime_python_entry_present: bool,
    pub source_commit: Option<String>,
    pub expected_commit_matches: Option<bool>,
    pub marker_commit_matches: Option<bool>,
    pub declared_desktop_version: Option<String>,
    pub missing: Vec<&'static str>,
    pub signature_checked: bool,
    pub runtime_health_checked: bool,
    pub source_integrity_checked: bool,
}

impl InstallationObservation {
    /// A user-facing observation, never a trusted installed/success verdict.
    pub fn summary(&self) -> &'static str {
        if self.marker_commit_matches == Some(false) || self.expected_commit_matches == Some(false)
        {
            "Hermes 版本记录不一致，需检查"
        } else if self.desktop_executable_present && self.runtime_python_entry_present {
            "发现 Hermes 桌面与运行环境，尚未完成验证"
        } else if self.desktop_executable_present {
            "发现 Hermes 桌面，运行环境不完整"
        } else if self.setup_present {
            "仅发现 Hermes 安装器，桌面尚未安装"
        } else if self.source_commit.is_some() || self.runtime_python_entry_present {
            "发现 Hermes 部分安装，桌面尚未安装"
        } else {
            "未发现 Hermes 完整安装"
        }
    }
}

/// `root` is HERMES_HOME, not the source checkout. An explicit expected SHA is
/// optional; a matching HEAD alone does not verify the working tree or dependencies.
pub fn observe_installation(
    root: &Path,
    os: OperatingSystem,
    architecture: Architecture,
    expected_commit: Option<&str>,
) -> Result<InstallationObservation, String> {
    if expected_commit.is_some_and(|value| !is_commit(value)) {
        return Err("expected commit must be a full 40-character hexadecimal SHA".into());
    }
    let (setup, desktop, python) = match (os, architecture) {
        (OperatingSystem::MacOs, Architecture::Arm64) => (
            "hermes-setup",
            "hermes-agent/apps/desktop/release/mac-arm64/Hermes.app/Contents/MacOS/Hermes",
            "hermes-agent/venv/bin/python",
        ),
        (OperatingSystem::Windows, Architecture::X64) => (
            "hermes-setup.exe",
            "hermes-agent/apps/desktop/release/win-unpacked/Hermes.exe",
            "hermes-agent/venv/Scripts/python.exe",
        ),
        (OperatingSystem::Windows, Architecture::Arm64) => (
            "hermes-setup.exe",
            "hermes-agent/apps/desktop/release/win-arm64-unpacked/Hermes.exe",
            "hermes-agent/venv/Scripts/python.exe",
        ),
        _ => return Err("unsupported Hermes observation platform".into()),
    };
    let setup_present = regular_file(root, setup)?;
    let primary_desktop = regular_file(root, desktop)?;
    let alternate_desktop = os == OperatingSystem::MacOs
        && regular_file(
            root,
            "hermes-agent/apps/desktop/release/mac/Hermes.app/Contents/MacOS/Hermes",
        )?;
    if primary_desktop && alternate_desktop {
        return Err("multiple Hermes desktop build layouts found; refusing to choose one".into());
    }
    let desktop_executable_present = primary_desktop || alternate_desktop;
    // Python venv executables can be symlinks to a managed interpreter outside
    // this root. Observe the link itself without following or executing it.
    let runtime_python_entry_present = match checked_path(root, python, true)? {
        Some(path) => {
            let metadata =
                fs::symlink_metadata(path).map_err(|_| "cannot inspect runtime entry")?;
            metadata.is_file() || metadata.file_type().is_symlink()
        }
        None => false,
    };
    let source_commit = read_commit(root)?;
    let marker = read_text(root, "hermes-agent/.hermes-bootstrap-complete")?
        .map(|text| serde_json::from_str::<serde_json::Value>(&text))
        .transpose()
        .map_err(|_| "invalid bootstrap marker JSON")?;
    let marker_commit = marker
        .as_ref()
        .and_then(|value| value.get("pinnedCommit"))
        .and_then(|value| value.as_str())
        .filter(|value| is_commit(value));
    let declared_desktop_version = read_text(root, "hermes-agent/apps/desktop/package.json")?
        .map(|text| serde_json::from_str::<serde_json::Value>(&text))
        .transpose()
        .map_err(|_| "invalid desktop package JSON")?
        .and_then(|value| {
            value
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        });
    let mut missing = Vec::new();
    for (present, label) in [
        (setup_present, "setup"),
        (desktop_executable_present, "desktop_executable"),
        (runtime_python_entry_present, "runtime_python_entry"),
        (source_commit.is_some(), "source_commit"),
        (marker_commit.is_some(), "bootstrap_marker_commit"),
        (
            declared_desktop_version.is_some(),
            "declared_desktop_version",
        ),
    ] {
        if !present {
            missing.push(label);
        }
    }
    Ok(InstallationObservation {
        expected_commit_matches: expected_commit
            .zip(source_commit.as_deref())
            .map(|(expected, actual)| expected.eq_ignore_ascii_case(actual)),
        marker_commit_matches: marker_commit
            .zip(source_commit.as_deref())
            .map(|(marker, actual)| marker.eq_ignore_ascii_case(actual)),
        setup_present,
        desktop_executable_present,
        runtime_python_entry_present,
        source_commit,
        declared_desktop_version,
        missing,
        signature_checked: false,
        runtime_health_checked: false,
        source_integrity_checked: false,
    })
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read_commit(root: &Path) -> Result<Option<String>, String> {
    let Some(head) = read_text(root, "hermes-agent/.git/HEAD")? else {
        return Ok(None);
    };
    let head = head.trim();
    if is_commit(head) {
        return Ok(Some(head.to_ascii_lowercase()));
    }
    let reference = head.strip_prefix("ref: ").ok_or("invalid Git HEAD")?;
    if !reference.starts_with("refs/heads/")
        || reference
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || reference.contains(['\\', ':'])
    {
        return Err("unsupported or unsafe Git HEAD reference".into());
    }
    if let Some(commit) = read_text(root, &format!("hermes-agent/.git/{reference}"))? {
        return if is_commit(commit.trim()) {
            Ok(Some(commit.trim().to_ascii_lowercase()))
        } else {
            Err("invalid Git reference commit".into())
        };
    }
    if let Some(refs) = read_text(root, "hermes-agent/.git/packed-refs")? {
        for line in refs.lines() {
            if let Some((commit, name)) = line.split_once(' ')
                && name == reference
            {
                return if is_commit(commit) {
                    Ok(Some(commit.to_ascii_lowercase()))
                } else {
                    Err("invalid packed Git reference commit".into())
                };
            }
        }
    }
    Ok(None)
}

fn regular_file(root: &Path, relative: &str) -> Result<bool, String> {
    Ok(checked_path(root, relative, false)?.is_some_and(|path| path.is_file()))
}

pub(super) fn checked_path(
    root: &Path,
    relative: &str,
    allow_leaf_link: bool,
) -> Result<Option<PathBuf>, String> {
    // Reject a symlink root and child components. Ancestors of a caller-selected
    // root may be system aliases; normalize those once without inspecting outside it.
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        _ => return Err("Hermes root must be a readable directory, not a symlink".into()),
    }
    let mut path = fs::canonicalize(root).map_err(|_| "cannot resolve Hermes root")?;
    let components: Vec<_> = Path::new(relative).components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err("unsafe metadata path".into());
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if !(allow_leaf_link && index + 1 == components.len()) {
                    return Err("refusing linked Hermes metadata or executable".into());
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("cannot inspect Hermes path".into()),
        }
    }
    Ok(Some(path))
}

fn read_text(root: &Path, relative: &str) -> Result<Option<String>, String> {
    let Some(path) = checked_path(root, relative, false)? else {
        return Ok(None);
    };
    let initial = fs::symlink_metadata(&path).map_err(|_| "cannot inspect Hermes metadata")?;
    if !initial.is_file() || initial.len() > MAX_METADATA_BYTES {
        return Err("Hermes metadata must be a bounded regular file".into());
    }
    let file = fs::File::open(path).map_err(|_| "cannot open Hermes metadata")?;
    let metadata = file
        .metadata()
        .map_err(|_| "cannot inspect Hermes metadata")?;
    if !metadata.is_file() || metadata.len() > MAX_METADATA_BYTES {
        return Err("Hermes metadata must be a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read Hermes metadata")?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err("Hermes metadata exceeds size limit".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "Hermes metadata is not UTF-8".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    const COMMIT: &str = "29112bef099274229cadff79cdff7bf7b99c4b77";

    fn put(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn probe(root: &Path) -> Result<InstallationObservation, String> {
        observe_installation(
            root,
            OperatingSystem::MacOs,
            Architecture::Arm64,
            Some(COMMIT),
        )
    }

    #[test]
    fn absent_and_setup_only_are_not_complete_installations() {
        let dir = tempfile::tempdir().unwrap();
        let missing = probe(&dir.path().join("absent")).unwrap();
        assert!(!missing.setup_present);
        assert_eq!(missing.expected_commit_matches, None);
        put(dir.path(), "hermes-setup", "not a verified executable");
        let setup = probe(dir.path()).unwrap();
        assert!(setup.setup_present);
        assert!(setup.missing.contains(&"desktop_executable"));
        assert!(!setup.signature_checked);
        assert!(!setup.runtime_health_checked);
        assert_eq!(setup.summary(), "仅发现 Hermes 安装器，桌面尚未安装");
    }

    #[test]
    fn complete_layout_is_only_an_observation_and_marker_can_disagree() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "hermes-setup",
            "hermes-agent/apps/desktop/release/mac-arm64/Hermes.app/Contents/MacOS/Hermes",
            "hermes-agent/venv/bin/python",
        ] {
            put(dir.path(), path, "stub");
        }
        put(dir.path(), "hermes-agent/.git/HEAD", COMMIT);
        put(
            dir.path(),
            "hermes-agent/apps/desktop/package.json",
            r#"{"version":"0.21.0"}"#,
        );
        put(
            dir.path(),
            "hermes-agent/.hermes-bootstrap-complete",
            &format!(r#"{{"pinnedCommit":"{}"}}"#, "f".repeat(40)),
        );
        let report = probe(dir.path()).unwrap();
        assert!(report.missing.is_empty());
        assert_eq!(report.expected_commit_matches, Some(true));
        assert_eq!(report.marker_commit_matches, Some(false));
        assert_eq!(report.summary(), "Hermes 版本记录不一致，需检查");
        assert!(!report.source_integrity_checked);
        assert!(!report.signature_checked);
        assert!(!report.runtime_health_checked);
    }

    #[test]
    fn loose_and_packed_refs_resolve_but_traversal_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "hermes-agent/.git/HEAD",
            "ref: refs/heads/main\n",
        );
        put(
            dir.path(),
            "hermes-agent/.git/packed-refs",
            &format!("# packed refs\n{COMMIT} refs/heads/main\n"),
        );
        assert_eq!(
            probe(dir.path()).unwrap().source_commit.as_deref(),
            Some(COMMIT)
        );
        put(
            dir.path(),
            "hermes-agent/.git/refs/heads/main",
            &"a".repeat(40),
        );
        assert_eq!(
            probe(dir.path()).unwrap().expected_commit_matches,
            Some(false)
        );
        put(
            dir.path(),
            "hermes-agent/.git/HEAD",
            "ref: refs/heads/../../../outside",
        );
        assert!(probe(dir.path()).is_err());
    }

    #[test]
    fn windows_layouts_are_architecture_specific() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "hermes-setup.exe", "stub");
        put(
            dir.path(),
            "hermes-agent/apps/desktop/release/win-unpacked/Hermes.exe",
            "stub",
        );
        let x64 = observe_installation(
            dir.path(),
            OperatingSystem::Windows,
            Architecture::X64,
            None,
        )
        .unwrap();
        let arm = observe_installation(
            dir.path(),
            OperatingSystem::Windows,
            Architecture::Arm64,
            None,
        )
        .unwrap();
        assert!(x64.desktop_executable_present);
        assert!(!arm.desktop_executable_present);
    }

    #[test]
    fn mac_alternate_layout_is_observed_but_multiple_layouts_are_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "hermes-agent/apps/desktop/release/mac/Hermes.app/Contents/MacOS/Hermes",
            "stub",
        );
        assert!(probe(dir.path()).unwrap().desktop_executable_present);
        put(
            dir.path(),
            "hermes-agent/apps/desktop/release/mac-arm64/Hermes.app/Contents/MacOS/Hermes",
            "stub",
        );
        assert!(probe(dir.path()).is_err());
    }

    #[test]
    fn invalid_expected_sha_and_oversized_or_malformed_metadata_fail() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            observe_installation(
                dir.path(),
                OperatingSystem::MacOs,
                Architecture::Arm64,
                Some("main")
            )
            .is_err()
        );
        put(
            dir.path(),
            "hermes-agent/.git/HEAD",
            &"a".repeat(MAX_METADATA_BYTES as usize + 1),
        );
        assert!(probe(dir.path()).is_err());
        put(dir.path(), "hermes-agent/.git/HEAD", COMMIT);
        put(
            dir.path(),
            "hermes-agent/.hermes-bootstrap-complete",
            "not JSON",
        );
        assert!(probe(dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn linked_metadata_is_rejected_and_python_link_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("hermes-agent/venv/bin")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("absent-python"),
            dir.path().join("hermes-agent/venv/bin/python"),
        )
        .unwrap();
        // A dangling venv link is observed, never treated as a healthy runtime.
        let report = probe(dir.path()).unwrap();
        assert!(report.runtime_python_entry_present);
        assert!(!report.runtime_health_checked);
        std::os::unix::fs::symlink(outside.path(), dir.path().join("hermes-agent/.git")).unwrap();
        assert!(probe(dir.path()).is_err());
    }
}
