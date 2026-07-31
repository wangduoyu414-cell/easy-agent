use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

use base64::Engine;
use regex::Regex;
use serde::Deserialize;
use zip::ZipArchive;

use crate::core::{
    Architecture, Detection, PackageKind, ProductId, StableFileIdentity, TrustEntry,
    verify_staged_identity,
};

use super::{ArtifactVerification, PlannedCommand};

const DETECTION_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$product = $env:AI_CLIENT_INSTALLER_PRODUCT
$patterns = switch ($product) {
  'workbuddy' { @('WorkBuddy') }
  'hermes' { @('Hermes', 'Hermes Agent') }
  'cc_switch' { @('CC Switch', 'CCSwitch') }
  'claude' { @('Claude') }
  'chatgpt' { @('ChatGPT', 'OpenAI.Codex') }
  default { @() }
}

$result = $null
if ($product -eq 'claude') {
  $pkg = Get-AppxPackage -Name Claude -ErrorAction SilentlyContinue | Sort-Object Version -Descending | Select-Object -First 1
  if ($pkg) { $result = [pscustomobject]@{ installed=$true; version=$pkg.Version.ToString(); managed=[bool]$pkg.NonRemovable; management_known=[bool]$pkg.NonRemovable; evidence=('AppX:' + $pkg.PackageFamilyName) } }
}
if ($product -eq 'chatgpt') {
  $pkg = Get-AppxPackage -ErrorAction SilentlyContinue | Where-Object { $_.PackageFamilyName -eq 'OpenAI.Codex_2p2nqsd0c76g0' } | Sort-Object Version -Descending | Select-Object -First 1
  if ($pkg) { $result = [pscustomobject]@{ installed=$true; version=$pkg.Version.ToString(); managed=[bool]$pkg.NonRemovable; management_known=[bool]$pkg.NonRemovable; evidence=('AppX:' + $pkg.PackageFamilyName) } }
}

if (-not $result) {
  $roots = @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
  )
  $entry = Get-ItemProperty -Path $roots -ErrorAction SilentlyContinue |
    Where-Object { $name = $_.DisplayName; $patterns | Where-Object { $name -eq $_ } } |
    Sort-Object DisplayVersion -Descending | Select-Object -First 1
  if ($entry) {
    $isCurrentUser = ([string]$entry.PSPath).StartsWith('Microsoft.PowerShell.Core\Registry::HKEY_CURRENT_USER\', [System.StringComparison]::OrdinalIgnoreCase)
    $result = [pscustomobject]@{ installed=$true; version=[string]$entry.DisplayVersion; managed=$false; management_known=$isCurrentUser; evidence=('Uninstall:' + $entry.DisplayName) }
  }
}

if (-not $result) { $result = [pscustomobject]@{ installed=$false; version=$null; managed=$false; management_known=$true; evidence='No exact registered identity found' } }
$result | ConvertTo-Json -Compress
"#;

const VERIFY_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
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
Add-AppxPackage -LiteralPath $env:AI_CLIENT_INSTALLER_ARTIFACT -ErrorAction Stop
"#;

#[derive(Debug, Deserialize)]
struct DetectionOutput {
    installed: bool,
    version: Option<String>,
    managed: bool,
    management_known: bool,
    evidence: String,
}

#[derive(Debug, Deserialize)]
struct VerificationOutput {
    signature_status: String,
    signer_subject: Option<String>,
    product: Option<String>,
    version: Option<String>,
    template: Option<String>,
}

pub fn detect_product(product: ProductId) -> Result<Detection, io::Error> {
    let bytes: Vec<u8> = DETECTION_SCRIPT
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
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
    Ok(Detection {
        installed: parsed.installed,
        version: parsed.version,
        managed: parsed.managed,
        management_known: parsed.management_known,
        evidence: parsed.evidence,
    })
}

pub fn verify_artifact(
    path: &Path,
    kind: PackageKind,
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<ArtifactVerification, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("cannot stat artifact: {error}"))?;
    if !metadata.is_file() {
        return Err("artifact is not a regular file".into());
    }

    let architecture = match kind {
        PackageKind::Exe => Some(read_pe_architecture(path)?),
        PackageKind::Msix => Some(inspect_msix(path)?.architecture),
        PackageKind::Msi => None,
        _ => return Err(format!("unsupported Windows package type: {kind:?}")),
    };
    let encoded = encode_powershell(VERIFY_SCRIPT);
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
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
    if architecture != Some(expected_architecture) {
        return Err(format!(
            "artifact architecture mismatch: expected {expected_architecture:?}, got {architecture:?}"
        ));
    }
    if parsed.signature_status != "Valid" {
        return Err(format!(
            "Authenticode/AppX signature is not valid: {}",
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
            program: "msiexec.exe".into(),
            arguments: vec!["/i".into(), literal_path],
            environment: Vec::new(),
        }),
        PackageKind::Msix => Ok(PlannedCommand {
            program: "powershell.exe".into(),
            arguments: vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
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
    private_root: &Path,
    path: &Path,
    verified_identity: &StableFileIdentity,
    expected_sha256: Option<&str>,
    kind: PackageKind,
    trust: &TrustEntry,
    expected_architecture: Architecture,
) -> Result<i32, String> {
    if !trust.enabled {
        return Err(format!(
            "installation is disabled by the embedded trust registry: {}",
            trust.status_reason
        ));
    }
    verify_staged_identity(private_root, path, verified_identity, expected_sha256)
        .map_err(|error| format!("verified artifact changed before execution: {error}"))?;
    verify_artifact(path, kind, trust, expected_architecture)?;
    let rebound = verify_staged_identity(private_root, path, verified_identity, expected_sha256)
        .map_err(|error| format!("artifact changed at execution handoff: {error}"))?;
    if rebound.sha256 != verified_identity.sha256 {
        return Err("artifact digest changed at execution handoff".into());
    }

    let plan = plan_install_command(path, kind)?;
    let mut command = Command::new(&plan.program);
    command.args(&plan.arguments);
    for (key, value) in &plan.environment {
        command.env(key, value);
    }
    let status = command
        .status()
        .map_err(|error| format!("cannot start installer: {error}"))?;
    Ok(status.code().unwrap_or(-1))
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

fn read_pe_architecture(path: &Path) -> Result<Architecture, String> {
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
        0x8664 => Ok(Architecture::X64),
        0xaa64 => Ok(Architecture::Arm64),
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
    use super::parse_msi_template_architecture;
    use crate::core::Architecture;

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
}
