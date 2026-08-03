use std::collections::HashMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::json;

use super::{OperationState, OperationUpdate, ProductId};

const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_LOG_MESSAGE_CHARS: usize = 8192;
const LOG_FILE_NAME: &str = "operations.jsonl";
const PREVIOUS_LOG_FILE_NAME: &str = "operations.previous.jsonl";

pub struct OperationLog {
    inner: Mutex<OperationLogInner>,
}

struct OperationLogInner {
    file: File,
    last_states: HashMap<ProductId, OperationState>,
}

impl OperationLog {
    pub fn open_default() -> io::Result<Self> {
        let directory = default_log_directory()?;
        fs::create_dir_all(&directory)?;
        Self::open_at(directory.join(LOG_FILE_NAME))
    }

    fn open_at(path: PathBuf) -> io::Result<Self> {
        rotate_if_needed(&path)?;
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            inner: Mutex::new(OperationLogInner {
                file,
                last_states: HashMap::new(),
            }),
        })
    }

    pub fn record(&self, update: &OperationUpdate) -> io::Result<bool> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("operation log lock is poisoned"))?;
        let repeated_download = update.state == OperationState::Downloading
            && inner.last_states.get(&update.product) == Some(&OperationState::Downloading);
        inner.last_states.insert(update.product, update.state);
        if repeated_download {
            return Ok(false);
        }

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let message = redact_message(&update.message);
        let line = json!({
            "timestamp_unix_ms": timestamp_ms,
            "product": update.product.key(),
            "state": format!("{:?}", update.state),
            "message": message,
        });
        serde_json::to_writer(&mut inner.file, &line)?;
        inner.file.write_all(b"\n")?;
        inner.file.flush()?;
        Ok(true)
    }
}

fn default_log_directory() -> io::Result<PathBuf> {
    platform_log_directory()
}

#[cfg(windows)]
fn platform_log_directory() -> io::Result<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("easy agent").join("logs"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is unavailable"))
}

#[cfg(target_os = "macos")]
fn platform_log_directory() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join("Library").join("Logs").join("easy agent"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unavailable"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_log_directory() -> io::Result<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".local").join("state"))
        })
        .map(|path| path.join("easy-agent").join("logs"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "state directory is unavailable"))
}

fn rotate_if_needed(path: &Path) -> io::Result<()> {
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES)
    {
        let previous = path.with_file_name(PREVIOUS_LOG_FILE_NAME);
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        fs::rename(path, previous)?;
    }
    Ok(())
}

fn redact_message(message: &str) -> String {
    redact_message_with_roots(
        message,
        env::var("USERPROFILE").ok().as_deref(),
        env::var("TEMP").ok().as_deref(),
    )
}

fn redact_message_with_roots(
    message: &str,
    user_profile: Option<&str>,
    temp_directory: Option<&str>,
) -> String {
    static URL_PATTERN: OnceLock<Regex> = OnceLock::new();
    static AUTHORIZATION_PATTERN: OnceLock<Regex> = OnceLock::new();
    static AUTH_SCHEME_PATTERN: OnceLock<Regex> = OnceLock::new();
    static SECRET_ASSIGNMENT_PATTERN: OnceLock<Regex> = OnceLock::new();
    let url_pattern = URL_PATTERN.get_or_init(|| {
        Regex::new(r#"(?i)https?://[^\s\"'<>]+"#).expect("static operation-log URL regex")
    });
    let authorization_pattern = AUTHORIZATION_PATTERN.get_or_init(|| {
        Regex::new(r#"(?i)\bauthorization\s*[:=]\s*(?:bearer|basic)\s+[^\s,;]+"#)
            .expect("static operation-log authorization regex")
    });
    let auth_scheme_pattern = AUTH_SCHEME_PATTERN.get_or_init(|| {
        Regex::new(r#"(?i)\b(bearer|basic)\s+[^\s,;]+"#)
            .expect("static operation-log auth scheme regex")
    });
    let secret_assignment_pattern = SECRET_ASSIGNMENT_PATTERN.get_or_init(|| {
        Regex::new(
            r#"(?i)\b(api[_-]?key|access[_-]?token|token|secret|password|passwd)\b\s*[:=]\s*(?:\"[^\"]*\"|'[^']*'|[^\s,;]+)"#,
        )
        .expect("static operation-log secret assignment regex")
    });
    let mut redacted = message.replace(['\r', '\n'], " ");
    if let Some(temp_directory) = temp_directory.filter(|value| !value.is_empty()) {
        redacted = redacted.replace(temp_directory, "%TEMP%");
    }
    if let Some(user_profile) = user_profile.filter(|value| !value.is_empty()) {
        redacted = redacted.replace(user_profile, "%USERPROFILE%");
    }
    redacted = authorization_pattern
        .replace_all(&redacted, "Authorization=<redacted>")
        .into_owned();
    redacted = auth_scheme_pattern
        .replace_all(&redacted, "$1 <redacted>")
        .into_owned();
    redacted = secret_assignment_pattern
        .replace_all(&redacted, "$1=<redacted>")
        .into_owned();
    redacted = url_pattern
        .replace_all(&redacted, |captures: &regex::Captures<'_>| {
            url::Url::parse(&captures[0])
                .ok()
                .and_then(|url| {
                    url.host_str()
                        .map(|host| format!("{}://{host}/<redacted>", url.scheme()))
                })
                .unwrap_or_else(|| "<redacted-url>".into())
        })
        .into_owned();
    let normalized = redacted.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut bounded: String = normalized.chars().take(MAX_LOG_MESSAGE_CHARS).collect();
    if normalized.chars().count() > MAX_LOG_MESSAGE_CHARS {
        bounded.push('…');
    }
    bounded
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{OperationLog, redact_message_with_roots};
    use crate::core::{OperationState, OperationUpdate, ProductId};

    #[test]
    fn log_records_state_changes_but_not_every_download_progress_update() {
        let root = tempdir().unwrap();
        let path = root.path().join("operations.jsonl");
        let log = OperationLog::open_at(path.clone()).unwrap();
        let downloading = OperationUpdate {
            product: ProductId::WorkBuddy,
            state: OperationState::Downloading,
            message: "从官方来源下载".into(),
        };
        assert!(log.record(&downloading).unwrap());
        assert!(!log.record(&downloading).unwrap());
        assert!(
            log.record(&OperationUpdate {
                product: ProductId::WorkBuddy,
                state: OperationState::Failed,
                message: "下载失败".into(),
            })
            .unwrap()
        );
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 2);
    }

    #[test]
    fn log_redacts_urls_user_profile_and_temp_directory() {
        let redacted = redact_message_with_roots(
            "https://download.example/private/token.exe?key=secret C:\\Users\\admin C:\\Temp\\file.exe\nAuthorization: Bearer abc123 api_key=top-secret password='hidden value' next",
            Some(r"C:\Users\admin"),
            Some(r"C:\Temp"),
        );
        assert!(redacted.contains("https://download.example/<redacted>"));
        assert!(redacted.contains("%USERPROFILE%"));
        assert!(redacted.contains("%TEMP%"));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("hidden value"));
        assert!(redacted.contains("Authorization=<redacted>"));
        assert!(redacted.contains("api_key=<redacted>"));
        assert!(redacted.contains("password=<redacted>"));
        assert!(!redacted.contains('\n'));
    }

    #[test]
    fn log_messages_are_bounded_after_redaction() {
        let redacted = redact_message_with_roots(&"x".repeat(9000), None, None);
        assert_eq!(redacted.chars().count(), 8193);
        assert!(redacted.ends_with('…'));
    }
}
