mod download;
mod http;
mod model;
mod orchestrator;
mod security;
mod trust;
mod verification;

pub use download::{
    DownloadControl, DownloadError, DownloadRequest, DownloadResult, download_to_private_staging,
    download_to_private_staging_controlled,
};
pub use http::{
    HttpError, fetch_official_bytes, fetch_official_text, resolve_official_url, safe_http_client,
};
pub use model::{
    Architecture, Detection, OperatingSystem, OperationState, OperationUpdate, PackageKind,
    PlatformInfo, ProductId, ProductOperationResult, ProductView, ReleaseCandidate, SupportState,
};
pub use orchestrator::{
    PreinstallDecision, assess_existing_install, run_install_batch, version_is_older,
};
pub use security::{
    SecurityError, StableFileIdentity, ensure_allowed_url, inspect_staged_file, sha256_file,
    validate_staged_file_name, verify_staged_identity,
};
pub use trust::{TrustEntry, TrustRegistry, TrustRegistryError, UrlRule};
pub use verification::{VerificationError, verify_minisign_file};
