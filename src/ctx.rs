//! Per-request context: the locale, the path, and the URL/header helpers the
//! templates ask for. Mirrors the veb `Context` struct and `before_request`.
//!
//! Ctx is the request-facts half of the old god-struct: what the visitor
//! sent and who they are (lang, path, user, htmx, `is_admin`). The chrome —
//! navigation lists, themes, EP tiers, the class strings — lives in
//! `crate::chrome`, which knows nothing about requests; these methods
//! delegate so askama still finds the names on the context. The doc-comment
//! path references below are crate paths, hence the backticks.

use axum::extract::ConnectInfo;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use std::convert::Infallible;
use std::net::SocketAddr;

use crate::i18n::{self, Loc, DEFAULT_LANG};
use crate::section::Section;
use crate::session::SessionUser;

pub const LANG_COOKIE: &str = "wikilang";

/// What the lang middleware resolved, parked in the request extensions so the
/// extractor and the response layer both see it.
#[derive(Clone, Debug)]
pub struct LangChoice {
    pub lang: String,
    /// set when the response should refresh the cookie, as `before_request`
    /// does
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
        Self {
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
        let suffix = if lang == DEFAULT_LANG {
            String::new()
        } else {
            format!("?lang={lang}")
        };
        format!("{}{}{}", self.site_url, self.path, suffix)
    }

    /// The language modal's switch URL, distinct from `lang_url`: it always
    /// carries the explicit `?lang=` (English included), because
    /// `resolve_lang` lets a `?lang=` query beat the `wikilang` cookie while
    /// the cookie beats an absent param. The clean canonical form of the
    /// default locale would re-render the cookie's language and switching
    /// to English could never work.
    pub fn lang_switch_url(&self, lang: &str) -> String {
        format!("{}{}?lang={}", self.site_url, self.path, lang)
    }

    /// The loaded locales; a method because askama calls it on the context.
    #[allow(clippy::unused_self)]
    pub fn langs(&self) -> Vec<String> {
        i18n::available_langs()
    }

    /// Each locale with its `lang_map.tr` display name and flag, as the
    /// language modal renders them.
    #[allow(clippy::unused_self)]
    pub fn lang_options(&self) -> Vec<i18n::LangOption> {
        i18n::lang_options()
    }

    /// The locale's `lang_map.tr` display name; a method so templates can
    /// render options as "English"/"ไทย" instead of bare codes.
    #[allow(clippy::unused_self)]
    pub fn lang_display(&self, lang: &str) -> String {
        i18n::lang_display(lang)
    }

