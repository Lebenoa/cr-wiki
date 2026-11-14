//! Uploads images to the external image host (`[images] base_url` in
//! `Config.toml`) — the `discord-images` service that stores uploads as
//! Discord attachments and serves them back from its own domain.
//!
//! The contract is the host's `POST /api/upload-image`: raw image bytes as
//! the body, the image's type in `Content-Type`, an optional vanity name in
//! `X-Vanity`. Success answers JSON `{"status":"success","url":…,
//! "proxy_url":…}`; anything else is an error this module folds into
//! `UploadError` for the route layer to report.

use std::net::IpAddr;

use futures_util::StreamExt;
use serde::Deserialize;
use url::Url;

/// 20 MB — Discord's attachment ceiling. The upload is rejected here, before
/// the bytes leave the process, when larger. The image host
/// (`api::TEN_MB` in discord-images) must allow the same or larger.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

/// How many redirects a remote avatar URL may chain before the fetch gives
/// up. A pasted link is usually one hop; the cap stops a hostile chain from
/// pinning a worker.
pub const MAX_REDIRECTS: usize = 5;

/// Why a remote avatar URL could not be turned into an uploaded image. Every
/// variant maps to one translated message the settings page shows.
#[derive(Debug)]
pub enum FetchError {
    /// Not a parseable `http(s)://` URL with a host, or past the length cap.
    InvalidUrl,
    /// The host resolved to a loopback/private/link-local address — an
    /// internal service is not a valid avatar source (SSRF guard).
    ForbiddenHost,
    /// More than `MAX_REDIRECTS` hops without landing on a final response.
    TooManyRedirects,
    /// The URL answered non-2xx, or the connection failed or timed out.
    Unreachable(String),
    /// The response is bigger than the caller's byte cap.
    TooLarge,
    /// The final response's `Content-Type` is not an `image/*` type.
    NotImage,
    /// The bytes are not a raster image the catalog serves, or they do not
    /// match the declared type — remote headers are client input too.
    BadContent,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl => write!(f, "invalid URL"),
            Self::ForbiddenHost => write!(f, "URL resolves to a private or internal address"),
            Self::TooManyRedirects => write!(f, "too many redirects"),
            Self::Unreachable(e) => write!(f, "URL unreachable: {e}"),
            Self::TooLarge => write!(f, "image larger than the avatar cap"),
            Self::NotImage => write!(f, "URL does not serve an image/* content type"),
            Self::BadContent => write!(f, "bytes are not a supported image"),
        }
    }
}

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

/// Resolves an account avatar vanity to the URL templates render. Avatars
/// live only on the image host — there is no local fallback directory, so an
/// unconfigured host means no avatar image, exactly like an empty name.
/// Callers render the initial circle for `None`.
pub fn resolve_avatar(vanity: &str) -> Option<String> {
    let vanity = vanity.trim();
    if vanity.is_empty() {
        return None;
    }
    let state = crate::state::state();
    state
        .cfg
        .images
        .is_configured()
        .then(|| served_url(state.cfg.images.origin(), vanity))
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

/// SSRF guard: whether `ip` designates an address the server itself owns or
/// an internal network — unspecified, loopback, private space, link-local,
/// the CGNAT range, and the IPv4-mapped / NAT64 views of those. A visitor-
/// chosen URL resolving anywhere in here would reach the host's own services
/// (this app and the image host included) instead of an external server, so
/// an avatar URL is refused before anything is fetched.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private() // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local() // 169.254/16
                || {
                    let o = v4.octets();
                    o[0] == 100 && (64..=127).contains(&o[1]) // 100.64/10 CGNAT
                }
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_ip(IpAddr::V4(v4));
            }
            let seg = v6.segments();
            v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_unique_local() // fc00::/7
                || v6.is_unicast_link_local() // fe80::/10
                // 64:ff9b::/96: a NAT64 view of an IPv4 target, checked too
                || (seg[0] == 0x0064
                    && seg[1] == 0xff9b
                    && seg[2] == 0
                    && seg[3] == 0
                    && is_private_ip(IpAddr::V4(
                        ((u32::from(seg[6]) << 16) | u32::from(seg[7])).into(),
                    )))
        }
    }
}

