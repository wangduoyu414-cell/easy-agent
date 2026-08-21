use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, ETAG, HeaderValue, IF_RANGE, LOCATION, RANGE,
};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use thiserror::Error;
use url::Url;

use super::security::{
    SecurityError, StableFileIdentity, inspect_staged_file, sha256_file, verify_staged_identity,
};
use super::{
    UrlRule, ensure_allowed_url_against_rules, safe_artifact_client, validate_staged_file_name,
};

const MAX_INSTALLER_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const MAX_DOWNLOAD_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct DownloadRequest<'a> {
    pub url: Url,
    pub file_name: String,
    pub url_rules: &'a [UrlRule],
    pub expected_size: Option<u64>,
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
    #[error("network read failed: {0}")]
    NetworkRead(#[source] io::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error("redirect response has no valid Location header")]
    MissingRedirectLocation,
    #[error("redirect limit exceeded")]
    RedirectLimit,
    #[error("download is larger than the 2 GiB safety limit")]
    TooLarge,
    #[error("server returned HTTP {0}")]
    HttpStatus(StatusCode),
    #[error("server returned an invalid Content-Range header")]
    InvalidContentRange,
    #[error("the resumable download object changed")]
    ResumeObjectChanged,
    #[error("server rejected the requested resume range")]
    RangeNotSatisfiable,
    #[error("download length mismatch: expected {expected} bytes, received {actual}")]
    LengthMismatch { expected: u64, actual: u64 },
    #[error("server-declared size {declared} does not match expected size {expected}")]
    ExpectedSizeMismatch { expected: u64, declared: u64 },
    #[error("download cancelled")]
    Cancelled,
    #[error("verified download copy does not match the private staged file")]
    VisibleCopyMismatch,
}

struct ResponsePlan {
    append: bool,
    initial_length: u64,
    expected_total: Option<u64>,
    expected_body_length: Option<u64>,
}

#[derive(Default)]
struct ResumeState {
    bound_url: Option<Url>,
    validator: Option<HeaderValue>,
    expected_total: Option<u64>,
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
    ensure_allowed_url_against_rules(&request.url, request.url_rules)?;
    let client = safe_artifact_client()?;
    let private_root = tempfile::Builder::new().prefix("easy-agent-").tempdir()?;
    let part_path = private_root
        .path()
        .join(format!("{}.part", request.file_name));
    let staged_path = private_root.path().join(&request.file_name);
    let mut partial_file = open_new_partial_file(&part_path)?;

