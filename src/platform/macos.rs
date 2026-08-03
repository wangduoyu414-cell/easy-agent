use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use flate2::read::GzDecoder;
use plist::Value;
use tar::Archive;
use tempfile::Builder;
use zip::ZipArchive;

use crate::core::{
    Architecture, Detection, PackageKind, ProductId, TrustEntry, verify_minisign_file,
    verify_staged_identity,
};

use super::{ArtifactVerification, InstallerExecution, PlannedCommand, VerifiedInstallRequest};

const MAX_ARCHIVE_ENTRIES: usize = 200_000;
const MAX_EXPANDED_BYTES: u64 = 12 * 1024 * 1024 * 1024;
const MAX_PLIST_BYTES: u64 = 4 * 1024 * 1024;

pub fn hardware_architecture() -> Architecture {
    let arm64_hardware = command_output("/usr/sbin/sysctl", &["-n", "hw.optional.arm64"])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "1")
        .unwrap_or(false);
    if arm64_hardware {
        Architecture::Arm64
    } else if cfg!(target_arch = "x86_64") {
        Architecture::X64
    } else if cfg!(target_arch = "aarch64") {
        Architecture::Arm64
    } else {
        Architecture::Unsupported
    }
}

pub fn operating_system_version() -> Option<String> {
    let output = command_output("/usr/bin/sw_vers", &["-productVersion"]).ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!version.is_empty()).then_some(version)
}

pub fn detect_product(product: ProductId, trust: Option<&TrustEntry>) -> Result<Detection, String> {
    let Some(trust) = trust else {
        return Ok(Detection::absent(format!(
            "{} 没有当前 Mac 的信任策略",
            product.display_name()
        )));
    };
    let Some(application_name) = trust.macos_application_name.as_deref() else {
        return Ok(Detection::absent("macOS 应用名尚未固定"));
    };
    if trust.macos_bundle_id.is_none() {
        return Ok(Detection::absent("macOS Bundle ID 尚未固定"));
    }

    let candidates = application_candidates(application_name)?;
    let mut found = Vec::new();
    for (path, label) in candidates {
        if !path.exists() {
            continue;
        }
        let inspection = inspect_app_bundle(&path, trust, trust.architecture)?;
        found.push((path, label, inspection));
    }
    if found.is_empty() {
        return Ok(Detection::absent(
            "未在用户或系统 Applications 中发现精确 Bundle ID",
        ));
    }
    if found.len() > 1 {
        return Err(format!(
            "同时发现用户级和系统级 {}，拒绝猜测应更新哪一份",
            application_name
        ));
    }
    let (_, label, inspection) = found.remove(0);
    Ok(Detection {
        installed: true,
        version: Some(inspection.version),
        managed: false,
        management_known: true,
        package_identity: Some(inspection.bundle_id),
        package_family: None,
        publisher: inspection.team_id,
        architecture: Some(trust.architecture),
        evidence: format!("{label} · 已通过 Bundle/签名/Gatekeeper 检查"),
    })
}

pub fn verify_artifact(
    path: &Path,
    kind: PackageKind,
    trust: &TrustEntry,
    expected_architecture: Architecture,
    updater_signature_verified: bool,
) -> Result<ArtifactVerification, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("cannot stat artifact: {error}"))?;
    if !metadata.is_file() {
        return Err("artifact is not a regular file".into());
    }
    if trust.updater_public_key.is_some() && !updater_signature_verified {
        return Err("the configured updater signature was not verified".into());
    }
    with_prepared_app(path, kind, trust, |app| {
        let inspection = inspect_app_bundle(app, trust, expected_architecture)?;
        Ok(ArtifactVerification {
            signer_subject: inspection.team_id,
            product_identity: inspection.bundle_id,
            version: Some(inspection.version),
            architecture: Some(expected_architecture),
        })
    })
}

pub fn plan_install_command(_path: &Path, kind: PackageKind) -> Result<PlannedCommand, String> {
    Err(format!(
        "macOS {kind:?} installation uses the internal verified app-bundle copy path"
    ))
}

