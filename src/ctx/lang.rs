//! The locale half of the context: the `wikilang` cookie, the `?lang=`
//! resolution the middleware runs, and every URL and label built from the
//! language the visitor ended up on.
//!
//! `?lang=` wins over the cookie: the language has to live in the URL or the
//! two locales share one address, which leaves only the default one indexable
//! and gives hreflang nothing to point at. The cookie carries the choice
//! across links that omit the param.

use axum::http::HeaderMap;

use crate::i18n::{self, DEFAULT_LANG};

use super::{cookie, Ctx};
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

// Four independent request facts live here as bools on purpose: they are not

impl Ctx {
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
}
