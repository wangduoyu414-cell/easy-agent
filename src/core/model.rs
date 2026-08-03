use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductId {
    WorkBuddy,
    Hermes,
    CcSwitch,
    Claude,
    ChatGpt,
}

impl ProductId {
    pub const ALL: [Self; 5] = [
        Self::Hermes,
        Self::Claude,
        Self::ChatGpt,
        Self::WorkBuddy,
        Self::CcSwitch,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::WorkBuddy => "workbuddy",
            Self::Hermes => "hermes",
            Self::CcSwitch => "cc_switch",
            Self::Claude => "claude",
            Self::ChatGpt => "chatgpt",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::WorkBuddy => "WorkBuddy",
            Self::Hermes => "Hermes Agent",
            Self::CcSwitch => "CC Switch",
            Self::Claude => "Claude Desktop",
            Self::ChatGpt => "ChatGPT",
        }
    }
}

impl fmt::Display for ProductId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatingSystem {
    Windows,
    #[serde(rename = "macos")]
    MacOs,
    Unsupported,
}

impl OperatingSystem {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X64,
    Arm64,
    Unsupported,
}

impl Architecture {
    pub const fn key(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::Arm64 => "arm64",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformInfo {
    pub os: OperatingSystem,
    pub architecture: Architecture,
    pub os_version: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    Exe,
    Msi,
    Msix,
    Dmg,
    TarGz,
    Zip,
}

impl PackageKind {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Exe => "exe",
            Self::Msi => "msi",
            Self::Msix => "msix",
            Self::Dmg => "dmg",
            Self::TarGz => "tar.gz",
            Self::Zip => "zip",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportState {
    Ready,
    Disabled(String),
    Unsupported(String),
}

impl SupportState {
    pub fn can_install(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Ready => "可安装",
            Self::Disabled(_) => "验证待完成",
            Self::Unsupported(_) => "不支持",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Ready => None,
            Self::Disabled(reason) | Self::Unsupported(reason) => Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub installed: bool,
    pub version: Option<String>,
    pub managed: bool,
    pub management_known: bool,
    pub package_identity: Option<String>,
    pub package_family: Option<String>,
    pub publisher: Option<String>,
    pub architecture: Option<Architecture>,
    pub evidence: String,
}

impl Detection {
    pub fn absent(evidence: impl Into<String>) -> Self {
        Self {
            installed: false,
            version: None,
            managed: false,
            management_known: true,
            package_identity: None,
            package_family: None,
            publisher: None,
            architecture: None,
            evidence: evidence.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseCandidate {
    pub product: ProductId,
    pub version: String,
    pub architecture: Architecture,
    pub package_kind: PackageKind,
    pub download_url: url::Url,
    pub expected_sha256: Option<String>,
    pub detached_signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrosoftStorePlan {
    pub product: ProductId,
    pub architecture: Architecture,
    pub store_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallPlan {
    DirectPackage(ReleaseCandidate),
    MicrosoftStore(MicrosoftStorePlan),
}

impl InstallPlan {
    pub const fn product(&self) -> ProductId {
        match self {
            Self::DirectPackage(candidate) => candidate.product,
            Self::MicrosoftStore(plan) => plan.product,
        }
    }

    pub const fn architecture(&self) -> Architecture {
        match self {
            Self::DirectPackage(candidate) => candidate.architecture,
            Self::MicrosoftStore(plan) => plan.architecture,
        }
    }

    pub fn target_version(&self) -> Option<&str> {
        match self {
            Self::DirectPackage(candidate) => Some(&candidate.version),
            Self::MicrosoftStore(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProductView {
    pub product: ProductId,
    pub selected: bool,
    pub support: SupportState,
    pub detection: Detection,
    pub install_plan: Option<InstallPlan>,
    pub result_unknown: bool,
    pub status_line: String,
    pub staged_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationState {
    Ready,
    Downloading,
    Verifying,
    AwaitingUserInstall,
    Installing,
    Postchecking,
    Succeeded,
    ResultUnknown,
    Failed,
    Cancelled,
}

impl OperationState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "准备安装",
            Self::Downloading => "正在下载",
            Self::Verifying => "正在验证",
            Self::AwaitingUserInstall => "等待厂商安装器",
            Self::Installing => "正在安装",
            Self::Postchecking => "正在复检",
            Self::Succeeded => "安装成功",
            Self::ResultUnknown => "结果待复检",
            Self::Failed => "安装失败",
            Self::Cancelled => "已取消",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OperationUpdate {
    pub product: ProductId,
    pub state: OperationState,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProductOperationResult {
    pub product: ProductId,
    pub state: OperationState,
    pub message: String,
}