pub fn execute_verified_installer(
    request: &VerifiedInstallRequest<'_>,
) -> Result<InstallerExecution, String> {
    if !request.trust.enabled {
        return Err(format!(
            "installation is disabled by the embedded trust registry: {}",
            request.trust.status_reason
        ));
    }
    verify_staged_identity(
        request.private_root,
        request.path,
        request.verified_identity,
        request.expected_sha256,
    )
    .map_err(|error| format!("verified artifact changed before installation: {error}"))?;
    let updater_signature_verified = match (
        request.trust.updater_public_key.as_deref(),
        request.detached_signature,
    ) {
        (Some(public_key), Some(signature)) => {
            verify_minisign_file(request.path, public_key, signature).map_err(|error| {
                format!("updater signature changed before installation: {error}")
            })?;
            true
        }
        (None, None) => false,
        _ => return Err("updater public key and detached signature are incomplete".into()),
    };
    verify_artifact(
        request.path,
        request.kind,
        request.trust,
        request.expected_architecture,
        updater_signature_verified,
    )?;
    verify_staged_identity(
        request.private_root,
        request.path,
        request.verified_identity,
        request.expected_sha256,
    )
    .map_err(|error| format!("artifact changed after platform verification: {error}"))?;

    let target = select_install_target(request.trust, request.expected_architecture)?;
    with_prepared_app(request.path, request.kind, request.trust, |source_app| {
        inspect_app_bundle(source_app, request.trust, request.expected_architecture)?;
        install_app_bundle(
            source_app,
            &target,
            request.trust,
            request.expected_architecture,
        )
    })?;

    Ok(InstallerExecution {
        exit_code: 0,
        error_summary: None,
    })
}

#[derive(Debug)]
struct AppInspection {
    bundle_id: String,
    version: String,
    team_id: Option<String>,
}

fn inspect_app_bundle(
    app: &Path,
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<AppInspection, String> {
    let metadata = fs::symlink_metadata(app)
        .map_err(|error| format!("cannot inspect app bundle {}: {error}", app.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{} is not a regular app bundle directory",
            app.display()
        ));
    }
    let canonical_app = fs::canonicalize(app)
        .map_err(|error| format!("cannot canonicalize app bundle: {error}"))?;
    let info_path = app.join("Contents").join("Info.plist");
    let info_link_metadata = fs::symlink_metadata(&info_path)
        .map_err(|error| format!("cannot inspect {}: {error}", info_path.display()))?;
    if info_link_metadata.file_type().is_symlink() {
        return Err("Info.plist must not be a symlink".into());
    }
    let canonical_info = fs::canonicalize(&info_path)
        .map_err(|error| format!("cannot canonicalize Info.plist: {error}"))?;
    if !canonical_info.starts_with(&canonical_app) {
        return Err("Info.plist escapes the app bundle".into());
    }
    let info_metadata = fs::metadata(&info_path)
        .map_err(|error| format!("cannot read {}: {error}", info_path.display()))?;
    if !info_metadata.is_file() || info_metadata.len() > MAX_PLIST_BYTES {
        return Err("Info.plist is absent, not regular, or too large".into());
    }
    let value =
        Value::from_file(&info_path).map_err(|error| format!("invalid app Info.plist: {error}"))?;
    let dictionary = value
        .as_dictionary()
        .ok_or_else(|| "Info.plist root is not a dictionary".to_owned())?;
    let bundle_id = plist_string(dictionary, "CFBundleIdentifier")?;
    let expected_bundle_id = trust
        .macos_bundle_id
        .as_deref()
        .ok_or_else(|| "trust registry has no pinned macOS Bundle ID".to_owned())?;
    if bundle_id != expected_bundle_id {
        return Err(format!(
            "Bundle ID mismatch: expected {expected_bundle_id}, got {bundle_id}"
        ));
    }
    let version = dictionary
        .get("CFBundleShortVersionString")
        .and_then(Value::as_string)
        .or_else(|| dictionary.get("CFBundleVersion").and_then(Value::as_string))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "app bundle version is absent".to_owned())?
        .to_owned();
    let executable_name = plist_string(dictionary, "CFBundleExecutable")?;
    if !safe_single_file_name(executable_name) {
        return Err("CFBundleExecutable is not a safe single file name".into());
    }
    let executable = app.join("Contents").join("MacOS").join(executable_name);
    let executable_metadata = fs::symlink_metadata(&executable)
        .map_err(|error| format!("cannot inspect main executable: {error}"))?;
    if executable_metadata.file_type().is_symlink() || !executable_metadata.is_file() {
        return Err("main executable is not a regular file".into());
    }
    let canonical_executable = fs::canonicalize(&executable)
        .map_err(|error| format!("cannot canonicalize main executable: {error}"))?;
    if !canonical_executable.starts_with(&canonical_app) {
        return Err("main executable escapes the app bundle".into());
    }
    let architectures = read_macho_architectures(&executable)?;
    if !architectures.contains(&expected_architecture) {
        return Err(format!(
            "application architecture mismatch: expected {expected_architecture:?}, found {architectures:?}"
        ));
    }

    let verification = command_output(
        "/usr/bin/codesign",
        &[
            "--verify",
            "--deep",
            "--strict",
            "--verbose=2",
            path_text(app)?,
        ],
    )?;
    ensure_command_success("codesign verification", &verification)?;
    let display = command_output(
        "/usr/bin/codesign",
        &["--display", "--verbose=4", path_text(app)?],
    )?;
    ensure_command_success("codesign identity inspection", &display)?;
    let display_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&display.stdout),
        String::from_utf8_lossy(&display.stderr)
    );
    let team_id = parse_codesign_value(&display_text, "TeamIdentifier");
    if let Some(expected_team_id) = trust.macos_team_id.as_deref()
        && team_id.as_deref() != Some(expected_team_id)
    {
        return Err(format!(
            "Team ID mismatch: expected {expected_team_id}, got {:?}",
            team_id
        ));
    }
    let gatekeeper = command_output(
        "/usr/sbin/spctl",
        &[
            "--assess",
            "--type",
            "execute",
            "--verbose=4",
            path_text(app)?,
        ],
    )?;
    ensure_command_success("Gatekeeper assessment", &gatekeeper)?;

    Ok(AppInspection {
        bundle_id: bundle_id.to_owned(),
        version,
        team_id,
    })
}

