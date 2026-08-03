use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use base64::Engine;
use regex::Regex;
use serde::Deserialize;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use zip::ZipArchive;

use crate::core::{
    Architecture, Detection, PackageKind, ProductId, TrustEntry, WindowsPeMachine,
    verify_staged_identity,
};

use super::{ArtifactVerification, InstallerExecution, PlannedCommand, VerifiedInstallRequest};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_INSTALLER_ERROR_CHARS: usize = 4096;

const DETECTION_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Import-Module Appx -ErrorAction Stop
$product = $env:AI_CLIENT_INSTALLER_PRODUCT
$tokens = switch ($product) {
  'workbuddy' { @('WorkBuddy') }
  'hermes' { @('Hermes', 'Hermes Agent') }
  'cc_switch' { @('CC Switch', 'CCSwitch') }
  'claude' { @('Claude') }
  'chatgpt' { @('ChatGPT', 'OpenAI.Codex') }
  default { @() }
}

$appx = $null
if ($product -eq 'claude') {
  $pkg = Get-AppxPackage -Name Claude -ErrorAction SilentlyContinue | Sort-Object Version -Descending | Select-Object -First 1
  if ($pkg) { $appx = [pscustomobject]@{ installed=$true; version=$pkg.Version.ToString(); managed=[bool]$pkg.NonRemovable; management_known=$true; package_identity=[string]$pkg.Name; package_family=[string]$pkg.PackageFamilyName; publisher=[string]$pkg.Publisher; architecture=[string]$pkg.Architecture; evidence=('AppX:' + $pkg.PackageFamilyName) } }
}
if ($product -eq 'chatgpt') {
  $pkg = Get-AppxPackage -ErrorAction SilentlyContinue | Where-Object { $_.PackageFamilyName -eq 'OpenAI.Codex_2p2nqsd0c76g0' } | Sort-Object Version -Descending | Select-Object -First 1
  if ($pkg) { $appx = [pscustomobject]@{ installed=$true; version=$pkg.Version.ToString(); managed=[bool]$pkg.NonRemovable; management_known=$true; package_identity=[string]$pkg.Name; package_family=[string]$pkg.PackageFamilyName; publisher=[string]$pkg.Publisher; architecture=[string]$pkg.Architecture; evidence=('AppX:' + $pkg.PackageFamilyName) } }
}

$registryEntries = @()
if (-not $appx) {
  $roots = @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
  )
  $registryEntries = @(
    Get-ItemProperty -Path $roots -ErrorAction SilentlyContinue |
      Where-Object {
        $name = [string]$_.DisplayName
        $matched = $false
        foreach ($token in $tokens) {
          if ($name.StartsWith($token, [System.StringComparison]::OrdinalIgnoreCase)) {
            $matched = $true
            break
          }
        }
        $matched
      } |
      ForEach-Object {
        [pscustomobject]@{
          display_name = [string]$_.DisplayName
          version = if ($null -eq $_.DisplayVersion) { $null } else { [string]$_.DisplayVersion }
          publisher = if ($null -eq $_.Publisher) { $null } else { [string]$_.Publisher }
          install_location = if ($null -eq $_.InstallLocation) { $null } else { [string]$_.InstallLocation }
          display_icon = if ($null -eq $_.DisplayIcon) { $null } else { [string]$_.DisplayIcon }
          uninstall_string = if ($null -eq $_.UninstallString) { $null } else { [string]$_.UninstallString }
          current_user = ([string]$_.PSPath).StartsWith('Microsoft.PowerShell.Core\Registry::HKEY_CURRENT_USER\', [System.StringComparison]::OrdinalIgnoreCase)
        }
      }
  )
}

[pscustomobject]@{
  appx = $appx
  registry_entries = $registryEntries
} | ConvertTo-Json -Compress -Depth 5
"#;

const VERIFY_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$path = $env:AI_CLIENT_INSTALLER_ARTIFACT
$kind = $env:AI_CLIENT_INSTALLER_PACKAGE_KIND
$signature = Get-AuthenticodeSignature -LiteralPath $path
$product = $null
$version = $null
$template = $null

if ($kind -eq 'msi') {
  $installer = New-Object -ComObject WindowsInstaller.Installer
  $database = $installer.OpenDatabase($path, 0)
  foreach ($property in @('ProductName', 'ProductVersion')) {
    $view = $database.OpenView("SELECT `Value` FROM `Property` WHERE `Property`='$property'")
    $view.Execute()
    $record = $view.Fetch()
    if ($record) {
      if ($property -eq 'ProductName') { $product = $record.StringData(1) }
      if ($property -eq 'ProductVersion') { $version = $record.StringData(1) }
    }
    $view.Close()
  }
  $summary = $database.SummaryInformation(0)
  $template = [string]$summary.Property(7)
} elseif ($kind -eq 'exe') {
  $info = (Get-Item -LiteralPath $path).VersionInfo
  $product = [string]$info.ProductName
  $version = [string]$info.ProductVersion
}

[pscustomobject]@{
  signature_status = [string]$signature.Status
  signer_subject = if ($signature.SignerCertificate) { [string]$signature.SignerCertificate.Subject } else { $null }
  product = $product
  version = $version
  template = $template
} | ConvertTo-Json -Compress
"#;

const INSTALL_MSIX_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Import-Module Appx -ErrorAction Stop
Add-AppxPackage -Path $env:AI_CLIENT_INSTALLER_ARTIFACT -ForceTargetApplicationShutdown -ErrorAction Stop
"#;

