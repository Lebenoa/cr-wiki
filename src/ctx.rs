//! Per-request context: the locale, the path, and the URL/header helpers the
//! templates ask for. Mirrors the veb `Context` struct and `before_request`.

use axum::extract::FromRequestParts;
use axum::extract::ConnectInfo;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use std::convert::Infallible;
use std::net::SocketAddr;

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
    // read by the debug-build admin bypass only; a release binary never
    // touches it
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
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
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        Ok(Ctx {
            user,
            l: Loc::new(&lang),
            lang,
            path: parts.uri.path().to_string(),
            site_url: site_url(h),
            htmx: header(h, "HX-Request").is_some(),
            boosted: header(h, "HX-Boosted").is_some(),
            is_local: is_local(h, peer),
        })
    }
}

impl Ctx {
    /// A context with no request behind it, for the panic handler: by the
    /// time it runs the extractors are gone, and the 500 page still has a
    /// navbar to render.
    pub fn minimal() -> Self {
        Ctx {
            lang: crate::i18n::DEFAULT_LANG.to_string(),
            l: crate::i18n::Loc::new(crate::i18n::DEFAULT_LANG),
            path: "/".to_string(),
            site_url: String::new(),
            htmx: false,
            boosted: false,
            is_local: false,
            user: None,
        }
    }

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
        self.user.as_ref().map(|u| u.username.clone()).unwrap_or_default()
    }

    /// "EP 5" / "Special EP 2", localized. A special tier wins over the
    /// plain one, matching the badge.
    pub fn build_ep_label(&self, ep: i64, ep_special: i64) -> String {
        if ep_special > 0 {
            return self.l.t("build_ep_special").replace("{n}", &ep_special.to_string());
        }
        self.l.t("build_ep_tier").replace("{n}", &ep.to_string())
    }

    /// The combobox option labels. Askama hands loop variables out by
    /// reference, so these take one rather than making every call site deref.
    pub fn ep_tier_label(&self, n: &i64) -> String {
        self.build_ep_label(*n, 0)
    }

    pub fn ep_special_label(&self, n: &i64) -> String {
        self.build_ep_label(0, *n)
    }

    /// The regular EP tiers and the special ones, for the filter combobox.
    pub fn ep_tiers(&self) -> Vec<i64> {
        (1..=7).collect()
    }

    pub fn ep_specials(&self) -> Vec<i64> {
        (1..=3).collect()
    }

    /// A page title with the site name appended. Older .tr values bake the
    /// suffix in and newer ones do not, so it is added here when missing
    /// rather than leaving half the sections unbranded.
    pub fn page_title(&self, key: &str) -> String {
        self.title_of(self.l.t(key))
    }

    /// The same, for a title built from an entity name rather than a key.
    pub fn entity_title(&self, name: &str) -> String {
        self.title_of(self.l.t("entity_detail_title").replace("{name}", name))
    }

    fn title_of(&self, title: String) -> String {
        let suffix = self.l.t("site_title_suffix");
        if suffix.is_empty() || title.contains(&suffix) {
            return title;
        }
        format!("{title} | {suffix}")
    }

    /// The site-wide description, used when a page names no key of its own.
    pub fn meta_description(&self) -> String {
        self.l.t("site_description")
    }

    /// A page's own description. A key with no string behind it falls back
    /// to the site one rather than printing the key.
    pub fn page_desc(&self, key: &str) -> String {
        let text = self.l.t(key);
        if text == key || text.is_empty() {
            return self.meta_description();
        }
        text
    }

    /// A detail page's description. Only some kinds have a template of their
    /// own; the rest read the site line.
    pub fn entity_desc(&self, section: &str, name: &str) -> String {
        let key = match section {
            "cookies" | "pets" | "treasures" => "entity_detail_description",
            "episodes" => "episode_detail_description",
            "ingredients" => "ingredient_detail_description",
            "jellies" => "jelly_detail_description",
            _ => return self.meta_description(),
        };
        self.page_desc(key).replace("{name}", name)
    }

    /// The wide banner, absolute, for Open Graph and Twitter cards.
    pub fn social_image(&self) -> String {
        format!("{}/img/landscape.jpg", self.site_url.trim_end_matches('/'))
    }

    /// A detail page shares the entity's own sprite instead, when it has one.
    pub fn entity_image(&self, section: &str, image: Option<&String>) -> String {
        match image {
            Some(img) if !img.is_empty() => {
                format!("{}/img/{section}/{img}", self.site_url.trim_end_matches('/'))
            }
            _ => self.social_image(),
        }
    }

    /// An entity sprite is a square icon, so it reads better as a summary
    /// thumbnail; the wide banner wants the large card.
    pub fn social_card_type(&self, image: &str) -> &'static str {
        if image.ends_with("/img/landscape.jpg") {
            "summary_large_image"
        } else {
            "summary"
        }
    }

    /// The wikilang cookie as an Open Graph locale tag.
    pub fn og_locale(&self) -> &'static str {
        if self.lang == "th" { "th_TH" } else { "en_US" }
    }

    pub fn default_lang_url(&self) -> String {
        self.lang_url(crate::i18n::DEFAULT_LANG)
    }

    /// The per-language Google Fonts stylesheet. Thai needs its own face,
    /// and shipping both to every visitor is two families nobody reads.
    pub fn font_css_url(&self) -> &'static str {
        if self.lang == "th" {
            "https://fonts.googleapis.com/css2?family=Noto+Sans+Thai:wght@100..900&display=swap"
        } else {
            "https://fonts.googleapis.com/css2?family=Hanken+Grotesk:ital,wght@0,100..900;1,100..900&family=JetBrains+Mono:ital,wght@0,100..800;1,100..800&family=Sora:wght@100..800&display=swap"
        }
    }

    /// The draw pool's display name. The `name` column holds a .tr key
    /// rather than prose, so the tier is what actually reads.
    pub fn gacha_tier_label(&self, tier: &str) -> String {
        self.l.t(&format!("gacha_tier_{tier}"))
    }

    /// The public Turnstile site key, for the widget divs.
    pub fn turnstile_sitekey(&self) -> &'static str {
        crate::turnstile::SITEKEY
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
    let peer_str = peer
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());
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