fn with_prepared_app<T>(
    artifact: &Path,
    kind: PackageKind,
    trust: &TrustEntry,
    operation: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let application_name = trust
        .macos_application_name
        .as_deref()
        .ok_or_else(|| "trust registry has no pinned macOS application name".to_owned())?;
    match kind {
        PackageKind::Dmg => {
            let mount = MountedDmg::attach(artifact)?;
            let app = find_expected_app(&mount.mount_point, application_name)?;
            operation(&app)
        }
        PackageKind::Zip | PackageKind::TarGz => {
            let expanded = prepare_archive(artifact, kind)?;
            let app = find_expected_app(&expanded, application_name)?;
            operation(&app)
        }
        _ => Err(format!("unsupported macOS package type: {kind:?}")),
    }
}

struct MountedDmg {
    mount_point: PathBuf,
}

impl MountedDmg {
    fn attach(path: &Path) -> Result<Self, String> {
        let output = command_output(
            "/usr/bin/hdiutil",
            &[
                "attach",
                "-readonly",
                "-nobrowse",
                "-plist",
                path_text(path)?,
            ],
        )?;
        ensure_command_success("DMG attach", &output)?;
        let plist = Value::from_reader(Cursor::new(output.stdout))
            .map_err(|error| format!("invalid hdiutil plist output: {error}"))?;
        let entities = plist
            .as_dictionary()
            .and_then(|dictionary| dictionary.get("system-entities"))
            .and_then(Value::as_array)
            .ok_or_else(|| "hdiutil output has no system-entities".to_owned())?;
        let mount_point = entities
            .iter()
            .filter_map(Value::as_dictionary)
            .find_map(|entity| entity.get("mount-point").and_then(Value::as_string))
            .map(PathBuf::from)
            .ok_or_else(|| "hdiutil did not report a mount point".to_owned())?;
        let mut mounted = Self { mount_point };
        let mount_point = fs::canonicalize(&mounted.mount_point)
            .map_err(|error| format!("cannot canonicalize DMG mount point: {error}"))?;
        if !mount_point.starts_with("/Volumes") || !mount_point.is_dir() {
            return Err(format!(
                "unexpected DMG mount point: {}",
                mount_point.display()
            ));
        }
        mounted.mount_point = mount_point;
        Ok(mounted)
    }
}

