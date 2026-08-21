use std::io::{ErrorKind, Read};
use std::thread;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{LOCATION, RANGE};
use thiserror::Error;
use url::Url;

use super::{
    SecurityError, TrustEntry, UrlRule, ensure_allowed_url, ensure_allowed_url_against_rules,
};

const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const MAX_METADATA_ATTEMPTS: usize = 3;
const METADATA_RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Error)]
pub enum HttpError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Security(#[from] SecurityError),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("redirect response has no valid Location header")]
    MissingRedirectLocation,
    #[error("redirect limit exceeded")]
    RedirectLimit,
    #[error("metadata response is larger than 4 MiB")]
    MetadataTooLarge,
    #[error("server returned HTTP {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("metadata response is not UTF-8")]
    InvalidUtf8,
    #[error("official service reports that this region is unavailable")]
    RegionRestricted,
}

pub fn safe_http_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("easy-agent/", env!("CARGO_PKG_VERSION")))
        .build()
}

pub fn safe_artifact_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent(concat!("easy-agent/", env!("CARGO_PKG_VERSION")))
        .build()
}

pub fn fetch_official_text(
    client: &Client,
    start: &Url,
    trust: &TrustEntry,
) -> Result<(Url, String), HttpError> {
    let (url, bytes) = fetch_official_bytes(client, start, trust)?;
    let text = String::from_utf8(bytes).map_err(|_| HttpError::InvalidUtf8)?;
    Ok((url, text))
}

pub fn fetch_official_bytes(
    client: &Client,
    start: &Url,
    trust: &TrustEntry,
) -> Result<(Url, Vec<u8>), HttpError> {
    fetch_allowed_bytes(client, start, &trust.url_rules)
}

pub fn fetch_allowed_bytes(
    client: &Client,
    start: &Url,
    rules: &[UrlRule],
) -> Result<(Url, Vec<u8>), HttpError> {
    retry_metadata_operation(|| fetch_allowed_bytes_once(client, start, rules))
}

fn fetch_allowed_bytes_once(
    client: &Client,
    start: &Url,
    rules: &[UrlRule],
) -> Result<(Url, Vec<u8>), HttpError> {
    let mut current = start.clone();
    for redirect_count in 0..=MAX_REDIRECTS {
        ensure_allowed_url_against_rules(&current, rules)?;
        let response = client.get(current.clone()).send()?;
        if response.status().is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err(HttpError::RedirectLimit);
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(HttpError::MissingRedirectLocation)?;
            current = current.join(location)?;
            continue;
        }
        if !response.status().is_success() {
            return Err(HttpError::HttpStatus(response.status()));
        }
        ensure_allowed_url_against_rules(response.url(), rules)?;
        let mut bytes = Vec::new();
        response
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_METADATA_BYTES {
            return Err(HttpError::MetadataTooLarge);
        }
        return Ok((current, bytes));
    }
    Err(HttpError::RedirectLimit)
}

pub fn resolve_official_url(
    client: &Client,
    start: &Url,
    trust: &TrustEntry,
) -> Result<Url, HttpError> {
    retry_metadata_operation(|| resolve_official_url_once(client, start, trust))
}

fn resolve_official_url_once(
    client: &Client,
    start: &Url,
    trust: &TrustEntry,
) -> Result<Url, HttpError> {
    let mut current = start.clone();
    for redirect_count in 0..=MAX_REDIRECTS {
        if is_exact_claude_region_restriction(&current) {
            return Err(HttpError::RegionRestricted);
        }
        ensure_allowed_url(&current, trust)?;
        let response = client
            .get(current.clone())
            .header(RANGE, "bytes=0-0")
            .send()?;
        if response.status().is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err(HttpError::RedirectLimit);
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(HttpError::MissingRedirectLocation)?;
            current = current.join(location)?;
            continue;
        }
        if !response.status().is_success() {
            return Err(HttpError::HttpStatus(response.status()));
        }
        ensure_allowed_url(response.url(), trust)?;
        return Ok(response.url().clone());
    }
    Err(HttpError::RedirectLimit)
}

fn retry_metadata_operation<T>(
    mut operation: impl FnMut() -> Result<T, HttpError>,
) -> Result<T, HttpError> {
    for attempt in 0..MAX_METADATA_ATTEMPTS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error)
                if attempt + 1 < MAX_METADATA_ATTEMPTS && metadata_error_is_retryable(&error) =>
            {
                thread::sleep(METADATA_RETRY_DELAY.saturating_mul((attempt + 1) as u32));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("metadata retry loop always returns")
}

fn metadata_error_is_retryable(error: &HttpError) -> bool {
    match error {
        HttpError::Request(error) => {
            (error.is_connect() || error.is_timeout() || error.is_body())
                && !request_error_has_certificate_failure(error)
        }
        HttpError::Io(error) => matches!(
            error.kind(),
            ErrorKind::BrokenPipe
                | ErrorKind::ConnectionAborted
                | ErrorKind::ConnectionReset
                | ErrorKind::Interrupted
                | ErrorKind::TimedOut
                | ErrorKind::UnexpectedEof
        ),
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
    let messages = messages.to_ascii_lowercase();
    (messages.contains("certificate")
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
        .any(|marker| messages.contains(marker)))
        || messages.contains("cert_e_")
}

fn is_exact_claude_region_restriction(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("claude.ai") || host.eq_ignore_ascii_case("www.anthropic.com")
        })
        && url.path() == "/app-unavailable-in-region"
        && url.query().is_none()
        && url.fragment().is_none()
}

#[cfg(test)]
mod tests {
    use std::io;

    use url::Url;

    use super::{HttpError, is_exact_claude_region_restriction, metadata_error_is_retryable};

    #[test]
    fn recognizes_only_the_exact_claude_region_block_pages() {
        for allowed in [
            "https://claude.ai/app-unavailable-in-region",
            "https://www.anthropic.com/app-unavailable-in-region",
        ] {
            assert!(is_exact_claude_region_restriction(
                &Url::parse(allowed).unwrap()
            ));
        }
        for rejected in [
            "http://www.anthropic.com/app-unavailable-in-region",
            "https://anthropic.com/app-unavailable-in-region",
            "https://www.anthropic.com/app-unavailable-in-region?next=payload",
            "https://www.anthropic.com/other",
        ] {
            assert!(!is_exact_claude_region_restriction(
                &Url::parse(rejected).unwrap()
            ));
        }
    }

    #[test]
    fn retries_only_transient_metadata_io_failures() {
        for kind in [
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::TimedOut,
            io::ErrorKind::UnexpectedEof,
        ] {
            assert!(metadata_error_is_retryable(&HttpError::Io(
                io::Error::from(kind)
            )));
        }
        assert!(!metadata_error_is_retryable(&HttpError::Io(
            io::Error::from(io::ErrorKind::PermissionDenied,)
        )));
        assert!(!metadata_error_is_retryable(&HttpError::HttpStatus(
            reqwest::StatusCode::FORBIDDEN,
        )));
    }
}