/// Scheme, host and SSRF checks for one hop of a remote image fetch: http(s)
/// only (a `file:` or `data:` target has nothing a CDN would serve), a real
/// host, and every address the hostname resolves to must be public. Fails
/// closed — a resolution that mixes public and private addresses, or one
/// that returns nothing at all, is refused.
async fn validate_remote_url(url: &Url) -> Result<(), FetchError> {
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(FetchError::InvalidUrl);
    }
    let Some(host) = url.host_str() else {
        return Err(FetchError::InvalidUrl);
    };
    let port = url.port_or_known_default().unwrap_or(80);
    // one DNS resolution per hop; reqwest resolves again when connecting, so
    // a rebinding host would have to change within milliseconds
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| FetchError::Unreachable(e.to_string()))?;
    let mut any = false;
    for addr in addrs {
        any = true;
        if is_private_ip(addr.ip()) {
            return Err(FetchError::ForbiddenHost);
        }
    }
    if !any {
        return Err(FetchError::Unreachable("host did not resolve".into()));
    }
    Ok(())
}

/// Fetches a remote image so it can be re-hosted as an avatar. The URL is
/// validated before anything leaves the process (`InvalidUrl`,
/// `ForbiddenHost`), every redirect hop is validated again, and the eventual
/// 2xx response must carry an `image/*` `Content-Type` whose bytes sniff as
/// a real raster the catalog serves — remote headers are as trustworthy as a
/// client's, so magic bytes decide. The body is cut off at `max_bytes + 1`
/// so an oversized (or chunked, unbounded) response cannot buffer whole.
/// Returns the declared content type and the capped bytes, ready for
/// `upload`.
pub async fn fetch_image(raw: &str, max_bytes: usize) -> Result<(String, Vec<u8>), FetchError> {
    if raw.trim().chars().count() > 2048 {
        return Err(FetchError::InvalidUrl);
    }
    let url = Url::parse(raw.trim()).map_err(|_| FetchError::InvalidUrl)?;
    validate_remote_url(&url).await?;
    fetch_redirect_chain(&fetch_client()?, url, max_bytes).await
}

/// The one-shot client remote avatar fetches share: no redirects (each hop
/// is validated by hand before it is followed) and a hard timeout so a
/// hostile URL cannot pin a worker.
fn fetch_client() -> Result<reqwest::Client, FetchError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| FetchError::Unreachable(e.to_string()))
}