impl Drop for MountedDmg {
    fn drop(&mut self) {
        let _ = command_output(
            "/usr/bin/hdiutil",
            &[
                "detach",
                "-quiet",
                self.mount_point.to_string_lossy().as_ref(),
            ],
        );
    }
}

fn prepare_archive(artifact: &Path, kind: PackageKind) -> Result<PathBuf, String> {
    match kind {
        PackageKind::Zip => validate_zip_archive(artifact)?,
        PackageKind::TarGz => validate_tar_gz_archive(artifact)?,
        _ => return Err(format!("unsupported archive kind: {kind:?}")),
    }
    let parent = artifact
        .parent()
        .ok_or_else(|| "artifact has no parent directory".to_owned())?;
    let file_name = artifact
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "artifact file name is not Unicode".to_owned())?;
    let expanded = parent.join(format!(".{file_name}.expanded"));
    if expanded.exists() {
        validate_private_child(parent, &expanded)?;
        validate_extracted_tree(&expanded)?;
        return Ok(expanded);
    }
    fs::create_dir(&expanded)
        .map_err(|error| format!("cannot create archive staging directory: {error}"))?;
    let output = match kind {
        PackageKind::Zip => command_output(
            "/usr/bin/ditto",
            &["-x", "-k", path_text(artifact)?, path_text(&expanded)?],
        )?,
        PackageKind::TarGz => command_output(
            "/usr/bin/tar",
            &["-xzf", path_text(artifact)?, "-C", path_text(&expanded)?],
        )?,
        _ => unreachable!(),
    };
    if let Err(error) = ensure_command_success("archive extraction", &output) {
        let _ = remove_directory_if_regular(&expanded);
        return Err(error);
    }
    validate_extracted_tree(&expanded)?;
    Ok(expanded)
}

fn validate_zip_archive(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("cannot open ZIP: {error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("invalid ZIP: {error}"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("ZIP contains too many entries".into());
    }
    let mut total = 0_u64;
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot inspect ZIP entry: {error}"))?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| format!("ZIP entry escapes staging: {}", entry.name()))?
            .to_owned();
        validate_relative_path(&enclosed)?;
        if !names.insert(archive_path_key(&enclosed)) {
            return Err(format!(
                "ZIP contains duplicate path: {}",
                enclosed.display()
            ));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| "ZIP expanded size overflow".to_owned())?;
        if total > MAX_EXPANDED_BYTES {
            return Err("ZIP expanded size exceeds 12 GiB".into());
        }
        if entry.is_symlink() {
            let mut target = String::new();
            entry
                .by_ref()
                .take(4097)
                .read_to_string(&mut target)
                .map_err(|error| format!("cannot inspect ZIP symlink: {error}"))?;
            if target.len() > 4096 {
                return Err("ZIP symlink target is too long".into());
            }
            validate_link_target(&enclosed, Path::new(&target))?;
        } else if !entry.is_file() && !entry.is_dir() {
            return Err(format!("ZIP contains unsupported entry: {}", entry.name()));
        }
    }
    Ok(())
}