    let mut successful_url = None;
    let mut resume = ResumeState::default();
    for attempt in 0..MAX_DOWNLOAD_ATTEMPTS {
        if (control.is_cancelled)() {
            return Err(DownloadError::Cancelled);
        }
        match download_attempt(&client, request, control, &mut partial_file, &mut resume) {
            Ok(final_url) => {
                successful_url = Some(final_url);
                break;
            }
            Err(error)
                if attempt + 1 < MAX_DOWNLOAD_ATTEMPTS && is_retryable_download_error(&error) =>
            {
                if resume.validator.is_none()
                    || matches!(
                        error,
                        DownloadError::ResumeObjectChanged | DownloadError::RangeNotSatisfiable
                    )
                {
                    reset_partial_file(&mut partial_file)?;
                    resume = ResumeState::default();
                }
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    let final_url = successful_url.expect("download attempts always return or record success");

    partial_file.sync_all()?;
    drop(partial_file);
    inspect_staged_file(private_root.path(), &part_path)?;
    fs::rename(&part_path, &staged_path)?;
    let identity = inspect_staged_file(private_root.path(), &staged_path)?;

    Ok(DownloadResult {
        private_root,
        staged_path,
        identity,
        final_url,
    })
}

pub fn save_verified_download_copy(
    download: &DownloadResult,
    destination_directory: &Path,
) -> Result<PathBuf, DownloadError> {
    let file_name = download
        .staged_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("verified installer file name is not valid Unicode"))?;
    validate_staged_file_name(file_name)?;
    verify_staged_identity(
        download.private_root.path(),
        &download.staged_path,
        &download.identity,
        Some(&download.identity.sha256),
    )?;

    fs::create_dir_all(destination_directory)?;
    if !fs::metadata(destination_directory)?.is_dir() {
        return Err(io::Error::other("system Downloads path is not a directory").into());
    }

    let short_digest = &download.identity.sha256[..12];
    let candidates = [
        destination_directory.join(file_name),
        destination_directory.join(digest_qualified_file_name(file_name, short_digest)),
        destination_directory.join(digest_qualified_file_name(
            file_name,
            &download.identity.sha256,
        )),
    ];
    let mut selected = None;
    for candidate in candidates {
        if visible_copy_matches(&candidate, &download.identity)? {
            return Ok(candidate);
        }
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                selected = Some(candidate);
                break;
            }
            Ok(_) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let target = selected.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "system Downloads already contains conflicting package names",
        )
    })?;

    let mut source = File::open(&download.staged_path)?;
    let mut visible = tempfile::Builder::new()
        .prefix(".easy-agent-")
        .suffix(".part")
        .tempfile_in(destination_directory)?;
    let mut digest = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        visible.write_all(&buffer[..read])?;
        digest.update(&buffer[..read]);
        copied = copied
            .checked_add(read as u64)
            .ok_or(DownloadError::VisibleCopyMismatch)?;
    }
    visible.as_file_mut().sync_all()?;
    let copied_sha256 = hex::encode(digest.finalize());
    if copied != download.identity.length
        || !copied_sha256.eq_ignore_ascii_case(&download.identity.sha256)
    {
        return Err(DownloadError::VisibleCopyMismatch);
    }
    verify_staged_identity(
        download.private_root.path(),
        &download.staged_path,
        &download.identity,
        Some(&download.identity.sha256),
    )?;

    match visible.persist_noclobber(&target) {
        Ok(_) => Ok(target),
        Err(error)
            if error.error.kind() == io::ErrorKind::AlreadyExists
                && visible_copy_matches(&target, &download.identity)? =>
        {
            Ok(target)
        }
        Err(error) => Err(error.error.into()),
    }
}

fn visible_copy_matches(path: &Path, expected: &StableFileIdentity) -> Result<bool, DownloadError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != expected.length
    {
        return Ok(false);
    }
    Ok(sha256_file(path)?.eq_ignore_ascii_case(&expected.sha256))
}

fn digest_qualified_file_name(file_name: &str, digest: &str) -> String {
    const SUFFIXES: &[&str] = &[".tar.gz", ".exe", ".msi", ".msix", ".dmg", ".zip"];
    let suffix = SUFFIXES
        .iter()
        .find(|suffix| file_name.ends_with(**suffix))
        .copied()
        .unwrap_or("");
    let stem = file_name.strip_suffix(suffix).unwrap_or(file_name);
    format!("{stem}-{digest}{suffix}")
}