#[derive(Debug, Deserialize)]
struct AppxDetectionOutput {
    installed: bool,
    version: Option<String>,
    managed: bool,
    management_known: bool,
    package_identity: Option<String>,
    package_family: Option<String>,
    publisher: Option<String>,
    architecture: Option<String>,
    evidence: String,
}

#[derive(Debug, Deserialize)]
struct RegistryEntryOutput {
    display_name: String,
    version: Option<String>,
    publisher: Option<String>,
    install_location: Option<String>,
    display_icon: Option<String>,
    uninstall_string: Option<String>,
    current_user: bool,
}

#[derive(Debug, Deserialize)]
struct DetectionOutput {
    appx: Option<AppxDetectionOutput>,
    #[serde(default)]
    registry_entries: Vec<RegistryEntryOutput>,
}

#[derive(Debug, Deserialize)]
struct VerificationOutput {
    signature_status: String,
    signer_subject: Option<String>,
    product: Option<String>,
    version: Option<String>,
    template: Option<String>,
}

fn trusted_system_executable(relative_path: &Path) -> Result<PathBuf, String> {
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("trusted system executable path must be a fixed relative path".into());
    }
    let mut buffer = vec![0_u16; 32_768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 || length as usize >= buffer.len() {
        return Err("cannot resolve the Windows system directory".into());
    }
    let system_directory = PathBuf::from(OsString::from_wide(&buffer[..length as usize]));
    let candidate = system_directory.join(relative_path);
    let canonical_root = fs::canonicalize(&system_directory)
        .map_err(|error| format!("cannot canonicalize the Windows system directory: {error}"))?;
    let canonical_candidate = fs::canonicalize(&candidate).map_err(|error| {
        format!(
            "cannot resolve trusted system executable {}: {error}",
            candidate.display()
        )
    })?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(format!(
            "trusted system executable escaped the Windows system directory: {}",
            canonical_candidate.display()
        ));
    }
    let metadata = fs::metadata(&canonical_candidate)
        .map_err(|error| format!("cannot stat trusted system executable: {error}"))?;
    if !metadata.is_file() {
        return Err(format!(
            "trusted system executable is not a regular file: {}",
            canonical_candidate.display()
        ));
    }
    Ok(canonical_candidate)
}

pub(crate) fn trusted_powershell_program() -> Result<String, String> {
    trusted_system_executable(Path::new(r"WindowsPowerShell\v1.0\powershell.exe"))?
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "trusted PowerShell path is not valid Unicode".into())
}

fn trusted_msiexec_program() -> Result<String, String> {
    trusted_system_executable(Path::new("msiexec.exe"))?
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "trusted msiexec path is not valid Unicode".into())
}

pub(crate) fn is_powershell_program(program: &str) -> bool {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("powershell.exe"))
}

pub(crate) fn hide_console_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

