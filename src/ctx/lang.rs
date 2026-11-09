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
    /// Absolute canonical URL, retaining meaningful listing parameters.
    pub fn canonical_url(&self) -> String {
        self.lang_url(&self.lang)
    }

    /// This listing in `lang`, preserving pagination and supported filters.
    /// Canonical and hreflang URLs share the same normalized query.
    pub fn lang_url(&self, lang: &str) -> String {
        self.listing_url(lang, None)
    }

    /// `path` in `lang`, absolute. The one place the `?lang=` rule lives: the
    /// default locale carries no param and every other one carries its own, so
    /// a helper that builds a link can never disagree with the canonical form
    /// or the hreflang alternates about what a locale's address is.
    pub fn url_for(&self, path: &str, lang: &str) -> String {
        let suffix = if lang == DEFAULT_LANG {
            String::new()
        } else {
            format!("?lang={lang}")
        };
        format!("{}{}{}", self.site_url, path, suffix)
    }

    /// A crawlable next page in the current locale and filter sequence.
    // askama hands template variables to methods by reference; taking the
    // copy by value would force every template to dereference first
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn page_url(&self, page: &i64) -> String {
        self.listing_url(&self.lang, Some(*page))
    }

    fn listing_url(&self, lang: &str, page: Option<i64>) -> String {
        let base = self.url_for(&self.path, lang);
        let Ok(mut url) = reqwest::Url::parse(&base) else {
            return base;
        };
        let allowed: &[&str] = match self.path.as_str() {
            "/cookies" | "/pets" | "/treasures" => &["page", "tab"],
            "/builds" => &["page", "sort", "cookie", "pet", "treasure", "ep", "author"],
            "/news" | "/changelog" => &["page"],
            _ => &[],
        };
        let parsed = reqwest::Url::parse(&format!("http://local/?{}", self.query));
        let mut pairs = std::collections::BTreeMap::new();
        if let Ok(parsed) = parsed {
            for (key, value) in parsed.query_pairs() {
                if allowed.contains(&key.as_ref()) {
                    pairs
                        .entry(key.into_owned())
                        .or_insert_with(|| value.into_owned());
                }
            }
        }
        let number = page.or_else(|| pairs.get("page").and_then(|v| v.parse().ok()));
        pairs.remove("page");
        let number = crate::pagination::clamp_page(number);
        if number > 1 {
            pairs.insert("page".into(), number.to_string());
        }
        if !pairs.is_empty() {
            url.query_pairs_mut().extend_pairs(pairs);
        }
        url.into()
    }

    /// The news RSS feed in this locale, absolute. Same `?lang=` rule as
    /// `lang_url` — the feed the head advertises has to be the language the
    /// page is being read in, or a Thai visitor subscribes to an English
    /// feed — but always the feed's own path, never the page's.
    ///
    /// A path, not a language: this is the one caller that needs another
    /// address than the current one, which is why `url_for` takes the path.
    pub fn news_feed_url(&self) -> String {
        self.url_for("/news/feed.xml", &self.lang)
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

    /// A cache-busting query for ride-deploy assets (`styles.css`): the
    /// binary's build moment. A fresh deploy answers with a new value, so
    /// edge caches that hold the old asset for its day-long TTL are bypassed
    /// by every page the new binary renders — no cache purge needed.
    // an associated function would be cleaner, but askama templates call
    // this through the `ctx` receiver (`base.html` and friends)
    #[allow(clippy::unused_self)]
    pub fn deploy_stamp(&self) -> u64 {
        static STAMP: std::sync::LazyLock<u64> = std::sync::LazyLock::new(|| {
            // build timestamp set by the release build profile; 0 when absent
            const BUILT: &str = env!("BUILD_TS");
            BUILT.parse().unwrap_or(0)
        });
        *STAMP
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

    #[test]
    fn listing_urls_preserve_pages_filters_and_locale() {
        let mut ctx = crate::testutil::test_ctx("th");
        ctx.path = "/builds".into();
        ctx.query = "page=2&author=A%26B&lang=th&utm_source=tracker".into();
        assert_eq!(
            ctx.canonical_url(),
            "http://localhost:6785/builds?lang=th&author=A%26B&page=2"
        );
        assert_eq!(
            ctx.lang_url("en"),
            "http://localhost:6785/builds?author=A%26B&page=2"
        );
        assert_eq!(
            ctx.page_url(&3),
            "http://localhost:6785/builds?lang=th&author=A%26B&page=3"
        );
        for query in ["page=1", "page=0", "page=-1", "page=bad"] {
            ctx.query = query.into();
            assert_eq!(ctx.canonical_url(), "http://localhost:6785/builds?lang=th");
        }
        ctx.path = "/cookies/1".into();
        ctx.query = "page=2&utm_source=tracker".into();
        assert_eq!(
            ctx.canonical_url(),
            "http://localhost:6785/cookies/1?lang=th"
        );
    }

    /// The advertised feed is the locale's own: the default locale carries no
    /// param, every other one carries its own, exactly like the alternates the
    /// canonical tag shares a helper with.
    #[test]
    fn feed_url_follows_the_locale() {
        let en = crate::testutil::test_ctx("en");
        assert_eq!(en.news_feed_url(), "http://localhost:6785/news/feed.xml");
        let th = crate::testutil::test_ctx("th");
        assert_eq!(
            th.news_feed_url(),
            "http://localhost:6785/news/feed.xml?lang=th"
        );
    }
}