    /// The locale's flag SVG, as the navbar's Language button and the modal
    /// render it.
    #[allow(clippy::unused_self)]
    pub fn lang_flag(&self, lang: &str) -> &'static str {
        i18n::lang_flag(lang)
    }

    /// The sections behind the Wiki dropdown, in navbar order.
    #[allow(clippy::unused_self)]
    pub fn wiki_sections(&self) -> Vec<&'static str> {
        crate::chrome::NAV_SECTIONS
            .iter()
            .map(|s| s.as_str())
            .collect()
    }

    #[allow(dead_code)] // used through templates, invisible to rustc
    pub fn nav_section_active(&self, section: &str) -> bool {
        crate::chrome::nav_section_active(&self.path, section)
    }

    pub fn nav_class(&self, section: &str) -> &'static str {
        crate::chrome::nav_class(&self.path, section)
    }

    /// True when the current page sits under any wiki section, so the
    /// dropdown trigger can carry the active styling its entries would.
    #[allow(dead_code)] // used through templates, invisible to rustc
    pub fn wiki_active(&self) -> bool {
        crate::chrome::wiki_active(&self.path)
    }

    pub fn wiki_class(&self) -> &'static str {
        crate::chrome::wiki_class(&self.path)
    }

    /// The theme names the picker offers.
    #[allow(clippy::unused_self)]
    pub fn themes(&self) -> Vec<&'static str> {
        crate::chrome::THEMES.to_vec()
    }

    /// The palette tokens the custom-theme editor may override.
    #[allow(clippy::unused_self)]
    pub fn theme_tokens(&self) -> Vec<&'static str> {
        crate::chrome::THEME_TOKENS.to_vec()
    }

    #[allow(clippy::unused_self)]
    pub fn theme_token_key(&self, token: &str) -> String {
        crate::chrome::theme_token_key(token)
    }

    #[allow(clippy::unused_self)]
    pub fn theme_key(&self, name: &str) -> String {
        crate::chrome::theme_key(name)
    }

    /// A combo pairs a cookie with a pet, so the partner of one is the other.
    #[allow(clippy::unused_self, clippy::missing_const_for_fn)]
    pub fn combi_partner_section(&self, section: &str) -> &'static str {
        crate::chrome::combi_partner_section(section)
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

    /// "EP 5" / "Special EP 2", localized. A special tier wins over the
    /// plain one, matching the badge.
    pub fn build_ep_label(&self, ep: i64, ep_special: i64) -> String {
        if ep_special > 0 {
            return self
                .l
                .t("build_ep_special")
                .replace("{n}", &ep_special.to_string());
        }
        self.l.t("build_ep_tier").replace("{n}", &ep.to_string())
    }

    /// The combobox option labels. Askama hands loop variables out by
    /// reference, so these take one rather than making every call site deref.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn ep_tier_label(&self, n: &i64) -> String {
        self.build_ep_label(*n, 0)
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn ep_special_label(&self, n: &i64) -> String {
        self.build_ep_label(0, *n)
    }

    /// The regular EP tiers and the special ones, for the filter combobox.
    #[allow(clippy::unused_self)]
    pub fn ep_tiers(&self) -> Vec<i64> {
        crate::chrome::EP_TIERS.to_vec()
    }

    #[allow(clippy::unused_self)]
    pub fn ep_specials(&self) -> Vec<i64> {
        crate::chrome::EP_SPECIALS.to_vec()
    }

    /// A page title with the site name appended.
    pub fn page_title(&self, key: &str) -> String {
        crate::chrome::page_title(&self.l, key)
    }

    /// The same, for a title built from an entity name rather than a key.
    // the {name} placeholder is consumed by replace(), not a formatting macro
    #[allow(clippy::literal_string_with_formatting_args)]
    pub fn entity_title(&self, name: &str) -> String {
        crate::chrome::entity_title(&self.l, name)
    }

    /// The site-wide description, used when a page names no key of its own.
    pub fn meta_description(&self) -> String {
        self.l.t("site_description")
    }

    /// A page's own description. A key with no string behind it falls back
    /// to the site one rather than printing the key.
    pub fn page_desc(&self, key: &str) -> String {
        crate::chrome::page_desc(&self.l, key)
    }

    /// A detail page's description. Only some kinds have a template of their
    /// own; the rest read the site line.
    // the {name} placeholder is consumed by replace(), not a formatting macro
    #[allow(clippy::literal_string_with_formatting_args)]
    pub fn entity_desc(&self, section: &str, name: &str) -> String {
        crate::chrome::entity_desc(&self.l, section, name)
    }

    /// The wide banner, absolute, for Open Graph and Twitter cards.
    #[allow(clippy::unused_self)] // askama calls it on the context
    pub fn social_image(&self) -> String {
        format!(
            "{}/static/img/landscape.jpg",
            self.site_url.trim_end_matches('/')
        )
    }

    /// A detail page shares the entity's own sprite instead, when it has one.
    pub fn entity_image(&self, section: &str, image: Option<&String>) -> String {
        match image {
            Some(img) if !img.is_empty() => {
                format!(
                    "{}/static/img/{section}/{img}",
                    self.site_url.trim_end_matches('/')
                )
            }
            _ => self.social_image(),
        }
    }

    /// An entity sprite is a square icon, so it reads better as a summary
    /// thumbnail; the wide banner wants the large card. A method because
    /// askama calls it on the context.
    #[allow(clippy::unused_self)]
    pub fn social_card_type(&self, image: &str) -> &'static str {
        if image.ends_with("/static/img/landscape.jpg") {
            "summary_large_image"
        } else {
            "summary"
        }
    }

    /// The image URL's MIME type from its extension; sprites are PNG, the
    /// banner JPEG. A method because askama's lexer has no single-quoted
    /// strings, so the branch cannot live inline in the attribute. The
    /// comparison is case-insensitive out of paranoia about what a hand
    /// edit of the image column can hold.
    #[allow(clippy::unused_self)]
    pub fn image_mime(&self, image: &str) -> &'static str {
        if image.to_ascii_lowercase().ends_with(".png") {
            "image/png"
        } else {
            "image/jpeg"
        }
    }

    /// The image URL's on-disk size for `og:image:width`/`height`, so
    /// platform previews reserve the right aspect for a detail page's sprite
    /// as well as the wide banner. Banner dims when the file is missing or
    /// unreadable — a wrong hint is better than an absent one, and the
    /// sprite is served from `/static/img/...`, which this method maps to
    /// `static/img/...`.
    #[allow(clippy::unused_self)]
    pub fn image_dimensions(&self, image: &str) -> (u32, u32) {
        const BANNER: (u32, u32) = (2560, 1440);
        let Some(path) = image
            .strip_prefix(&format!("{}/", self.site_url.trim_end_matches('/')))
            .and_then(|rel| rel.strip_prefix("static/"))
            .map(|rel| format!("static/{rel}"))
        else {
            return BANNER;
        };
        let read = |f: std::fs::File| -> Option<(u32, u32)> {
            imagesize::reader_size(std::io::BufReader::new(f))
                .ok()
                .map(|d| {
                    (
                        u32::try_from(d.width).unwrap_or(0),
                        u32::try_from(d.height).unwrap_or(0),
                    )
                })
        };
        std::fs::File::open(&path).map_or(BANNER, |f| read(f).unwrap_or(BANNER))
    }

    /// The wikilang cookie as an Open Graph locale tag.
    pub fn og_locale(&self) -> &'static str {
        if self.lang == "th" {
            "th_TH"
        } else {
            "en_US"
        }
    }

    pub fn default_lang_url(&self) -> String {
        self.lang_url(crate::i18n::DEFAULT_LANG)
    }

    /// The per-language Google Fonts stylesheet. Thai needs its own face,
    /// and shipping both to every visitor is two families nobody reads.
    pub fn font_css_url(&self) -> &'static str {
        // only the weights the stylesheet uses (400-800); the variable-font
        // axis otherwise ships ten
        if self.lang == "th" {
            "https://fonts.googleapis.com/css2?family=Noto+Sans+Thai:wght@400..800&display=swap"
        } else {
            "https://fonts.googleapis.com/css2?family=Hanken+Grotesk:ital,wght@0,400..800;1,400..800&family=JetBrains+Mono:ital,wght@0,400..800;1,400..800&family=Sora:wght@400..800&display=swap"
        }
    }

    /// The draw pool's display name. The `name` column holds a .tr key
    /// rather than prose, so the tier is what actually reads.
    pub fn gacha_tier_label(&self, tier: &str) -> String {
        self.l.t(&format!("gacha_tier_{tier}"))
    }

    /// Localizes an episode kind code (story/special/event).
    pub fn episode_kind_label(&self, kind: &str) -> String {
        self.l.t(&format!("episode_kind_{kind}"))
    }

    /// An episode's difficulty as repeated stars ('' when unrated).
    #[allow(clippy::unused_self, clippy::trivially_copy_pass_by_ref)]
    // askama passes template arguments by reference, so the parameter has
    // to stay `&i64` even though i64 is Copy
    pub fn stars_label(&self, stars: &i64) -> String {
        "\u{2605}".repeat(usize::try_from((*stars).max(0)).unwrap_or(0))
    }

    /// A stored grade value as its label ("S+" for `s_plus`), or '' when the
    /// row has no grade. The ungraded grids (ingredients, skins) show it.
    #[allow(clippy::unused_self, clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn grade_label_int(&self, grade: &Option<i64>) -> String {
        (*grade).map_or_else(String::new, crate::grade::label)
    }

    /// The full badge class list for a stored grade value, so every grade
    /// reads as its own colour on the ungraded grids. Wind4's palette is
    /// replaced by the semantic block in uno.config.ts, so the colors these
    /// name are declared there.
    #[allow(clippy::unused_self, clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn grade_badge_cls(&self, grade: &Option<i64>) -> &'static str {
        match grade.unwrap_or(-1) {
            1 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-lime-400 text-lime-400",
            2 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-yellow-400 text-yellow-400",
            3 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-orange-400 text-orange-400",
            4 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-red-400 text-red-400",
            5 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-fuchsia-400 text-fuchsia-400",
            6 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-violet-400 text-violet-400",
            _ => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-secondary/40 text-foreground-muted",
        }
    }

    /// An episode id as the short badge the ingredient cards and the
    /// An episode id as the short badge the ingredient cards and the
    /// drop-location tile use: "EP 2" story, "SP 1" special (501+), "EP 6-1"
    /// event (601+). Empty for none, so a template can test it directly.
    // The subtraction is range-checked just above it; the div/mod by the
    // constant 100 cannot fault.
    #[allow(clippy::arithmetic_side_effects)]
    #[allow(clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn episode_short(&self, id: &Option<i64>) -> String {
        let Some(eid) = *id else {
            return String::new();
        };
        if (500..600).contains(&eid) {
            return format!("{} {}", self.l.t("special_episode_abbrev"), eid - 500);
        }
        if eid >= 600 {
            return format!("{} {}-{}", self.l.t("episode_abbrev"), eid / 100, eid % 100);
        }
        format!("{} {eid}", self.l.t("episode_abbrev"))
    }

    /// The badge class list for an episode id, so each episode reads as its
    /// own colour on the ingredient cards. Story episodes run cool-to-warm
    /// in play order, the special episodes take the remaining cool hues and
    /// the event ones the reds.
    #[allow(clippy::unused_self, clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn episode_badge_cls(&self, id: &Option<i64>) -> &'static str {
        match id.unwrap_or(0) {
            1 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-emerald-400 text-emerald-400",
            2 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-lime-400 text-lime-400",
            3 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-orange-400 text-orange-400",
            4 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-sky-400 text-sky-400",
            5 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-pink-400 text-pink-400",
            6 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-fuchsia-400 text-fuchsia-400",
            7 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-amber-400 text-amber-400",
            501 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-yellow-400 text-yellow-400",
            502 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-cyan-400 text-cyan-400",
            503 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-violet-400 text-violet-400",
            601 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-teal-400 text-teal-400",
            602 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-indigo-400 text-indigo-400",
            701 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-red-400 text-red-400",
            _ => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-secondary/40 text-foreground-muted",
        }
    }

    /// The same palette as `episode_badge_cls` without the pill, for the
    /// drop-location tile where the number sits inline before the name.
    #[allow(clippy::unused_self, clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn episode_text_cls(&self, id: &Option<i64>) -> &'static str {
        match id.unwrap_or(0) {
            1 => "text-emerald-400",
            2 => "text-lime-400",
            3 => "text-orange-400",
            4 => "text-sky-400",
            5 => "text-pink-400",
            6 => "text-fuchsia-400",
            7 => "text-amber-400",
            501 => "text-yellow-400",
            502 => "text-cyan-400",
            503 => "text-violet-400",
            601 => "text-teal-400",
            602 => "text-indigo-400",
            701 => "text-red-400",
            _ => "text-accent",
        }
    }

    /// A jelly's score value without trailing zeroes (432.9 -> "432.9",
    /// 5000 -> "5000"). Formatted, then trimmed: no float comparisons or
    /// casts, and Display for f64 never prints "5000.0" anyway.
    #[allow(clippy::unused_self, clippy::trivially_copy_pass_by_ref)]
    // askama passes template arguments by reference, so the parameter has
    // to stay `&f64`
    pub fn score_label(&self, score: &f64) -> String {
        let mut s = format!("{score}");
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        s
    }

    /// The grade-filter buttons a tabbed catalog shows: (slug, label) in
    /// declaration order. The filter JS keys on the data-grade slug.
    #[allow(clippy::unused_self)]
    pub fn grade_options(&self) -> Vec<(&'static str, &'static str)> {
        [
            ("c", "C"),
            ("b", "B"),
            ("a", "A"),
            ("s", "S"),
            ("s_plus", "S+"),
            ("l", "L"),
            ("e", "E"),
        ]
        .into_iter()
        .collect()
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
                    return LangChoice {
                        lang: v.to_string(),
                        write_cookie: write,
                    };
                }
            }
        }
    }
    match cookie(h, LANG_COOKIE) {
        Some(c) if i18n::is_available(&c) => LangChoice {
            lang: c,
            write_cookie: false,
        },
        _ => LangChoice {
            lang: DEFAULT_LANG.to_string(),
            write_cookie: true,
        },
    }
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

    /// Locale resolution: ?lang= wins over the cookie, an unknown locale is
    /// ignored, and a bare request falls back to English while asking for the
    /// cookie to be written.
    #[test]
    fn lang_resolution() {
        crate::i18n::load("translations");

        let empty = HeaderMap::new();
        let c = resolve_lang(None, &empty);
        assert_eq!(c.lang, "en");
        assert!(c.write_cookie);

        let c = resolve_lang(Some("lang=th"), &empty);
        assert_eq!(c.lang, "th");
        assert!(c.write_cookie);

        // an unknown locale is ignored, not echoed back
        let c = resolve_lang(Some("lang=zz"), &empty);
        assert_eq!(c.lang, "en");

        let mut with_cookie = HeaderMap::new();
        with_cookie.insert("cookie", HeaderValue::from_static("wikilang=th; other=1"));
        let c = resolve_lang(None, &with_cookie);
        assert_eq!(c.lang, "th");
        assert!(!c.write_cookie, "cookie already agrees, no need to rewrite");

        // the param wins over a disagreeing cookie, and refreshes it
        let c = resolve_lang(Some("page=2&lang=en"), &with_cookie);
        assert_eq!(c.lang, "en");
        assert!(c.write_cookie);
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
