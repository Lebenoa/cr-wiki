//! Per-request context: the locale, the path, and the URL/header helpers the
//! templates ask for. Mirrors the veb `Context` struct and `before_request`.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use std::convert::Infallible;

use crate::i18n::{self, Loc, DEFAULT_LANG};
use crate::session::SessionUser;

pub const LANG_COOKIE: &str = "wikilang";

/// What the lang middleware resolved, parked in the request extensions so the
/// extractor and the response layer both see it.
#[derive(Clone, Debug)]
pub struct LangChoice {
    pub lang: String,
    /// set when the response should refresh the cookie, as before_request does
    pub write_cookie: bool,
}

#[derive(Clone, Debug)]
pub struct Ctx {
    pub lang: String,
    pub l: Loc,
    /// request path with the query stripped; the navbar asks for it many
    /// times per page, so it is resolved once
    pub path: String,
    pub site_url: String,
    pub htmx: bool,
    pub boosted: bool,
    pub is_local: bool,
    pub user: Option<SessionUser>,
}

impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let lang = parts
            .extensions
            .get::<LangChoice>()
            .map(|c| c.lang.clone())
            .unwrap_or_else(|| DEFAULT_LANG.to_string());
        let user = parts.extensions.get::<SessionUser>().cloned();
        let h = &parts.headers;
        Ok(Ctx {
            user,
            l: Loc::new(&lang),
            lang,
            path: parts.uri.path().to_string(),
            site_url: site_url(h),
            htmx: header(h, "HX-Request").is_some(),
            boosted: header(h, "HX-Boosted").is_some(),
            is_local: is_local(h),
        })
    }
}

impl Ctx {
    /// The clean (query-free) absolute URL of this page.
    pub fn canonical_url(&self) -> String {
        self.lang_url(&self.lang)
    }

    /// This page in `lang`: the current path, plus ?lang= for every locale but
    /// the default. Backs both the canonical tag and the hreflang alternates,
    /// so the two cannot disagree.
    pub fn lang_url(&self, lang: &str) -> String {
        let suffix = if lang == DEFAULT_LANG { String::new() } else { format!("?lang={lang}") };
        format!("{}{}{}", self.site_url, self.path, suffix)
    }

    pub fn langs(&self) -> Vec<String> {
        i18n::available_langs()
    }

    /// True when the path is `/section` or sits under it, so the navbar can
    /// mark the active entry.
    pub fn nav_section_active(&self, section: &str) -> bool {
        let p = self.path.trim_start_matches('/');
        p == section || p.starts_with(&format!("{section}/"))
    }

    /// The navbar entry's classes, active or not — the same two class lists
    /// nav_link builds in app.v.
    pub fn nav_class(&self, section: &str) -> &'static str {
        if self.nav_section_active(section) {
            "font-semibold text-center text-primary border-b-2 border-primary pb-1 transition-all duration-1000"
        } else {
            "font-medium text-center text-foreground-muted hover:text-primary transition-all duration-1000"
        }
    }

    /// The sections behind the Wiki dropdown, in navbar order.
    pub fn wiki_sections(&self) -> Vec<&'static str> {
        vec![
            "cookies",
            "pets",
            "treasures",
            "episodes",
            "ingredients",
            "jellies",
            "skins",
            "gacha",
        ]
    }

    /// True when the current page sits under any wiki section, so the
    /// dropdown trigger can carry the active styling its entries would.
    pub fn wiki_active(&self) -> bool {
        self.wiki_sections().iter().any(|s| self.nav_section_active(s))
    }

    pub fn wiki_class(&self) -> &'static str {
        if self.wiki_active() {
            "font-semibold text-center text-primary border-b-2 border-primary pb-1 transition-all duration-1000"
        } else {
            "font-medium text-center text-foreground-muted hover:text-primary transition-all duration-1000"
        }
    }

    /// The theme names the picker offers. The palettes live in the UnoCSS
    /// preflight; the dropdown only toggles the data-theme attribute.
    pub fn themes(&self) -> Vec<&'static str> {
        vec![
            "default",
            "light",
            "tokyo_night",
            "cappuccino",
            "dracula",
            "nord",
            "gruvbox",
            "rose_pine",
        ]
    }

    /// The palette tokens the custom-theme editor may override. The --on-*
    /// contrast partners are derived from the chosen colour rather than
    /// exposed, so a theme cannot end up with unreadable text on a button.
    pub fn theme_tokens(&self) -> Vec<&'static str> {
        vec![
            "background",
            "surface",
            "border",
            "primary",
            "secondary",
            "accent",
            "muted",
            "foreground",
            "foreground-muted",
            "success",
            "warning",
            "error",
        ]
    }

    /// theme_token_* keys use an underscore, since a hyphen is not valid in
    /// a translation key.
    pub fn theme_token_key(&self, token: &str) -> String {
        format!("theme_token_{}", token.replace('-', "_"))
    }

    pub fn theme_key(&self, name: &str) -> String {
        format!("theme_{name}")
    }

    /// A combo pairs a cookie with a pet, so the partner of one is the other.
    pub fn combi_partner_section(&self, section: &str) -> &'static str {
        if section == "cookies" { "pets" } else { "cookies" }
    }

    /// A fragment swap, as opposed to an hx-boosted navigation which still
    /// wants the whole page.
    pub fn is_fragment(&self) -> bool {
        self.htmx && !self.boosted
    }

    /// Admin permission: an admin session, or a loopback request — which is
    /// what makes local development admin without a login, as in app.v.
    pub fn is_admin(&self) -> bool {
        self.user.as_ref().is_some_and(|u| u.is_admin) || self.is_local
    }

    pub fn username(&self) -> String {
        self.user.as_ref().map(|u| u.username.clone()).unwrap_or_default()
    }

    pub fn signed_in(&self) -> bool {
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
    match header(h, "host") {
        Some(host) => format!("{scheme}://{host}"),
        None => "http://localhost:6785".to_string(),
    }
}

/// Loopback only when no proxy header is present: any of them means the peer
/// address is the proxy's, not the visitor's.
fn is_local(h: &HeaderMap) -> bool {
    if header(h, "CF-Connecting-IP").is_some()
        || header(h, "X-Forwarded-For").is_some()
        || header(h, "X-Real-Ip").is_some()
    {
        return false;
    }
    true
}

/// The requester's IP, through the same trusted proxy headers veb checks,
/// then the peer address. Requests with no address share one bucket.
pub fn client_ip(h: &HeaderMap, peer: Option<std::net::SocketAddr>) -> String {
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
    peer.map(|a| a.ip().to_string()).unwrap_or_else(|| "unknown".to_string())
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

/// `?lang=` wins over the cookie: the language has to live in the URL or the
/// two locales share one address, which leaves only the default indexable and
/// gives hreflang nothing to point at. The cookie carries the choice across
/// links that omit the param. Returns the locale and whether the response
/// should refresh the cookie.
pub fn resolve_lang(query: Option<&str>, h: &HeaderMap) -> LangChoice {
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some(v) = pair.strip_prefix("lang=") {
                if i18n::is_available(v) {
                    let write = cookie(h, LANG_COOKIE).as_deref() != Some(v);
                    return LangChoice { lang: v.to_string(), write_cookie: write };
                }
            }
        }
    }
    match cookie(h, LANG_COOKIE) {
        Some(c) if i18n::is_available(&c) => LangChoice { lang: c, write_cookie: false },
        _ => LangChoice { lang: DEFAULT_LANG.to_string(), write_cookie: true },
    }
}