/// The redirect loop and final-response validation for an already-guarded
/// URL. Split from `fetch_image` so the response gates — the `image/*`
/// content-type requirement, the byte cap and the magic-byte sniff — are
/// testable against a local server without weakening the first-hop SSRF
/// guard (whose per-hop re-run this loop still applies to every redirect).
async fn fetch_redirect_chain(
    client: &reqwest::Client,
    mut url: Url,
    max_bytes: usize,
) -> Result<(String, Vec<u8>), FetchError> {
    let mut hops = 0;
    loop {
        let response = client
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, "cookierun-avatar-fetcher")
            .send()
            .await
            .map_err(|e| FetchError::Unreachable(e.to_string()))?;
        if response.status().is_redirection() {
            hops += 1;
            if hops > MAX_REDIRECTS {
                return Err(FetchError::TooManyRedirects);
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or(FetchError::InvalidUrl)?;
            url = url.join(location).map_err(|_| FetchError::InvalidUrl)?;
            validate_remote_url(&url).await?;
            continue;
        }
        if !response.status().is_success() {
            return Err(FetchError::Unreachable(format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }
        let media_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        // the gate this fetch exists for: the URL must point at an image,
        // judged by the media type it actually answers with
        if !media_type.starts_with("image/") {
            return Err(FetchError::NotImage);
        }
        if let Some(len) = response.content_length() {
            if len > max_bytes as u64 {
                return Err(FetchError::TooLarge);
            }
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| FetchError::Unreachable(e.to_string()))?;
            // the cap is enforced on every chunk, so a chunked body that lies
            // about its length (or never declares one) cannot buffer whole
            if bytes.len().saturating_add(chunk.len()) > max_bytes {
                return Err(FetchError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let Some(sniffed) = crate::upload::sniff_image(&bytes) else {
            return Err(FetchError::BadContent);
        };
        if crate::upload::extension_for(&media_type) != Some(sniffed) {
            return Err(FetchError::BadContent);
        }
        return Ok((media_type, bytes));
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

    #[test]
    fn private_ip_guard_covers_the_internal_ranges() {
        for ip in [
            "0.0.0.0",
            "10.0.0.1",
            "10.255.255.255",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "::1",
            "fc00::1",
            "fe80::1",
            "::",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
        ] {
            let ip: IpAddr = ip.parse().expect("test address");
            assert!(is_private_ip(ip), "{ip} must be refused");
        }
    }

    #[test]
    fn private_ip_guard_lets_public_addresses_through() {
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
        ] {
            let ip: IpAddr = ip.parse().expect("test address");
            assert!(!is_private_ip(ip), "{ip} must be allowed");
        }
    }

    #[test]
    fn mapped_ipv6_keeps_the_embedded_ipv4_check() {
        assert!(
            is_private_ip("::ffff:192.168.0.5".parse().expect("mapped address")),
            "an IPv4-mapped private address must stay private"
        );
    }

    /// A one-route local HTTP server answering a canned response, so the
    /// fetch gates run without a network. Returns the base URL and the
    /// server task; the caller aborts the task and drops the handle.
    async fn serve(
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral listener");
        let addr = listener.local_addr().expect("bound address");
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(move || {
                let body = body.clone();
                async move {
                    axum::response::Response::builder()
                        .status(axum::http::StatusCode::from_u16(status).expect("status code"))
                        .header(axum::http::header::CONTENT_TYPE, content_type)
                        .body(axum::body::Body::from(body))
                        .expect("response body")
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (format!("http://{addr}/"), server)
    }

    /// The eight PNG magic bytes; enough for `sniff_image`, which the gate
    /// checks before decode.
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

    #[tokio::test]
    async fn fetch_accepts_a_real_png() {
        let (base, server) = serve(200, "image/png", PNG_MAGIC.to_vec()).await;
        let (content_type, bytes) = fetch_redirect_chain(
            &fetch_client().expect("client"),
            Url::parse(&base).expect("url"),
            1024,
        )
        .await
        .expect("fetched");
        server.abort();
        assert_eq!(content_type, "image/png");
        assert_eq!(bytes, PNG_MAGIC);
    }

    #[tokio::test]
    async fn fetch_rejects_a_url_that_is_not_an_image() {
        let (base, server) = serve(200, "text/html", b"<h1>not an image</h1>".to_vec()).await;
        let err = fetch_redirect_chain(
            &fetch_client().expect("client"),
            Url::parse(&base).expect("url"),
            1024,
        )
        .await
        .expect_err("content-type gate");
        server.abort();
        assert!(matches!(err, FetchError::NotImage));
    }

    #[tokio::test]
    async fn fetch_rejects_bytes_that_lie_about_their_type() {
        let (base, server) = serve(200, "image/png", b"definitely not a png".to_vec()).await;
        let err = fetch_redirect_chain(
            &fetch_client().expect("client"),
            Url::parse(&base).expect("url"),
            1024,
        )
        .await
        .expect_err("sniff gate");
        server.abort();
        assert!(matches!(err, FetchError::BadContent));
    }

    #[tokio::test]
    async fn fetch_rejects_an_oversized_response_before_reading_it() {
        let (base, server) = serve(200, "image/png", vec![0u8; 2049]).await;
        let err = fetch_redirect_chain(
            &fetch_client().expect("client"),
            Url::parse(&base).expect("url"),
            1024,
        )
        .await
        .expect_err("size gate");
        server.abort();
        assert!(matches!(err, FetchError::TooLarge));
    }

    #[tokio::test]
    async fn guard_refuses_internal_hosts_and_non_http_schemes() {
        for url in [
            "http://localhost/",
            "http://127.0.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.5/img.png",
            "file:///etc/passwd",
        ] {
            let parsed = Url::parse(url).expect("parseable");
            let err = validate_remote_url(&parsed).await.expect_err("guarded");
            assert!(
                matches!(err, FetchError::ForbiddenHost | FetchError::InvalidUrl),
                "{url} must be refused, got {err:?}"
            );
        }
    }
}
