//! Per-request context: the locale, the path, and the URL/header helpers the
//! templates ask for. Mirrors the veb `Context` struct and `before_request`.
//!
//! Ctx is the request-facts half of the old god-struct: what the visitor sent
//! and who they are (lang, path, user, htmx, `is_admin`), plus the header
//! readers the middleware and the routes share. The rest of the surface sits
//! beside this file as further `impl Ctx` blocks, so askama still finds every
//! name on the context:
//!
//! - `lang.rs` — the locale, the `?lang=`/cookie resolution, the URLs built
//!   from it and the per-language font sheet
//! - `nav.rs` — the navbar, theme and EP lists, delegated to `crate::chrome`
//! - `meta.rs` — titles, descriptions and the Open Graph image facts
//! - `labels.rs` — the grade, episode and score labels and badge classes
//!
//! The chrome itself knows nothing about requests; these methods delegate so
//! askama still finds the names on the context. The doc-comment path
//! references below are crate paths, hence the backticks.

use axum::extract::ConnectInfo;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use std::convert::Infallible;
use std::net::SocketAddr;

use crate::i18n::{Loc, DEFAULT_LANG};
use crate::section::Section;
use crate::session::SessionUser;

mod labels;
mod lang;
mod meta;
mod nav;

pub use self::lang::{resolve_lang, LangChoice, LANG_COOKIE};
// and Google-configured in any combination — and each is read by different
// parts of the templates.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct Ctx {
    pub lang: String,
    pub l: Loc,
    /// request path with the query stripped; the navbar asks for it many
    /// times per page, so it is resolved once
    pub path: String,
    pub site_url: String,
    /// The site's own timezone, in minutes east of UTC: the news surface reads
    /// it to show a publication date that is a moment as the civil day it fell
    /// on *here*, rather than the UTC day it happened to start on.
    pub tz_offset: i64,
    pub htmx: bool,
    pub boosted: bool,
    // read by the debug-build admin bypass only; a release binary never
    // touches it
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub is_local: bool,
    pub user: Option<SessionUser>,
    /// whether `[google]` carries a usable credential: the auth templates ask
    /// before offering a button that would otherwise lead to a 404
    pub google_login: bool,
}

impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = Infallible;

    // axum 0.8's FromRequestParts is already async-fn-in-trait; no await here
    // because the context only reads already-extracted parts
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let lang = parts
            .extensions
            .get::<LangChoice>()
            .map_or_else(|| DEFAULT_LANG.to_string(), |c| c.lang.clone());
        let user = parts.extensions.get::<SessionUser>().cloned();
        let h = &parts.headers;
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0);
        Ok(Self {
            user,
            l: Loc::new(&lang),
            lang,
            path: parts.uri.path().to_string(),
            site_url: site_url(h),
            tz_offset: crate::state::site_tz_offset(),
            htmx: header(h, "HX-Request").is_some(),
            boosted: header(h, "HX-Boosted").is_some(),
            is_local: is_local(h, peer),
            google_login: crate::state::google_login_enabled(),
        })
    }
}

impl Ctx {
    /// A context with no request behind it, for the panic handler: by the
    /// time it runs the extractors are gone, and the 500 page still has a
    /// navbar to render.
    pub fn minimal() -> Self {
        Self {
            lang: crate::i18n::DEFAULT_LANG.to_string(),
            l: crate::i18n::Loc::new(crate::i18n::DEFAULT_LANG),
            path: "/".to_string(),
            site_url: String::new(),
            tz_offset: crate::state::site_tz_offset(),
            htmx: false,
            boosted: false,
            is_local: false,
            user: None,
            // a page rendered without a request has no config to consult, and
            // the failure page must never gain a sign-in control
            google_login: false,
        }
    }

    /// The site's own timezone in minutes east of UTC, for the templates that
    /// stamp a stored instant as a civil day. A method rather than the field:
    /// askama hands a *field* access to a call as a reference and a method's
    /// result by value, and the news date helpers take the value.
    pub const fn tz(&self) -> i64 {
        self.tz_offset
    }

    /// A fragment swap, as opposed to an hx-boosted navigation which still
    /// wants the whole page.
    pub const fn is_fragment(&self) -> bool {
        self.htmx && !self.boosted
    }

    /// Admin permission: an admin session, or a loopback request — which is
    /// what makes local development admin without a login, as in `app.v`.
    pub fn is_admin(&self) -> bool {
        if self.user.as_ref().is_some_and(|u| u.is_admin) {
            return true;
        }
        // The loopback bypass is a dev-build affordance only: a release
        // binary behind a proxy that forgets to forward client headers must
        // not mistake headerless remote requests for local ones (the V app
        // gated this behind $if !prod). `#[cfg]`, not `cfg!`, so a release
        // build never even compiles the bypass.
        #[cfg(debug_assertions)]
        if self.is_local {
            return true;
        }
        false
    }