pub fn detect_product(
    product: ProductId,
    trust: Option<&TrustEntry>,
) -> Result<Detection, io::Error> {
    let bytes: Vec<u8> = DETECTION_SCRIPT
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let powershell = trusted_powershell_program().map_err(io::Error::other)?;
    let mut command = Command::new(powershell);
    hide_console_window(&mut command);
    command.env_remove("PSModulePath");
    let output = command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-OutputFormat",
            "Text",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .env("AI_CLIENT_INSTALLER_PRODUCT", product.key())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let parsed: DetectionOutput = serde_json::from_slice(&output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if let Some(appx) = parsed.appx {
        return Ok(Detection {
            installed: appx.installed,
            version: appx.version,
            managed: appx.managed,
            management_known: appx.management_known,
            package_identity: appx.package_identity,
            package_family: appx.package_family,
            publisher: appx.publisher,
            architecture: appx
                .architecture
                .as_deref()
                .and_then(parse_appx_architecture),
            evidence: appx.evidence,
        });
    }
    if let Some(detection) = select_registry_detection(product, parsed.registry_entries, trust) {
        return Ok(detection);
    }
    if product == ProductId::Hermes
        && let (Some(local_app_data), Some(trust)) = (env::var_os("LOCALAPPDATA"), trust)
        && let Some(detection) =
            detect_hermes_fixed_install_at(&PathBuf::from(local_app_data), trust)
    {
        return Ok(detection);
    }
    Ok(Detection::absent("No exact registered identity found"))
}

fn detect_hermes_fixed_install_at(local_app_data: &Path, trust: &TrustEntry) -> Option<Detection> {
    detect_hermes_fixed_install_at_with(local_app_data, trust.architecture, |setup| {
        verify_artifact(setup, PackageKind::Exe, trust, trust.architecture, false).is_ok()
    })
}

fn detect_hermes_fixed_install_at_with(
    local_app_data: &Path,
    architecture: Architecture,
    verify_setup: impl FnOnce(&Path) -> bool,
) -> Option<Detection> {
    let local_app_data = fs::canonicalize(local_app_data).ok()?;
    let hermes_root = fs::canonicalize(local_app_data.join("hermes")).ok()?;
    if !hermes_root.starts_with(&local_app_data) {
        return None;
    }
    let setup = fs::canonicalize(hermes_root.join("hermes-setup.exe")).ok()?;
    if !setup.starts_with(&hermes_root) || !setup.is_file() || !verify_setup(&setup) {
        return None;
    }
    let install_root = fs::canonicalize(hermes_root.join("hermes-agent")).ok()?;
    if !install_root.starts_with(&hermes_root) {
        return None;
    }
    let unpacked_directory = match architecture {
        Architecture::X64 => "win-unpacked",
        Architecture::Arm64 => "win-arm64-unpacked",
        Architecture::Unsupported => return None,
    };
    let executable = fs::canonicalize(
        install_root
            .join("apps")
            .join("desktop")
            .join("release")
            .join(unpacked_directory)
            .join("Hermes.exe"),
    )
    .ok()?;
    if !executable.starts_with(&install_root) || !executable.is_file() {
        return None;
    }
    let actual_architecture = read_pe_machine(&executable).ok()?.architecture();

    let package_source = read_bounded_text_under(
        &install_root,
        &install_root
            .join("apps")
            .join("desktop")
            .join("package.json"),
        1024 * 1024,
    )?;
    let package: serde_json::Value = serde_json::from_str(&package_source).ok()?;
    if package.get("name").and_then(|value| value.as_str()) != Some("hermes")
        || package.get("productName").and_then(|value| value.as_str()) != Some("Hermes")
        || package.get("author").and_then(|value| value.as_str()) != Some("Nous Research")
        || package
            .get("build")
            .and_then(|build| build.get("appId"))
            .and_then(|value| value.as_str())
            != Some("com.nousresearch.hermes")
    {
        return None;
    }

    let git_config = read_bounded_text_under(
        &install_root,
        &install_root.join(".git").join("config"),
        1024 * 1024,
    )?;
    static OFFICIAL_HERMES_ORIGIN: OnceLock<Regex> = OnceLock::new();
    if !OFFICIAL_HERMES_ORIGIN
        .get_or_init(|| {
            Regex::new(r"(?im)^\s*url\s*=\s*https://github\.com/NousResearch/hermes-agent\.git\s*$")
                .expect("static Hermes origin regex")
        })
        .is_match(&git_config)
    {
        return None;
    }

    let version_source = read_bounded_text_under(
        &install_root,
        &install_root.join("hermes_cli").join("__init__.py"),
        1024 * 1024,
    )?;
    static HERMES_VERSION: OnceLock<Regex> = OnceLock::new();
    let version = HERMES_VERSION
        .get_or_init(|| {
            Regex::new(r#"(?m)^__version__\s*=\s*["']([0-9]+(?:\.[0-9]+)+)["']\s*$"#)
                .expect("static Hermes version regex")
        })
        .captures(&version_source)?
        .get(1)?
        .as_str()
        .to_owned();

    Some(Detection {
        installed: true,
        version: Some(version),
        managed: false,
        management_known: true,
        package_identity: Some("com.nousresearch.hermes".into()),
        package_family: None,
        publisher: Some("Nous Research".into()),
        architecture: Some(actual_architecture),
        evidence: "Hermes Desktop fixed local install".into(),
    })
}

fn read_bounded_text_under(root: &Path, path: &Path, max_bytes: u64) -> Option<String> {
    let path = fs::canonicalize(path).ok()?;
    if !path.starts_with(root) {
        return None;
    }
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn select_registry_detection(
    product: ProductId,
    entries: Vec<RegistryEntryOutput>,
    trust: Option<&TrustEntry>,
) -> Option<Detection> {
    entries
        .into_iter()
        .filter(|entry| matches_registry_entry(product, entry))
        .max_by(|left, right| compare_optional_versions(&left.version, &right.version))
        .map(|entry| {
            let scope = if entry.current_user { "HKCU" } else { "HKLM" };
            let architecture = trust
                .and_then(|entry| entry.postinstall_executable.as_deref())
                .and_then(|executable| resolve_postinstall_executable(&entry, executable))
                .map(|path| {
                    read_pe_machine(&path)
                        .map(WindowsPeMachine::architecture)
                        .unwrap_or(Architecture::Unsupported)
                });
            Detection {
                installed: true,
                version: entry.version,
                managed: false,
                management_known: entry.current_user,
                package_identity: None,
                package_family: None,
                publisher: entry.publisher,
                architecture,
                evidence: format!("Uninstall:{} [{scope}]", entry.display_name),
            }
        })
}

fn resolve_postinstall_executable(
    entry: &RegistryEntryOutput,
    executable: &str,
) -> Option<PathBuf> {
    let mut candidates = Vec::with_capacity(3);
    if let Some(install_location) = entry.install_location.as_deref() {
        let install_location = install_location.trim().trim_matches('"');
        if !install_location.is_empty() {
            candidates.push(PathBuf::from(install_location).join(executable));
        }
    }
    if let Some(display_icon) = entry.display_icon.as_deref()
        && let Some(path) = executable_path_prefix(display_icon)
        && file_name_matches(&path, executable)
    {
        candidates.push(path);
    }
    if let Some(uninstall_string) = entry.uninstall_string.as_deref()
        && let Some(uninstaller) = executable_path_prefix(uninstall_string)
        && let Some(parent) = uninstaller.parent()
    {
        candidates.push(parent.join(executable));
    }
    candidates
        .into_iter()
        .find(|path| path.is_absolute() && file_name_matches(path, executable) && path.is_file())
}

fn executable_path_prefix(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if let Some(remainder) = value.strip_prefix('"') {
        let end = remainder.find('"')?;
        return Some(PathBuf::from(&remainder[..end]));
    }
    let lowercase = value.to_ascii_lowercase();
    let end = lowercase.find(".exe")? + ".exe".len();
    Some(PathBuf::from(value[..end].trim()))
}

fn file_name_matches(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

fn matches_registry_entry(product: ProductId, entry: &RegistryEntryOutput) -> bool {
    match product {
        ProductId::WorkBuddy => {
            matches_versioned_display_name(&entry.display_name, "WorkBuddy")
                && entry.publisher.as_deref().is_some_and(|publisher| {
                    publisher.eq_ignore_ascii_case("Tencent Technology (Shenzhen) Company Limited")
                })
        }
        ProductId::Hermes => {
            entry.display_name.eq_ignore_ascii_case("Hermes")
                || entry.display_name.eq_ignore_ascii_case("Hermes Agent")
        }
        ProductId::CcSwitch => {
            (entry.display_name.eq_ignore_ascii_case("CC Switch")
                || entry.display_name.eq_ignore_ascii_case("CCSwitch"))
                && entry
                    .publisher
                    .as_deref()
                    .is_some_and(|publisher| publisher.eq_ignore_ascii_case("ccswitch"))
        }
        ProductId::Claude => entry.display_name.eq_ignore_ascii_case("Claude"),
        ProductId::ChatGpt => {
            entry.display_name.eq_ignore_ascii_case("ChatGPT")
                || entry.display_name.eq_ignore_ascii_case("OpenAI.Codex")
        }
    }
}

fn matches_versioned_display_name(value: &str, base: &str) -> bool {
    if value.eq_ignore_ascii_case(base) {
        return true;
    }
    let Some(head) = value.get(..base.len()) else {
        return false;
    };
    if !head.eq_ignore_ascii_case(base) {
        return false;
    }
    let Some(version) = value
        .get(base.len()..)
        .and_then(|suffix| suffix.strip_prefix(' '))
    else {
        return false;
    };
    !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn compare_optional_versions(left: &Option<String>, right: &Option<String>) -> std::cmp::Ordering {
    let parse = |value: Option<&str>| -> Vec<u64> {
        value
            .unwrap_or_default()
            .split(|character: char| !character.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let mut left = parse(left.as_deref());
    let mut right = parse(right.as_deref());
    let length = left.len().max(right.len());
    left.resize(length, 0);
    right.resize(length, 0);
    left.cmp(&right)
}

fn parse_appx_architecture(value: &str) -> Option<Architecture> {
    match value.trim().to_ascii_lowercase().as_str() {
        "x64" => Some(Architecture::X64),
        "arm64" => Some(Architecture::Arm64),
        _ => None,
    }
}

pub fn verify_artifact(
    path: &Path,
    kind: PackageKind,
    trust: &TrustEntry,
    expected_architecture: Architecture,
    updater_signature_verified: bool,
) -> Result<ArtifactVerification, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("cannot stat artifact: {error}"))?;
    if !metadata.is_file() {
        return Err("artifact is not a regular file".into());
    }

    let executable_machine = if kind == PackageKind::Exe {
        Some(read_pe_machine(path)?)
    } else {
        None
    };
    let architecture = match kind {
        PackageKind::Exe => executable_machine.map(WindowsPeMachine::architecture),
        PackageKind::Msix => Some(inspect_msix(path)?.architecture),
        PackageKind::Msi => None,
        _ => return Err(format!("unsupported Windows package type: {kind:?}")),
    };
    let encoded = encode_powershell(VERIFY_SCRIPT);
    let powershell = trusted_powershell_program()?;
    let mut command = Command::new(powershell);
    hide_console_window(&mut command);
    command.env_remove("PSModulePath");
    let output = command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-OutputFormat",
            "Text",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .env("AI_CLIENT_INSTALLER_ARTIFACT", path)
        .env("AI_CLIENT_INSTALLER_PACKAGE_KIND", package_kind_key(kind))
        .output()
        .map_err(|error| format!("cannot start Windows verifier: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Windows verifier failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let parsed: VerificationOutput = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid verifier output: {error}"))?;
    let architecture = match kind {
        PackageKind::Msi => Some(parse_msi_template_architecture(
            parsed
                .template
                .as_deref()
                .ok_or_else(|| "MSI summary has no Template property".to_owned())?,
        )?),
        _ => architecture,
    };
    if kind == PackageKind::Exe {
        let expected_machine = expected_executable_machine(trust, expected_architecture)?;
        if executable_machine != Some(expected_machine) {
            return Err(format!(
                "artifact PE machine mismatch: expected {expected_machine:?} for {expected_architecture:?} target, got {executable_machine:?}"
            ));
        }
    } else if architecture != Some(expected_architecture) {
        return Err(format!(
            "artifact architecture mismatch: expected {expected_architecture:?}, got {architecture:?}"
        ));
    }
    if parsed.signature_status != "Valid" && !updater_signature_verified {
        return Err(format!(
            "artifact has neither a valid Authenticode/AppX signature nor a verified updater signature: {}",
            parsed.signature_status
        ));
    }

    if !trust.signer_subjects.is_empty() {
        let subject = parsed
            .signer_subject
            .as_deref()
            .ok_or_else(|| "signed artifact has no signer subject".to_owned())?;
        if !trust
            .signer_subjects
            .iter()
            .any(|expected| certificate_subject_contains(subject, expected))
        {
            return Err(format!("unexpected signer subject: {subject}"));
        }
    }

    let (product_identity, version) = if kind == PackageKind::Msix {
        let manifest = inspect_msix(path)?;
        let expected_publisher = trust
            .msix_publisher
            .as_deref()
            .ok_or_else(|| "trust registry has no pinned MSIX publisher".to_owned())?;
        if manifest.publisher != expected_publisher {
            return Err(format!(
                "MSIX publisher mismatch: expected {expected_publisher}, got {}",
                manifest.publisher
            ));
        }
        (manifest.name, Some(manifest.version))
    } else {
        (
            parsed.product.unwrap_or_default(),
            parsed.version.filter(|value| !value.trim().is_empty()),
        )
    };
    if let Some(expected_identity) = &trust.package_identity
        && !product_identity.eq_ignore_ascii_case(expected_identity)
    {
        return Err(format!(
            "package identity mismatch: expected {expected_identity}, got {product_identity}"
        ));
    }

    Ok(ArtifactVerification {
        signer_subject: parsed.signer_subject,
        product_identity,
        version,
        architecture,
    })
}

fn expected_executable_machine(
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<WindowsPeMachine, String> {
    trust
        .windows_exe_machine
        .or_else(|| WindowsPeMachine::for_architecture(expected_architecture))
        .ok_or_else(|| format!("unsupported target architecture: {expected_architecture:?}"))
}

pub fn plan_install_command(path: &Path, kind: PackageKind) -> Result<PlannedCommand, String> {
    let literal_path = path
        .to_str()
        .ok_or_else(|| "installer path is not valid Unicode".to_owned())?
        .to_owned();
    match kind {
        PackageKind::Exe => Ok(PlannedCommand {
            program: literal_path,
            arguments: Vec::new(),
            environment: Vec::new(),
        }),
        PackageKind::Msi => Ok(PlannedCommand {
            program: trusted_msiexec_program()?,
            arguments: vec!["/i".into(), literal_path, "/qn".into(), "/norestart".into()],
            environment: Vec::new(),
        }),
        PackageKind::Msix => Ok(PlannedCommand {
            program: trusted_powershell_program()?,
            arguments: vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-OutputFormat".into(),
                "Text".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-EncodedCommand".into(),
                encode_powershell(INSTALL_MSIX_SCRIPT),
            ],
            environment: vec![("AI_CLIENT_INSTALLER_ARTIFACT".into(), literal_path)],
        }),
        _ => Err(format!("unsupported Windows package type: {kind:?}")),
    }
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
    .map_err(|error| format!("verified artifact changed before execution: {error}"))?;
    let updater_signature_verified = match (
        request.trust.updater_public_key.as_deref(),
        request.detached_signature,
    ) {
        (Some(public_key), Some(signature)) => {
            crate::core::verify_minisign_file(request.path, public_key, signature)
                .map_err(|error| format!("updater signature changed before execution: {error}"))?;
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
    let rebound = verify_staged_identity(
        request.private_root,
        request.path,
        request.verified_identity,
        request.expected_sha256,
    )
    .map_err(|error| format!("artifact changed at execution handoff: {error}"))?;
    if rebound.sha256 != request.verified_identity.sha256 {
        return Err("artifact digest changed at execution handoff".into());
    }

    let plan = plan_install_command(request.path, request.kind)?;
    let mut command = Command::new(&plan.program);
    hide_console_window(&mut command);
    command.args(&plan.arguments);
    if is_powershell_program(&plan.program) {
        command.env_remove("PSModulePath");
    }
    for (key, value) in &plan.environment {
        command.env(key, value);
    }
    let (exit_code, error_summary) = if request.kind == PackageKind::Msix {
        let output = command
            .output()
            .map_err(|error| format!("cannot start installer: {error}"))?;
        let exit_code = output.status.code().unwrap_or(-1);
        let error_summary = if output.status.success() {
            None
        } else {
            summarize_installer_error(&output.stdout, &output.stderr)
        };
        (exit_code, error_summary)
    } else {
        let status = command
            .status()
            .map_err(|error| format!("cannot start installer: {error}"))?;
        let exit_code = status.code().unwrap_or(-1);
        (exit_code, known_installer_error(request.kind, exit_code))
    };
    Ok(InstallerExecution {
        exit_code,
        error_summary,
    })
}

fn known_installer_error(kind: PackageKind, exit_code: i32) -> Option<String> {
    match (kind, exit_code) {
        (PackageKind::Msi, 1602) => Some("用户取消了 Windows Installer 操作".into()),
        (PackageKind::Msi, 1603) => Some("Windows Installer 报告致命安装错误".into()),
        (PackageKind::Msi, 1618) => Some("另一项 Windows Installer 操作正在进行".into()),
        _ => None,
    }
}

fn summarize_installer_error(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let stderr = String::from_utf8_lossy(stderr);
    let stdout = String::from_utf8_lossy(stdout);
    let raw = if stderr.trim().is_empty() {
        stdout.trim().to_owned()
    } else if stdout.trim().is_empty() {
        stderr.trim().to_owned()
    } else {
        format!("{} | stdout: {}", stderr.trim(), stdout.trim())
    };
    if raw.is_empty() {
        return None;
    }
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut summary: String = normalized.chars().take(MAX_INSTALLER_ERROR_CHARS).collect();
    if normalized.chars().count() > MAX_INSTALLER_ERROR_CHARS {
        summary.push('…');
    }
    Some(summary)
}

fn package_kind_key(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::Exe => "exe",
        PackageKind::Msi => "msi",
        PackageKind::Msix => "msix",
        _ => "unsupported",
    }
}

fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn certificate_subject_contains(subject: &str, expected: &str) -> bool {
    subject.split(',').any(|component| {
        component
            .split_once('=')
            .map(|(_, value)| value.trim().eq_ignore_ascii_case(expected))
            .unwrap_or(false)
    })
}

pub(crate) fn parse_msi_template_architecture(template: &str) -> Result<Architecture, String> {
    let platform = template
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match platform.as_str() {
        "x64" | "amd64" | "intel64" => Ok(Architecture::X64),
        "arm64" => Ok(Architecture::Arm64),
        "intel" | "x86" => Err("32-bit MSI packages are outside V1 scope".into()),
        _ => Err(format!("unsupported MSI Template platform: {platform}")),
    }
}

fn read_pe_machine(path: &Path) -> Result<WindowsPeMachine, String> {
    let mut file = File::open(path).map_err(|error| format!("cannot open PE file: {error}"))?;
    let mut dos_header = [0_u8; 64];
    file.read_exact(&mut dos_header)
        .map_err(|error| format!("cannot read PE header: {error}"))?;
    if &dos_header[0..2] != b"MZ" {
        return Err("file has no MZ header".into());
    }
    let offset = u32::from_le_bytes(dos_header[60..64].try_into().unwrap()) as u64;
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(offset))
        .map_err(|error| format!("cannot seek to PE header: {error}"))?;
    let mut pe = [0_u8; 6];
    file.read_exact(&mut pe)
        .map_err(|error| format!("cannot read PE signature: {error}"))?;
    if &pe[0..4] != b"PE\0\0" {
        return Err("file has no PE signature".into());
    }
    match u16::from_le_bytes([pe[4], pe[5]]) {
        0x014c => Ok(WindowsPeMachine::X86),
        0x8664 => Ok(WindowsPeMachine::X64),
        0xaa64 => Ok(WindowsPeMachine::Arm64),
        machine => Err(format!("unsupported PE machine 0x{machine:04x}")),
    }
}

struct MsixIdentity {
    name: String,
    publisher: String,
    version: String,
    architecture: Architecture,
}

fn inspect_msix(path: &Path) -> Result<MsixIdentity, String> {
    let file = File::open(path).map_err(|error| format!("cannot open MSIX: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("invalid MSIX ZIP: {error}"))?;
    let mut manifest = archive
        .by_name("AppxManifest.xml")
        .map_err(|error| format!("MSIX has no AppxManifest.xml: {error}"))?;
    if manifest.size() > 2 * 1024 * 1024 {
        return Err("MSIX manifest exceeds 2 MiB".into());
    }
    let mut xml = String::new();
    manifest
        .read_to_string(&mut xml)
        .map_err(|error| format!("cannot read MSIX manifest: {error}"))?;
    let identity_tag = Regex::new(r"(?is)<Identity\b[^>]*>")
        .expect("static identity regex")
        .find(&xml)
        .map(|value| value.as_str())
        .ok_or_else(|| "MSIX manifest has no Identity tag".to_owned())?;
    let attr = |name: &str| -> Result<String, String> {
        let pattern = format!(r#"(?i)\b{}\s*=\s*[\"']([^\"']+)[\"']"#, regex::escape(name));
        Regex::new(&pattern)
            .map_err(|error| error.to_string())?
            .captures(identity_tag)
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().to_owned())
            .ok_or_else(|| format!("MSIX Identity is missing {name}"))
    };
    let architecture = match attr("ProcessorArchitecture")?.to_ascii_lowercase().as_str() {
        "x64" => Architecture::X64,
        "arm64" => Architecture::Arm64,
        value => return Err(format!("unsupported MSIX architecture: {value}")),
    };
    Ok(MsixIdentity {
        name: attr("Name")?,
        publisher: attr("Publisher")?,
        version: attr("Version")?,
        architecture,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::{
        INSTALL_MSIX_SCRIPT, RegistryEntryOutput, detect_hermes_fixed_install_at_with,
        detect_product, encode_powershell, expected_executable_machine, hide_console_window,
        known_installer_error, matches_registry_entry, parse_msi_template_architecture,
        select_registry_detection, summarize_installer_error, trusted_msiexec_program,
        trusted_powershell_program, verify_artifact,
    };
    use crate::core::{
        Architecture, OperatingSystem, PackageKind, ProductId, TrustRegistry, WindowsPeMachine,
    };

    fn write_minimal_pe(path: &Path, machine: u16) {
        let mut bytes = vec![0_u8; 70];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&machine.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    fn write_hermes_fixed_install(root: &Path, origin: &str) {
        let hermes_root = root.join("hermes");
        fs::create_dir_all(&hermes_root).unwrap();
        write_minimal_pe(&hermes_root.join("hermes-setup.exe"), 0x8664);
        let install_root = hermes_root.join("hermes-agent");
        let executable = install_root
            .join("apps")
            .join("desktop")
            .join("release")
            .join("win-unpacked")
            .join("Hermes.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::create_dir_all(install_root.join(".git")).unwrap();
        fs::create_dir_all(install_root.join("hermes_cli")).unwrap();
        write_minimal_pe(&executable, 0x8664);
        fs::write(
            install_root
                .join("apps")
                .join("desktop")
                .join("package.json"),
            r#"{
  "name": "hermes",
  "productName": "Hermes",
  "author": "Nous Research",
  "build": { "appId": "com.nousresearch.hermes" }
}"#,
        )
        .unwrap();
        fs::write(
            install_root.join(".git").join("config"),
            format!("[remote \"origin\"]\n\turl = {origin}\n"),
        )
        .unwrap();
        fs::write(
            install_root.join("hermes_cli").join("__init__.py"),
            "__version__ = \"0.19.1\"\n",
        )
        .unwrap();
    }

    fn workbuddy_registry_entry(display_icon: Option<String>) -> RegistryEntryOutput {
        RegistryEntryOutput {
            display_name: "WorkBuddy 5.3.8.34705286".into(),
            version: Some("5.3.8.34705286".into()),
            publisher: Some("Tencent Technology (Shenzhen) Company Limited".into()),
            install_location: None,
            display_icon,
            uninstall_string: None,
            current_user: true,
        }
    }

    #[test]
    fn parses_msi_template_architecture() {
        assert_eq!(
            parse_msi_template_architecture("x64;1033").unwrap(),
            Architecture::X64
        );
        assert_eq!(
            parse_msi_template_architecture("Arm64;1033").unwrap(),
            Architecture::Arm64
        );
        assert!(parse_msi_template_architecture("Intel;1033").is_err());
    }

    #[test]
    fn msix_install_uses_the_supported_path_parameter_and_closes_the_target_app() {
        assert!(INSTALL_MSIX_SCRIPT.contains("Add-AppxPackage -Path"));
        assert!(INSTALL_MSIX_SCRIPT.contains("-ForceTargetApplicationShutdown"));
        assert!(!INSTALL_MSIX_SCRIPT.contains("-LiteralPath"));
    }

    #[test]
    fn installer_failure_summary_is_normalized_and_bounded() {
        let summary = summarize_installer_error(b"stdout line\nnext", b"stderr\r\nreason")
            .expect("failure output");
        assert_eq!(summary, "stderr reason | stdout: stdout line next");
        let long = summarize_installer_error(&[], "x".repeat(5000).as_bytes()).unwrap();
        assert_eq!(long.chars().count(), 4097);
        assert!(long.ends_with('…'));
        assert_eq!(
            known_installer_error(PackageKind::Msi, 1618).as_deref(),
            Some("另一项 Windows Installer 操作正在进行")
        );
    }

    #[test]
    fn hermes_fixed_install_detection_requires_the_official_layout_and_origin() {
        let root = tempfile::tempdir().unwrap();
        write_hermes_fixed_install(
            root.path(),
            "https://github.com/NousResearch/hermes-agent.git",
        );
        let detection =
            detect_hermes_fixed_install_at_with(root.path(), Architecture::X64, |setup| {
                setup.file_name().and_then(|name| name.to_str()) == Some("hermes-setup.exe")
            })
            .unwrap();
        assert_eq!(detection.version.as_deref(), Some("0.19.1"));
        assert_eq!(detection.architecture, Some(Architecture::X64));
        assert_eq!(
            detection.package_identity.as_deref(),
            Some("com.nousresearch.hermes")
        );

        let unrelated = tempfile::tempdir().unwrap();
        write_hermes_fixed_install(unrelated.path(), "https://example.invalid/fake.git");
        assert!(
            detect_hermes_fixed_install_at_with(unrelated.path(), Architecture::X64, |_| true)
                .is_none()
        );
        assert!(
            detect_hermes_fixed_install_at_with(root.path(), Architecture::X64, |_| false)
                .is_none()
        );
    }

    #[test]
    fn executable_machine_override_is_scoped_to_the_workbuddy_trust_entry() {
        let registry = TrustRegistry::embedded().unwrap();
        let workbuddy = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        let hermes = registry
            .find(
                ProductId::Hermes,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        assert_eq!(
            expected_executable_machine(workbuddy, Architecture::X64).unwrap(),
            WindowsPeMachine::X86
        );
        assert_eq!(
            expected_executable_machine(hermes, Architecture::X64).unwrap(),
            WindowsPeMachine::X64
        );
    }

    #[test]
    fn system_tools_are_resolved_to_absolute_system_directory_paths() {
        for program in [
            trusted_powershell_program().unwrap(),
            trusted_msiexec_program().unwrap(),
        ] {
            let path = Path::new(&program);
            assert!(path.is_absolute());
            assert!(path.is_file());
        }
    }

    #[test]
    fn background_commands_do_not_attach_a_console_window() {
        let script = r#"
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class NativeConsoleProbe {
    [DllImport("kernel32.dll")]
    public static extern IntPtr GetConsoleWindow();
}
'@
if ([NativeConsoleProbe]::GetConsoleWindow() -ne [IntPtr]::Zero) { exit 42 }
"#;
        let powershell = trusted_powershell_program().unwrap();
        let mut command = Command::new(&powershell);
        hide_console_window(&mut command);
        command.env_remove("PSModulePath");
        command.stdout(Stdio::null()).stderr(Stdio::null());
        let status = command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
                &encode_powershell(script),
            ])
            .status()
            .unwrap();
        assert!(
            status.success(),
            "background child attached a console window"
        );
    }

    #[test]
    fn workbuddy_registry_rule_accepts_numeric_versions_and_exact_publisher() {
        let valid = RegistryEntryOutput {
            display_name: "WorkBuddy 5.1.7".into(),
            version: Some("5.1.7".into()),
            publisher: Some("Tencent Technology (Shenzhen) Company Limited".into()),
            install_location: None,
            display_icon: None,
            uninstall_string: None,
            current_user: true,
        };
        assert!(matches_registry_entry(ProductId::WorkBuddy, &valid));

        let wrong_name = RegistryEntryOutput {
            display_name: "WorkBuddy Preview".into(),
            version: valid.version.clone(),
            publisher: valid.publisher.clone(),
            install_location: None,
            display_icon: None,
            uninstall_string: None,
            current_user: valid.current_user,
        };
        assert!(!matches_registry_entry(ProductId::WorkBuddy, &wrong_name));

        let wrong_publisher = RegistryEntryOutput {
            display_name: valid.display_name,
            version: valid.version,
            publisher: Some("Unrelated Publisher".into()),
            install_location: None,
            display_icon: None,
            uninstall_string: None,
            current_user: true,
        };
        assert!(!matches_registry_entry(
            ProductId::WorkBuddy,
            &wrong_publisher
        ));
    }

    #[test]
    fn cc_switch_registry_rule_requires_the_official_msi_publisher() {
        let valid = RegistryEntryOutput {
            display_name: "CC Switch".into(),
            version: Some("3.17.0".into()),
            publisher: Some("ccswitch".into()),
            install_location: None,
            display_icon: None,
            uninstall_string: None,
            current_user: false,
        };
        assert!(matches_registry_entry(ProductId::CcSwitch, &valid));

        let mut wrong_publisher = valid;
        wrong_publisher.publisher = Some("Unrelated Publisher".into());
        assert!(!matches_registry_entry(
            ProductId::CcSwitch,
            &wrong_publisher
        ));
    }

    #[test]
    fn registry_detection_selects_the_highest_numeric_version() {
        let detection = select_registry_detection(
            ProductId::WorkBuddy,
            vec![
                RegistryEntryOutput {
                    display_name: "WorkBuddy 5.9.0".into(),
                    version: Some("5.9.0".into()),
                    publisher: Some("Tencent Technology (Shenzhen) Company Limited".into()),
                    install_location: None,
                    display_icon: None,
                    uninstall_string: None,
                    current_user: true,
                },
                RegistryEntryOutput {
                    display_name: "WorkBuddy 5.10.0".into(),
                    version: Some("5.10.0".into()),
                    publisher: Some("Tencent Technology (Shenzhen) Company Limited".into()),
                    install_location: None,
                    display_icon: None,
                    uninstall_string: None,
                    current_user: true,
                },
            ],
            None,
        )
        .unwrap();
        assert_eq!(detection.version.as_deref(), Some("5.10.0"));
        assert!(detection.management_known);
        assert!(detection.evidence.contains("WorkBuddy 5.10.0"));
    }

    #[test]
    fn registry_detection_reads_the_pinned_workbuddy_application_architecture() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("WorkBuddy.exe");
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();

        write_minimal_pe(&executable, 0x8664);
        let display_icon = Some(format!("{},0", executable.display()));
        let detection = select_registry_detection(
            ProductId::WorkBuddy,
            vec![workbuddy_registry_entry(display_icon.clone())],
            Some(trust),
        )
        .unwrap();
        assert_eq!(detection.architecture, Some(Architecture::X64));

        write_minimal_pe(&executable, 0x014c);
        let detection = select_registry_detection(
            ProductId::WorkBuddy,
            vec![workbuddy_registry_entry(display_icon)],
            Some(trust),
        )
        .unwrap();
        assert_eq!(detection.architecture, Some(Architecture::Unsupported));
    }

    #[test]
    fn registry_detection_uses_install_location_and_uninstall_string_fallbacks() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("WorkBuddy.exe");
        write_minimal_pe(&executable, 0x8664);
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();

        let mut install_location = workbuddy_registry_entry(None);
        install_location.install_location = Some(directory.path().display().to_string());
        let detection =
            select_registry_detection(ProductId::WorkBuddy, vec![install_location], Some(trust))
                .unwrap();
        assert_eq!(detection.architecture, Some(Architecture::X64));

        let mut uninstall_string = workbuddy_registry_entry(None);
        uninstall_string.uninstall_string = Some(format!(
            "\"{}\" /currentuser",
            directory.path().join("Uninstall WorkBuddy.exe").display()
        ));
        let detection =
            select_registry_detection(ProductId::WorkBuddy, vec![uninstall_string], Some(trust))
                .unwrap();
        assert_eq!(detection.architecture, Some(Architecture::X64));
    }

    #[test]
    fn registry_detection_prefers_install_location_and_rejects_malformed_target_pe() {
        let preferred = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let preferred_executable = preferred.path().join("WorkBuddy.exe");
        let fallback_executable = fallback.path().join("WorkBuddy.exe");
        fs::write(&preferred_executable, b"not a PE file").unwrap();
        write_minimal_pe(&fallback_executable, 0x8664);
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        let mut entry =
            workbuddy_registry_entry(Some(format!("{},0", fallback_executable.display())));
        entry.install_location = Some(preferred.path().display().to_string());
        let detection =
            select_registry_detection(ProductId::WorkBuddy, vec![entry], Some(trust)).unwrap();
        assert_eq!(detection.architecture, Some(Architecture::Unsupported));
    }

    #[test]
    #[ignore = "requires AI_CLIENT_INSTALLER_WORKBUDDY_PACKAGE to point to a current official package"]
    fn current_official_workbuddy_bootstrap_matches_embedded_trust() {
        let path = std::env::var_os("AI_CLIENT_INSTALLER_WORKBUDDY_PACKAGE")
            .map(std::path::PathBuf::from)
            .expect("AI_CLIENT_INSTALLER_WORKBUDDY_PACKAGE is required");
        let registry = TrustRegistry::embedded().unwrap();
        let trust = registry
            .find(
                ProductId::WorkBuddy,
                OperatingSystem::Windows,
                Architecture::X64,
            )
            .unwrap();
        assert_eq!(trust.windows_exe_machine, Some(WindowsPeMachine::X86));
        let verification =
            verify_artifact(&path, PackageKind::Exe, trust, Architecture::X64, false).unwrap();
        assert_eq!(verification.product_identity, "WorkBuddy");
        assert_eq!(verification.architecture, Some(Architecture::Unsupported));
        assert!(verification.signer_subject.is_some());
    }

    #[test]
    #[ignore = "reads the current Windows host registration state"]
    fn current_host_detection_script_is_parseable_and_finds_workbuddy() {
        let registry = TrustRegistry::embedded().unwrap();
        for product in ProductId::ALL {
            let trust = registry.find(product, OperatingSystem::Windows, Architecture::X64);
            let detection = detect_product(product, trust).unwrap();
            if product == ProductId::WorkBuddy {
                assert!(detection.installed, "{}", detection.evidence);
                assert!(detection.management_known);
                assert_eq!(
                    detection.publisher.as_deref(),
                    Some("Tencent Technology (Shenzhen) Company Limited")
                );
                assert_eq!(detection.architecture, Some(Architecture::X64));
            }
            if product == ProductId::Hermes {
                assert!(detection.installed, "{}", detection.evidence);
                assert_eq!(detection.version.as_deref(), Some("0.19.1"));
                assert_eq!(detection.architecture, Some(Architecture::X64));
            }
        }
    }
}