fn validate_tar_gz_archive(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("cannot open tar.gz: {error}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut total = 0_u64;
    let mut count = 0_usize;
    let mut names = HashSet::new();
    let entries = archive
        .entries()
        .map_err(|error| format!("invalid tar.gz: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot inspect tar entry: {error}"))?;
        count += 1;
        if count > MAX_ARCHIVE_ENTRIES {
            return Err("tar.gz contains too many entries".into());
        }
        let path = entry
            .path()
            .map_err(|error| format!("invalid tar path: {error}"))?
            .into_owned();
        validate_relative_path(&path)?;
        if !names.insert(archive_path_key(&path)) {
            return Err(format!(
                "tar.gz contains duplicate path: {}",
                path.display()
            ));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| "tar.gz expanded size overflow".to_owned())?;
        if total > MAX_EXPANDED_BYTES {
            return Err("tar.gz expanded size exceeds 12 GiB".into());
        }
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() {
            let target = entry
                .link_name()
                .map_err(|error| format!("invalid tar link: {error}"))?
                .ok_or_else(|| "tar link has no target".to_owned())?;
            validate_link_target(&path, &target)?;
        } else if entry_type.is_hard_link() {
            let target = entry
                .link_name()
                .map_err(|error| format!("invalid tar hard link: {error}"))?
                .ok_or_else(|| "tar hard link has no target".to_owned())?;
            validate_relative_path(&target)?;
        } else if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(format!(
                "tar.gz contains unsupported entry type at {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(format!("archive path is not relative: {}", path.display()));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("archive path escapes staging: {}", path.display()));
    }
    Ok(())
}

fn archive_path_key(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_link_target(entry_path: &Path, target: &Path) -> Result<(), String> {
    if target.as_os_str().is_empty() || target.is_absolute() {
        return Err(format!(
            "archive link has an unsafe target: {} -> {}",
            entry_path.display(),
            target.display()
        ));
    }
    let mut depth = entry_path
        .parent()
        .map(|path| path.components().count())
        .unwrap_or(0);
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => depth += 1,
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir => {
                return Err(format!(
                    "archive link escapes staging: {} -> {}",
                    entry_path.display(),
                    target.display()
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "archive link has an absolute target: {} -> {}",
                    entry_path.display(),
                    target.display()
                ));
            }
        }
    }
    Ok(())
}

fn validate_extracted_tree(root: &Path) -> Result<(), String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("cannot canonicalize extraction root: {error}"))?;
    let mut stack = vec![root.clone()];
    let mut count = 0_usize;
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("cannot inspect extracted directory: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("cannot inspect extracted entry: {error}"))?;
            count += 1;
            if count > MAX_ARCHIVE_ENTRIES {
                return Err("extracted archive contains too many entries".into());
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("cannot inspect extracted path: {error}"))?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                let resolved = fs::canonicalize(&path).map_err(|error| {
                    format!("extracted archive contains a broken symlink: {error}")
                })?;
                if !resolved.starts_with(&root) {
                    return Err(format!(
                        "extracted symlink escapes staging: {}",
                        path.display()
                    ));
                }
            } else if file_type.is_dir() {
                stack.push(path);
            } else if !file_type.is_file() {
                return Err(format!(
                    "extracted archive contains a special file: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn find_expected_app(root: &Path, application_name: &str) -> Result<PathBuf, String> {
    let mut stack = vec![root.to_path_buf()];
    let mut matches = Vec::new();
    let mut count = 0_usize;
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("cannot search package contents: {error}"))?
        {
            let entry = entry.map_err(|error| format!("cannot inspect package entry: {error}"))?;
            count += 1;
            if count > MAX_ARCHIVE_ENTRIES {
                return Err("package contents exceed the search bound".into());
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("cannot inspect package path: {error}"))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if entry.file_name() == application_name {
                    matches.push(path);
                } else if path.extension().and_then(|value| value.to_str()) != Some("app") {
                    stack.push(path);
                }
            }
        }
    }
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(format!("package does not contain {application_name}")),
        _ => Err(format!(
            "package contains multiple {application_name} bundles"
        )),
    }
}

fn select_install_target(
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<PathBuf, String> {
    let application_name = trust
        .macos_application_name
        .as_deref()
        .ok_or_else(|| "trust registry has no pinned macOS application name".to_owned())?;
    let candidates = application_candidates(application_name)?;
    let existing: Vec<_> = candidates
        .iter()
        .filter(|(path, _)| path.exists())
        .collect();
    if existing.len() > 1 {
        return Err(format!(
            "同时发现用户级和系统级 {application_name}，拒绝自动覆盖"
        ));
    }
    if let Some((path, _)) = existing.first() {
        inspect_app_bundle(path, trust, expected_architecture)?;
        return Ok((*path).clone());
    }
    let user_applications = user_applications_directory()?;
    fs::create_dir_all(&user_applications)
        .map_err(|error| format!("cannot create user Applications directory: {error}"))?;
    validate_install_parent(&user_applications)?;
    Ok(user_applications.join(application_name))
}

fn install_app_bundle(
    source: &Path,
    target: &Path,
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "installation target has no parent".to_owned())?;
    validate_install_parent(parent)?;
    if target.exists() {
        inspect_app_bundle(target, trust, expected_architecture)?;
    }
    let stage_root = Builder::new()
        .prefix(".easy-agent-stage-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create installation staging directory: {error}"))?;
    let stage_app = stage_root.path().join(
        target
            .file_name()
            .ok_or_else(|| "installation target has no file name".to_owned())?,
    );
    let copy = command_output(
        "/usr/bin/ditto",
        &[path_text(source)?, path_text(&stage_app)?],
    )?;
    ensure_command_success("app bundle copy", &copy)?;
    inspect_app_bundle(&stage_app, trust, expected_architecture)?;

    activate_staged_app(&stage_app, target, |installed| {
        inspect_app_bundle(installed, trust, expected_architecture).map(|_| ())
    })
}