fn download_attempt(
    client: &Client,
    request: &DownloadRequest<'_>,
    control: &DownloadControl<'_>,
    output: &mut File,
    resume: &mut ResumeState,
) -> Result<Url, DownloadError> {
    let mut requested_offset = output.metadata()?.len();
    if requested_offset > 0 && (resume.bound_url.is_none() || resume.validator.is_none()) {
        output.set_len(0)?;
        requested_offset = 0;
        *resume = ResumeState::default();
    }
    if requested_offset > MAX_INSTALLER_BYTES {
        return Err(DownloadError::TooLarge);
    }

    let start_url = resume.bound_url.as_ref().unwrap_or(&request.url);
    let (final_url, mut response) = send_with_redirects(
        client,
        start_url,
        request.url_rules,
        requested_offset,
        resume.validator.as_ref(),
    )?;
    let mut plan = plan_response(&response, requested_offset)?;
    if let Some(expected) = request.expected_size {
        if expected == 0 || expected > MAX_INSTALLER_BYTES {
            return Err(DownloadError::TooLarge);
        }
        if let Some(declared) = plan.expected_total
            && declared != expected
        {
            return Err(DownloadError::ExpectedSizeMismatch { expected, declared });
        }
        plan.expected_total = Some(expected);
    }
    let response_validator = resume_validator(&response);

    if plan.append {
        if resume.bound_url.as_ref() != Some(&final_url)
            || resume
                .expected_total
                .is_some_and(|expected| plan.expected_total != Some(expected))
            || resume.validator.as_ref() != response_validator.as_ref()
        {
            return Err(DownloadError::ResumeObjectChanged);
        }
        if resume.expected_total.is_none() {
            resume.expected_total = plan.expected_total;
        }
    } else {
        resume.bound_url = Some(final_url.clone());
        resume.validator = response_validator;
        resume.expected_total = plan.expected_total;
    }

    if plan.append {
        if output.metadata()?.len() != plan.initial_length {
            return Err(io::Error::other("partial download length changed unexpectedly").into());
        }
        output.seek(SeekFrom::Start(plan.initial_length))?;
    } else {
        output.set_len(0)?;
        output.seek(SeekFrom::Start(0))?;
    }

    let mut received = plan.initial_length;
    let mut body_received = 0_u64;
    (control.on_progress)(received, plan.expected_total);
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        if (control.is_cancelled)() {
            return Err(DownloadError::Cancelled);
        }
        let read = response
            .read(&mut buffer)
            .map_err(DownloadError::NetworkRead)?;
        if read == 0 {
            break;
        }
        body_received = body_received
            .checked_add(read as u64)
            .ok_or(DownloadError::TooLarge)?;
        if let Some(expected) = plan.expected_body_length
            && body_received > expected
        {
            return Err(DownloadError::LengthMismatch {
                expected,
                actual: body_received,
            });
        }
        received = received
            .checked_add(read as u64)
            .ok_or(DownloadError::TooLarge)?;
        if received > MAX_INSTALLER_BYTES {
            return Err(DownloadError::TooLarge);
        }
        if let Some(expected) = plan.expected_total
            && received > expected
        {
            return Err(DownloadError::LengthMismatch {
                expected,
                actual: received,
            });
        }
        output.write_all(&buffer[..read])?;
        (control.on_progress)(received, plan.expected_total);
    }
    if let Some(expected) = plan.expected_body_length
        && body_received != expected
    {
        return Err(DownloadError::LengthMismatch {
            expected,
            actual: body_received,
        });
    }
    if let Some(expected) = plan.expected_total
        && received != expected
    {
        return Err(DownloadError::LengthMismatch {
            expected,
            actual: received,
        });
    }
    output.sync_all()?;
    Ok(final_url)
}

fn send_with_redirects(
    client: &Client,
    start: &Url,
    url_rules: &[UrlRule],
    requested_offset: u64,
    validator: Option<&HeaderValue>,
) -> Result<(Url, Response), DownloadError> {
    let mut current = start.clone();
    for redirect_count in 0..=MAX_REDIRECTS {
        ensure_allowed_url_against_rules(&current, url_rules)?;
        let mut request = client
            .get(current.clone())
            .header(ACCEPT_ENCODING, "identity");
        if requested_offset > 0 {
            request = request.header(RANGE, format!("bytes={requested_offset}-"));
            if let Some(validator) = validator {
                request = request.header(IF_RANGE, validator.clone());
            }
        }
        let response = request.send()?;
        if response.status().is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err(DownloadError::RedirectLimit);
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(DownloadError::MissingRedirectLocation)?;
            current = current.join(location)?;
            continue;
        }
        ensure_allowed_url_against_rules(response.url(), url_rules)?;
        return Ok((response.url().clone(), response));
    }
    Err(DownloadError::RedirectLimit)
}

