use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

use super::{TrustEntry, UrlRule};

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("URL must use HTTPS")]
    NonHttps,
    #[error("URL has no host")]
    MissingHost,
    #[error("host is not allowlisted: {0}")]
    HostNotAllowed(String),
    #[error("URL path is outside the adapter contract: {0}")]
    PathNotAllowed(String),
    #[error("staged path is not a regular file: {0}")]
    NotRegularFile(PathBuf),
    #[error("staged path contains a reparse point or symlink: {0}")]
    ReparsePoint(PathBuf),
    #[error("staged file escaped its private root")]
    EscapedRoot,
    #[error("staged file changed after verification")]
    IdentityChanged,
    #[error("SHA-256 mismatch")]
    DigestMismatch,
    #[error("staged file name is unsafe: {0}")]
    UnsafeFileName(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn validate_staged_file_name(file_name: &str) -> Result<(), SecurityError> {
    if file_name.is_empty()
        || file_name == "."
        || file_name == ".."
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains(':')
        || file_name.contains('\0')
    {
        return Err(SecurityError::UnsafeFileName(file_name.into()));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableFileIdentity {
    pub canonical_path: PathBuf,
    pub length: u64,
    pub sha256: String,
}

pub fn ensure_allowed_url(url: &Url, trust: &TrustEntry) -> Result<(), SecurityError> {
    ensure_allowed_url_against_rules(url, &trust.url_rules)
}

pub fn ensure_allowed_url_against_rules(url: &Url, rules: &[UrlRule]) -> Result<(), SecurityError> {
    if url.scheme() != "https" {
        return Err(SecurityError::NonHttps);
    }
    let host = url.host_str().ok_or(SecurityError::MissingHost)?;
    let Some(rule) = rules
        .iter()
        .find(|rule| host.eq_ignore_ascii_case(&rule.host))
    else {
        return Err(SecurityError::HostNotAllowed(host.into()));
    };
    let path_allowed = rule.exact_paths.iter().any(|path| url.path() == path)
        || rule
            .path_prefixes
            .iter()
            .any(|prefix| url.path().starts_with(prefix));
    if !path_allowed {
        return Err(SecurityError::PathNotAllowed(url.path().into()));
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, SecurityError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

pub fn inspect_staged_file(root: &Path, path: &Path) -> Result<StableFileIdentity, SecurityError> {
    reject_links_within_root(root, path)?;

    let canonical_root = fs::canonicalize(root)?;
    let canonical_path = fs::canonicalize(path)?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(SecurityError::EscapedRoot);
    }
    let metadata = fs::metadata(&canonical_path)?;
    if !metadata.is_file() {
        return Err(SecurityError::NotRegularFile(canonical_path));
    }

    Ok(StableFileIdentity {
        canonical_path: canonical_path.clone(),
        length: metadata.len(),
        sha256: sha256_file(&canonical_path)?,
    })
}

pub fn verify_staged_identity(
    root: &Path,
    path: &Path,
    previously_verified: &StableFileIdentity,
    expected_sha256: Option<&str>,
) -> Result<StableFileIdentity, SecurityError> {
    let current = inspect_staged_file(root, path)?;
    if &current != previously_verified {
        return Err(SecurityError::IdentityChanged);
    }
    if let Some(expected) = expected_sha256
        && !current.sha256.eq_ignore_ascii_case(expected)
    {
        return Err(SecurityError::DigestMismatch);
    }
    Ok(current)
}

fn reject_links_within_root(root: &Path, path: &Path) -> Result<(), SecurityError> {
    reject_link(root)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| SecurityError::EscapedRoot)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(name) => current.push(name),
            Component::CurDir => continue,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SecurityError::EscapedRoot);
            }
        }
        reject_link(&current)?;
    }
    Ok(())
}

fn reject_link(path: &Path) -> Result<(), SecurityError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(SecurityError::ReparsePoint(path.to_path_buf()))
        }
        Ok(_metadata) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
                if _metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(SecurityError::ReparsePoint(path.to_path_buf()));
                }
            }
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}