fn activate_staged_app(
    stage_app: &Path,
    target: &Path,
    verify_installed: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "installation target has no parent".to_owned())?;
    let backup_root = Builder::new()
        .prefix(".easy-agent-backup-")
        .tempdir_in(parent)
        .map_err(|error| format!("cannot create installation backup directory: {error}"))?;
    let backup_app = backup_root.path().join(
        target
            .file_name()
            .ok_or_else(|| "installation target has no file name".to_owned())?,
    );
    let had_existing = target.exists();
    if had_existing {
        fs::rename(target, &backup_app)
            .map_err(|error| format!("cannot move existing app aside: {error}"))?;
    }
    if let Err(error) = fs::rename(stage_app, target) {
        if had_existing && let Err(restore_error) = fs::rename(&backup_app, target) {
            let preserved = backup_root.keep();
            return Err(format!(
                "cannot activate the new app bundle: {error}; restoring the previous app also failed: {restore_error}; backup preserved at {}",
                preserved.display()
            ));
        }
        return Err(format!("cannot activate the new app bundle: {error}"));
    }
    if let Err(error) = verify_installed(target) {
        if let Err(remove_error) = remove_directory_if_regular(target) {
            if had_existing {
                let preserved = backup_root.keep();
                return Err(format!(
                    "installed app failed final verification: {error}; the invalid replacement could not be removed: {remove_error}; previous app preserved at {}",
                    preserved.display()
                ));
            }
            return Err(format!(
                "installed app failed final verification: {error}; the invalid replacement could not be removed: {remove_error}"
            ));
        }
        if had_existing && let Err(restore_error) = fs::rename(&backup_app, target) {
            let preserved = backup_root.keep();
            return Err(format!(
                "installed app failed final verification: {error}; restoring the previous app failed: {restore_error}; backup preserved at {}",
                preserved.display()
            ));
        }
        return Err(format!("installed app failed final verification: {error}"));
    }
    Ok(())
}

fn application_candidates(application_name: &str) -> Result<Vec<(PathBuf, &'static str)>, String> {
    Ok(vec![
        (
            user_applications_directory()?.join(application_name),
            "用户 Applications",
        ),
        (
            PathBuf::from("/Applications").join(application_name),
            "系统 Applications",
        ),
    ])
}

fn user_applications_directory() -> Result<PathBuf, String> {
    let home = user_home_directory()?;
    if !home.is_absolute() {
        return Err("the system home directory is not absolute".into());
    }
    let applications = home.join("Applications");
    if applications.exists() {
        let metadata = fs::symlink_metadata(&applications)
            .map_err(|error| format!("cannot inspect user Applications directory: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("user Applications is not a regular directory".into());
        }
    }
    Ok(applications)
}

fn validate_install_parent(parent: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("cannot inspect installation parent: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "installation parent is not a regular directory: {}",
            parent.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn user_home_directory() -> Result<PathBuf, String> {
    use std::ffi::CStr;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;
    use std::ptr;

    // SAFETY: both calls take no pointers and only query the current process/user database.
    let uid = unsafe { libc::geteuid() };
    let suggested = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let mut buffer_size = if suggested > 0 {
        (suggested as usize).clamp(16_384, 1024 * 1024)
    } else {
        16_384
    };
    loop {
        let mut password = MaybeUninit::<libc::passwd>::uninit();
        let mut result = ptr::null_mut();
        let mut buffer = vec![0_u8; buffer_size];
        // SAFETY: password and result point to live stack storage, while buffer remains allocated
        // for the full call and is passed with its exact length.
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                password.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE && buffer_size < 1024 * 1024 {
            buffer_size = (buffer_size * 2).min(1024 * 1024);
            continue;
        }
        if status != 0 || result.is_null() {
            return Err(format!(
                "cannot resolve the current user's home directory: {status}"
            ));
        }
        // SAFETY: POSIX guarantees the passwd structure is initialized when status is zero and
        // result is non-null; both conditions were checked above.
        let password = unsafe { password.assume_init() };
        if password.pw_dir.is_null() {
            return Err("the current user record has no home directory".into());
        }
        // SAFETY: pw_dir was checked for null and points into the still-live getpwuid_r buffer.
        let bytes = unsafe { CStr::from_ptr(password.pw_dir) }.to_bytes();
        return Ok(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)));
    }
}

#[cfg(not(target_os = "macos"))]
fn user_home_directory() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is unavailable in this non-macOS test build".to_owned())
}