fn plan_response(
    response: &Response,
    requested_offset: u64,
) -> Result<ResponsePlan, DownloadError> {
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    match response.status() {
        StatusCode::OK => {
            if content_length.is_some_and(|length| length > MAX_INSTALLER_BYTES) {
                return Err(DownloadError::TooLarge);
            }
            Ok(ResponsePlan {
                append: false,
                initial_length: 0,
                expected_total: content_length,
                expected_body_length: content_length,
            })
        }
        StatusCode::PARTIAL_CONTENT => {
            let value = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .ok_or(DownloadError::InvalidContentRange)?;
            let (start, end, total) =
                parse_content_range(value).ok_or(DownloadError::InvalidContentRange)?;
            if start != requested_offset || total > MAX_INSTALLER_BYTES {
                return Err(if total > MAX_INSTALLER_BYTES {
                    DownloadError::TooLarge
                } else {
                    DownloadError::InvalidContentRange
                });
            }
            let segment_length = end - start + 1;
            if content_length.is_some_and(|length| length != segment_length) {
                return Err(DownloadError::InvalidContentRange);
            }
            Ok(ResponsePlan {
                append: requested_offset > 0,
                initial_length: requested_offset,
                expected_total: Some(total),
                expected_body_length: Some(segment_length),
            })
        }
        StatusCode::RANGE_NOT_SATISFIABLE => Err(DownloadError::RangeNotSatisfiable),
        status => Err(DownloadError::HttpStatus(status)),
    }
}

fn resume_validator(response: &Response) -> Option<HeaderValue> {
    response
        .headers()
        .get(ETAG)
        .filter(|value| !value.as_bytes().starts_with(b"W/"))
        .cloned()
}

fn open_new_partial_file(path: &Path) -> Result<File, DownloadError> {
    let mut options = fs::OpenOptions::new();
    options.create_new(true).read(true).write(true);

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("partial download path is not a regular file").into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other("partial download path is a reparse point").into());
        }
    }
    Ok(file)
}

fn reset_partial_file(file: &mut File) -> Result<(), DownloadError> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    let end = end.parse::<u64>().ok()?;
    let total = total.parse::<u64>().ok()?;
    (start <= end && end < total).then_some((start, end, total))
}

fn is_retryable_download_error(error: &DownloadError) -> bool {
    match error {
        DownloadError::Http(error) => error.is_timeout() || error.is_connect() || error.is_body(),
        DownloadError::NetworkRead(_) => true,
        DownloadError::LengthMismatch { expected, actual } => actual < expected,
        DownloadError::ResumeObjectChanged | DownloadError::RangeNotSatisfiable => true,
        DownloadError::HttpStatus(status) => matches!(
            *status,
            StatusCode::REQUEST_TIMEOUT
                | StatusCode::TOO_MANY_REQUESTS
                | StatusCode::INTERNAL_SERVER_ERROR
                | StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::GATEWAY_TIMEOUT
        ),
        _ => false,
    }
}

pub fn download_error_allows_verified_fallback(error: &DownloadError) -> bool {
    match error {
        DownloadError::Http(error) => {
            error.is_timeout()
                || error.is_body()
                || (error.is_connect() && !request_error_has_certificate_failure(error))
        }
        DownloadError::NetworkRead(_) => true,
        DownloadError::LengthMismatch { expected, actual } => actual < expected,
        DownloadError::HttpStatus(status) => {
            matches!(
                *status,
                StatusCode::FORBIDDEN
                    | StatusCode::REQUEST_TIMEOUT
                    | StatusCode::TOO_MANY_REQUESTS
                    | StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
            ) || status.is_server_error()
        }
        _ => false,
    }
}

fn request_error_has_certificate_failure(error: &reqwest::Error) -> bool {
    let mut messages = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(current) = source {
        messages.push_str(" | ");
        messages.push_str(&current.to_string());
        source = current.source();
    }
    looks_like_certificate_failure(&messages)
}