    pub fn username(&self) -> String {
        self.user
            .as_ref()
            .map(|u| u.username.clone())
            .unwrap_or_default()
    }

    /// Whether the admin editor has a form for this section — the catalog
    /// page's new-entity button links nowhere else.
    #[allow(clippy::unused_self)]
    pub fn section_editable(&self, section: &str) -> bool {
        Section::parse(section).is_some_and(crate::section::Section::editable)
    }

    /// The public Turnstile site key, for the widget divs. A method because
    /// askama calls it on the context.
    #[allow(clippy::unused_self)]
    pub const fn turnstile_sitekey(&self) -> &'static str {
        crate::turnstile::SITEKEY
    }

    pub const fn signed_in(&self) -> bool {
        self.user.is_some()
    }
}

fn header<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

fn site_url(h: &HeaderMap) -> String {
    if let Ok(base) = std::env::var("CR_BASE_URL") {
        if !base.is_empty() {
            return base.trim_end_matches('/').to_string();
        }
    }
    let scheme = header(h, "X-Forwarded-Proto").unwrap_or("http");
    header(h, "host").map_or_else(
        || "http://localhost:6785".to_string(),
        |host| format!("{scheme}://{host}"),
    )
}

/// Loopback only when no proxy header is present: any of them means the peer
/// address is the proxy's, not the visitor's. The header half alone is NOT
/// enough — an internet request carries none of these headers by default, so
/// without the peer test every remote request would look local and inherit
/// admin. A request with no peer address at all fails closed.
fn is_local(h: &HeaderMap, peer: Option<SocketAddr>) -> bool {
    if header(h, "CF-Connecting-IP").is_some()
        || header(h, "X-Forwarded-For").is_some()
        || header(h, "X-Real-Ip").is_some()
    {
        return false;
    }
    peer.is_some_and(|a| a.ip().is_loopback())
}

/// The requester's IP for rate-limit keying. The TCP peer is the only source
/// a client cannot forge, so it is the default; forwarded headers stand in
/// only when the peer itself is in the configured trusted-proxy list — behind
/// such a proxy every visitor shares one peer, so the header carries the real
/// per-client address. Requests with no address share the 'unknown' bucket.
pub fn client_ip(
    h: &HeaderMap,
    peer: Option<std::net::SocketAddr>,
    trusted_proxies: &[String],
) -> String {
    let peer_str = peer.map_or_else(|| "unknown".to_string(), |a| a.ip().to_string());
    if trusted_proxies.iter().any(|p| p == &peer_str) {
        for name in ["CF-Connecting-IP", "X-Real-Ip"] {
            if let Some(v) = header(h, name) {
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
        if let Some(v) = header(h, "X-Forwarded-For") {
            if let Some(first) = v.split(',').next() {
                let first = first.trim();
                if !first.is_empty() {
                    return first.to_string();
                }
            }
        }
    }
    peer_str
}

/// Reads one cookie out of a Cookie header without pulling in a cookie crate:
/// the format is `a=1; b=2` and this only ever wants one name.
pub fn cookie(h: &HeaderMap, name: &str) -> Option<String> {
    let raw = header(h, "cookie")?;
    for part in raw.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix(name) {
            if let Some(value) = rest.strip_prefix('=') {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    /// A context standing in for one resolved off a real request.
    #[allow(dead_code)]
    pub(crate) fn test_ctx(lang: &str) -> Ctx {
        crate::testutil::test_ctx(lang)
    }

    /// Forwarded headers count only for a trusted-proxy peer; anyone else is
    /// keyed by their own TCP address, however many headers they send.
    #[test]
    fn client_ip_trusts_only_configured_proxies() {
        let peer: std::net::SocketAddr = "10.0.0.9:1234".parse().unwrap();
        let trusted = vec!["10.0.0.9".to_string()];

        let mut h = HeaderMap::new();
        h.insert(
            "X-Forwarded-For",
            HeaderValue::from_static("1.2.3.4, 5.6.7.8"),
        );
        // untrusted peer: the spoofable header loses
        assert_eq!(client_ip(&h, Some(peer), &[]), "10.0.0.9");
        assert_eq!(client_ip(&h, None, &trusted), "unknown");
        // trusted peer: the first XFF hop stands in for the visitor
        assert_eq!(
            client_ip(&h, Some(peer), &trusted),
            "1.2.3.4",
            "first hop, not the chain"
        );

        h.insert("CF-Connecting-IP", HeaderValue::from_static("9.9.9.9"));
        assert_eq!(
            client_ip(&h, Some(peer), &trusted),
            "9.9.9.9",
            "CF wins over XFF"
        );
        assert_eq!(client_ip(&h, Some(peer), &[]), "10.0.0.9");
    }
}