fn validate_private_child(parent: &Path, child: &Path) -> Result<(), String> {
    if child.parent() != Some(parent) {
        return Err("staging path is outside the artifact directory".into());
    }
    let metadata = fs::symlink_metadata(child)
        .map_err(|error| format!("cannot inspect staging path: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("staging path is not a regular directory".into());
    }
    Ok(())
}

fn remove_directory_if_regular(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect directory before removal: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "refusing to remove non-directory path: {}",
            path.display()
        ));
    }
    fs::remove_dir_all(path).map_err(|error| format!("cannot remove directory: {error}"))
}

fn plist_string<'a>(dictionary: &'a plist::Dictionary, key: &str) -> Result<&'a str, String> {
    dictionary
        .get(key)
        .and_then(Value::as_string)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Info.plist has no {key}"))
}

fn safe_single_file_name(value: &str) -> bool {
    value == value.trim()
        && !value.is_empty()
        && !value
            .chars()
            .any(|character| matches!(character, '/' | '\\' | ':'))
        && !matches!(value, "." | "..")
}

fn parse_codesign_value(source: &str, key: &str) -> Option<String> {
    source.lines().find_map(|line| {
        line.trim()
            .strip_prefix(key)
            .and_then(|value| value.strip_prefix('='))
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "not set")
            .map(ToOwned::to_owned)
    })
}

fn read_macho_architectures(path: &Path) -> Result<HashSet<Architecture>, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("cannot open Mach-O executable {}: {error}", path.display()))?;
    let length = file
        .metadata()
        .map_err(|error| format!("cannot stat Mach-O executable: {error}"))?
        .len();
    let mut header = [0_u8; 8];
    file.read_exact(&mut header)
        .map_err(|error| format!("cannot read Mach-O header: {error}"))?;
    let mut architectures = HashSet::new();
    match &header[..4] {
        [0xcf, 0xfa, 0xed, 0xfe] => insert_cpu_type(
            &mut architectures,
            u32::from_le_bytes(header[4..8].try_into().unwrap()),
        ),
        [0xfe, 0xed, 0xfa, 0xcf] => insert_cpu_type(
            &mut architectures,
            u32::from_be_bytes(header[4..8].try_into().unwrap()),
        ),
        [0xca, 0xfe, 0xba, 0xbe] | [0xca, 0xfe, 0xba, 0xbf] => {
            read_fat_architectures(&mut file, length, &header, true, &mut architectures)?
        }
        [0xbe, 0xba, 0xfe, 0xca] | [0xbf, 0xba, 0xfe, 0xca] => {
            read_fat_architectures(&mut file, length, &header, false, &mut architectures)?
        }
        _ => return Err("main executable is not a supported 64-bit Mach-O".into()),
    }
    if architectures.is_empty() {
        return Err("Mach-O has no supported x86_64 or arm64 slice".into());
    }
    Ok(architectures)
}

fn read_fat_architectures(
    file: &mut File,
    file_length: u64,
    header: &[u8; 8],
    big_endian: bool,
    architectures: &mut HashSet<Architecture>,
) -> Result<(), String> {
    let is_64 = matches!(
        &header[..4],
        [0xca, 0xfe, 0xba, 0xbf] | [0xbf, 0xba, 0xfe, 0xca]
    );
    let count = if big_endian {
        u32::from_be_bytes(header[4..8].try_into().unwrap())
    } else {
        u32::from_le_bytes(header[4..8].try_into().unwrap())
    } as usize;
    if count == 0 || count > 16 {
        return Err(format!("invalid Mach-O fat slice count: {count}"));
    }
    let entry_size = if is_64 { 32 } else { 20 };
    let table_size = 8_u64 + (count * entry_size) as u64;
    if table_size > file_length {
        return Err("Mach-O fat header exceeds file length".into());
    }
    for _ in 0..count {
        let mut entry = vec![0_u8; entry_size];
        file.read_exact(&mut entry)
            .map_err(|error| format!("cannot read Mach-O fat entry: {error}"))?;
        let cpu_type = if big_endian {
            u32::from_be_bytes(entry[0..4].try_into().unwrap())
        } else {
            u32::from_le_bytes(entry[0..4].try_into().unwrap())
        };
        insert_cpu_type(architectures, cpu_type);
    }
    Ok(())
}

