use std::io::Read;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{LOCATION, RANGE};
use thiserror::Error;
use url::Url;

use super::{SecurityError, TrustEntry, ensure_allowed_url};

const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

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
}

pub fn safe_http_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("ai-client-installer/", env!("CARGO_PKG_VERSION")))
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
    let mut current = start.clone();
    for redirect_count in 0..=MAX_REDIRECTS {
        ensure_allowed_url(&current, trust)?;
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
    let mut current = start.clone();
    for redirect_count in 0..=MAX_REDIRECTS {
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