fn looks_like_certificate_failure(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    (message.contains("certificate")
        && [
            "verify",
            "verification",
            "invalid",
            "expired",
            "not valid",
            "hostname",
            "host name",
            "issuer",
            "self signed",
            "untrusted",
            "not trusted",
        ]
        .iter()
        .any(|marker| message.contains(marker)))
        || message.contains("cert_e_")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use url::Url;

    use super::{
        DownloadError, DownloadResult, download_error_allows_verified_fallback,
        inspect_staged_file, looks_like_certificate_failure, parse_content_range,
        save_verified_download_copy,
    };

    #[test]
    fn content_range_parser_accepts_only_bounded_byte_ranges() {
        assert_eq!(
            parse_content_range("bytes 128-255/1024"),
            Some((128, 255, 1024))
        );
        assert_eq!(parse_content_range("bytes 0-0/1"), Some((0, 0, 1)));
        for invalid in [
            "bytes */1024",
            "bytes 255-128/1024",
            "bytes 0-1024/1024",
            "items 0-1/2",
            "bytes 0-1/*",
        ] {
            assert_eq!(parse_content_range(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn verified_fallback_accepts_only_download_availability_failures() {
        for status in [
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::REQUEST_TIMEOUT,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
        ] {
            assert!(download_error_allows_verified_fallback(
                &DownloadError::HttpStatus(status)
            ));
        }
        assert!(download_error_allows_verified_fallback(
            &DownloadError::NetworkRead(std::io::Error::from(std::io::ErrorKind::ConnectionReset))
        ));
        assert!(download_error_allows_verified_fallback(
            &DownloadError::LengthMismatch {
                expected: 100,
                actual: 50,
            }
        ));
        assert!(!download_error_allows_verified_fallback(
            &DownloadError::HttpStatus(reqwest::StatusCode::NOT_FOUND)
        ));
        assert!(!download_error_allows_verified_fallback(
            &DownloadError::ExpectedSizeMismatch {
                expected: 100,
                declared: 101,
            }
        ));
        assert!(looks_like_certificate_failure(
            "certificate verify failed: unable to get local issuer certificate"
        ));
        assert!(!looks_like_certificate_failure(
            "TLS peer closed the connection without sending close_notify"
        ));
    }

    #[test]
    fn verified_download_copy_is_visible_and_never_overwrites_a_conflicting_file() {
        let private_root = tempfile::tempdir().unwrap();
        let staged_path = private_root.path().join("claude-1.2.3.msix");
        fs::write(&staged_path, b"verified package").unwrap();
        let identity = inspect_staged_file(private_root.path(), &staged_path).unwrap();
        let expected_short_digest = identity.sha256[..12].to_owned();
        let download = DownloadResult {
            private_root,
            staged_path,
            identity,
            final_url: Url::parse("https://downloads.claude.ai/Claude.msix").unwrap(),
        };
        let downloads = tempfile::tempdir().unwrap();

        let visible = save_verified_download_copy(&download, downloads.path()).unwrap();
        assert_eq!(
            visible.file_name().and_then(|name| name.to_str()),
            Some("claude-1.2.3.msix")
        );
        assert_eq!(fs::read(&visible).unwrap(), b"verified package");

        fs::write(&visible, b"user-owned conflicting file").unwrap();
        let collision_safe = save_verified_download_copy(&download, downloads.path()).unwrap();
        let expected_collision_name = format!("claude-1.2.3-{expected_short_digest}.msix");
        assert_eq!(
            collision_safe.file_name().and_then(|name| name.to_str()),
            Some(expected_collision_name.as_str())
        );
        assert_eq!(fs::read(&collision_safe).unwrap(), b"verified package");
        assert_eq!(
            save_verified_download_copy(&download, downloads.path()).unwrap(),
            collision_safe
        );
    }
}
