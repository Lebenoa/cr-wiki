//! Uploads images to the external image host (`[images] base_url` in
//! `Config.toml`) — the `discord-images` service that stores uploads as
//! Discord attachments and serves them back from its own domain.
//!
//! The contract is the host's `POST /api/upload-image`: raw image bytes as
//! the body, the image's type in `Content-Type`, an optional vanity name in
//! `X-Vanity`. Success answers JSON `{"status":"success","url":…,
//! "proxy_url":…}`; anything else is an error this module folds into
//! `UploadError` for the route layer to report.

use serde::Deserialize;

/// 20 MB — Discord's attachment ceiling. The upload is rejected here, before
/// the bytes leave the process, when larger. The image host
/// (`api::TEN_MB` in discord-images) must allow the same or larger.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug)]
pub enum UploadError {
    /// The feature is off: `[images] base_url` is not configured.
    NotConfigured,
    /// The bytes are larger than the host accepts.
    TooLarge,
    /// The host refused the upload (bad type, taken vanity, …) — carries the
    /// status and the host's own message when one came back.
    Rejected(u16, String),
    /// The host could not be reached or answered nonsense.
    Unavailable(String),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "image host not configured"),
            Self::TooLarge => write!(f, "image larger than the host accepts"),
            Self::Rejected(status, message) => {
                write!(f, "image host rejected the upload ({status}): {message}")
            }
            Self::Unavailable(e) => write!(f, "image host unavailable: {e}"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    status: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    proxy_url: String,
}

/// The served URL of one upload: the host's `/proxy/…` route, which streams
/// the stored bytes with their original content type. Preferred over `url`
/// (the Discord CDN link) because it answers from the image host's own
/// domain — Discord rewrites attachment URLs when it garbage-collects, so a
/// stored CDN link can rot while the proxy keeps working.
pub fn served_url(base_url: &str, name: &str) -> String {
    format!("{}/proxy/{name}", base_url.trim_end_matches('/'))
}

/// Resolves a stored bare filename to the URL a page should use: the image
/// host's `/proxy/` route when `[images]` is configured (uploads live there),
/// else the local `/static/img/news/` directory the pre-host thumbnails were
/// written to. None for an empty name — callers hide the image rather than
/// guess another locale's artwork.
pub fn resolve(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let state = crate::state::state();
    if state.cfg.images.is_configured() {
        Some(served_url(state.cfg.images.origin(), name))
    } else {
        Some(format!("/static/img/news/{name}"))
    }
}

/// Uploads `bytes` to the image host under `vanity`. The vanity must already
/// be a safe name — the caller sanitizes (the news thumbnail sanitizer); this
/// function only transmits.
pub async fn upload(vanity: &str, content_type: &str, bytes: &[u8]) -> Result<String, UploadError> {
    let base = crate::state::state().cfg.images.origin().to_string();
    if base.is_empty() {
        return Err(UploadError::NotConfigured);
    }
    if bytes.len() > MAX_BYTES {
        return Err(UploadError::TooLarge);
    }

    // one client per call, the oauth.rs pattern: at most a couple of uploads
    // per admin edit, so there is nothing worth pooling
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| UploadError::Unavailable(e.to_string()))?;

    let response = client
        .post(format!("{base}/api/upload-image"))
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .header("X-Vanity", vanity)
        .body(bytes.to_vec())
        .send()
        .await
        .map_err(|e| UploadError::Unavailable(e.to_string()))?;

    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let parsed: Option<UploadResponse> = serde_json::from_str(&text).ok();
    match parsed {
        Some(r) if r.status == "success" && !r.proxy_url.is_empty() => Ok(r.proxy_url),
        Some(r) => Err(UploadError::Rejected(status, r.message)),
        None => Err(UploadError::Rejected(
            status,
            text.chars().take(200).collect(),
        )),
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    #[test]
    fn served_url_joins_the_proxy_path() {
        assert_eq!(
            served_url("https://image.lebenoa.com", "banner.png"),
            "https://image.lebenoa.com/proxy/banner.png"
        );
        assert_eq!(
            served_url("https://image.lebenoa.com/", "x.jpg"),
            "https://image.lebenoa.com/proxy/x.jpg",
            "a trailing slash in config must not double up"
        );
    }
}
