use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use reqwest::header::CONTENT_LENGTH;
use tempfile::TempDir;
use thiserror::Error;
use url::Url;

use super::security::{SecurityError, StableFileIdentity, inspect_staged_file};
use super::{TrustEntry, ensure_allowed_url, safe_http_client, validate_staged_file_name};

const MAX_INSTALLER_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

#[derive(Debug, Clone)]
pub struct DownloadRequest<'a> {
    pub url: Url,
    pub file_name: String,
    pub trust: &'a TrustEntry,
}

pub struct DownloadControl<'a> {
    pub is_cancelled: &'a dyn Fn() -> bool,
    pub on_progress: &'a dyn Fn(u64, Option<u64>),
}

#[derive(Debug)]
pub struct DownloadResult {
    pub private_root: TempDir,
    pub staged_path: PathBuf,
    pub identity: StableFileIdentity,
    pub final_url: Url,
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error(transparent)]
    Security(#[from] SecurityError),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error("redirect limit exceeded")]
    RedirectLimit,
    #[error("download is larger than the 2 GiB safety limit")]
    TooLarge,
    #[error("server returned HTTP {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("download cancelled")]
    Cancelled,
}

pub fn download_to_private_staging(
    request: &DownloadRequest<'_>,
) -> Result<DownloadResult, DownloadError> {
    download_to_private_staging_controlled(
        request,
        &DownloadControl {
            is_cancelled: &|| false,
            on_progress: &|_, _| {},
        },
    )
}

pub fn download_to_private_staging_controlled(
    request: &DownloadRequest<'_>,
    control: &DownloadControl<'_>,
) -> Result<DownloadResult, DownloadError> {
    validate_staged_file_name(&request.file_name)?;
    ensure_allowed_url(&request.url, request.trust)?;
    let client = safe_http_client()?;
    let private_root = tempfile::Builder::new()
        .prefix("ai-client-installer-")
        .tempdir()?;
    let part_path = private_root
        .path()
        .join(format!("{}.part", request.file_name));
    let staged_path = private_root.path().join(&request.file_name);

    let mut current = request.url.clone();
    let mut response = None;
    for _ in 0..=MAX_REDIRECTS {
        if (control.is_cancelled)() {
            return Err(DownloadError::Cancelled);
        }
        ensure_allowed_url(&current, request.trust)?;
        let next = client.get(current.clone()).send()?;
        if next.status().is_redirection() {
            let location = next
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(DownloadError::RedirectLimit)?;
            current = current.join(location)?;
            continue;
        }
        response = Some(next);
        break;
    }
    let mut response = response.ok_or(DownloadError::RedirectLimit)?;
    ensure_allowed_url(response.url(), request.trust)?;
    if !response.status().is_success() {
        return Err(DownloadError::HttpStatus(response.status()));
    }
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if content_length.is_some_and(|length| length > MAX_INSTALLER_BYTES) {
        return Err(DownloadError::TooLarge);
    }

    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&part_path)?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        if (control.is_cancelled)() {
            return Err(DownloadError::Cancelled);
        }
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_INSTALLER_BYTES {
            return Err(DownloadError::TooLarge);
        }
        output.write_all(&buffer[..read])?;
        (control.on_progress)(total, content_length);
    }
    output.sync_all()?;
    drop(output);
    fs::rename(&part_path, &staged_path)?;
    let identity = inspect_staged_file(private_root.path(), &staged_path)?;

    Ok(DownloadResult {
        private_root,
        staged_path,
        identity,
        final_url: current,
    })
}
