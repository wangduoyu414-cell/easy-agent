mod cc_switch;
mod chatgpt;
mod claude;
mod hermes;
mod resolver;
mod workbuddy;

pub use cc_switch::parse_cc_switch_manifest;
pub use chatgpt::{candidate_from_verified_chatgpt_mirror, parse_chatgpt_macos_appcast};
pub use claude::{candidate_from_claude_redirect, candidate_from_verified_claude_mirror};
pub use hermes::parse_hermes_homepage;
pub use resolver::{
    ResolveError, resolve_install_plan, resolve_latest, resolve_verified_download_fallback,
};
pub use workbuddy::parse_workbuddy_update;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("official response format changed: {0}")]
    Contract(String),
    #[error("no matching artifact for this platform")]
    NoMatchingArtifact,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
}