fn insert_cpu_type(architectures: &mut HashSet<Architecture>, cpu_type: u32) {
    match cpu_type {
        0x0100_0007 => {
            architectures.insert(Architecture::X64);
        }
        0x0100_000c => {
            architectures.insert(Architecture::Arm64);
        }
        _ => {}
    }
}

fn command_output(program: &str, arguments: &[&str]) -> Result<Output, String> {
    if !Path::new(program).is_absolute() {
        return Err(format!("refusing to run a non-absolute program: {program}"));
    }
    let mut command = Command::new(program);
    command
        .args(arguments)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env_remove("DYLD_INSERT_LIBRARIES")
        .env_remove("DYLD_LIBRARY_PATH")
        .env_remove("DYLD_FRAMEWORK_PATH")
        .env_remove("DYLD_FALLBACK_LIBRARY_PATH")
        .env_remove("DYLD_FALLBACK_FRAMEWORK_PATH");
    command
        .output()
        .map_err(|error| format!("cannot start {program}: {error}"))
}

fn ensure_command_success(label: &str, output: &Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if detail.is_empty() {
        format!("{label} failed with {}", output.status)
    } else {
        format!("{label} failed: {detail}")
    })
}

fn path_text(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("path is not valid Unicode: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_archive_paths_and_links_that_escape_staging() {
        assert!(validate_relative_path(Path::new("Client.app/Contents/Info.plist")).is_ok());
        assert!(validate_relative_path(Path::new("../outside")).is_err());
        assert!(validate_link_target(Path::new("a/b/link"), Path::new("../target")).is_ok());
        assert!(validate_link_target(Path::new("link"), Path::new("../outside")).is_err());
        assert!(validate_link_target(Path::new("a/link"), Path::new("/outside")).is_err());
    }

    #[test]
    fn parses_team_identifier_without_accepting_missing_values() {
        assert_eq!(
            parse_codesign_value("TeamIdentifier=ABCDE12345\n", "TeamIdentifier").as_deref(),
            Some("ABCDE12345")
        );
        assert_eq!(
            parse_codesign_value("TeamIdentifier=not set\n", "TeamIdentifier"),
            None
        );
    }

    #[test]
    fn reads_thin_and_universal_macho_architectures() {
        let root = tempfile::tempdir().unwrap();
        let thin = root.path().join("thin");
        fs::write(&thin, [0xcf, 0xfa, 0xed, 0xfe, 0x07, 0x00, 0x00, 0x01]).unwrap();
        assert_eq!(
            read_macho_architectures(&thin).unwrap(),
            HashSet::from([Architecture::X64])
        );

        let universal = root.path().join("universal");
        let mut file = File::create(&universal).unwrap();
        file.write_all(&[0xca, 0xfe, 0xba, 0xbe, 0x00, 0x00, 0x00, 0x02])
            .unwrap();
        for cpu in [0x0100_0007_u32, 0x0100_000c_u32] {
            file.write_all(&cpu.to_be_bytes()).unwrap();
            file.write_all(&[0_u8; 16]).unwrap();
        }
        drop(file);
        assert_eq!(
            read_macho_architectures(&universal).unwrap(),
            HashSet::from([Architecture::X64, Architecture::Arm64])
        );
    }

    #[test]
    fn failed_final_verification_restores_the_previous_app() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("Client.app");
        let stage = root.path().join("Stage.app");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("marker"), "old").unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("marker"), "new").unwrap();

        assert!(activate_staged_app(&stage, &target, |_| Err("fixture".into())).is_err());
        assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "old");
        assert!(!stage.exists());
    }
}
