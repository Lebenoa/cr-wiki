//! The .tr files, loaded once at startup.
//!
//! Format is the V one: key line, value line, `-----` separator. veb verifies
//! keys at compile time; askama cannot, so `t` falls back to English and then
//! to the key itself rather than panicking on a miss — a missing string must
//! not take a page down.

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

pub const DEFAULT_LANG: &str = "en";

static CATALOG: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();

fn parse(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut lines = text.lines();
    while let Some(key) = lines.next() {
        let key = key.trim();
        if key.is_empty() || key == "-----" {
            continue;
        }
        let mut value = String::new();
        for line in lines.by_ref() {
            if line.trim() == "-----" {
                break;
            }
            if !value.is_empty() {
                value.push('\n');
            }
            value.push_str(line);
        }
        out.insert(key.to_string(), value);
    }
    out
}

/// Reads every `*.tr` in `dir` except `lang_map.tr`, keyed by file stem — the
/// same scan `api/available_langs.v` does, and memoized the same way.
pub fn load(dir: &str) {
    let mut catalog: HashMap<String, HashMap<String, String>> = HashMap::new();
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("tr") {
                    continue;
                }
                let stem = match path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) if s != "lang_map" => s.to_string(),
                    _ => continue,
                };
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        catalog.insert(stem, parse(&text));
                    }
                    Err(e) => {
                        // a dropped language answers every key with the key
                        // itself, which looks like data loss on the site
                        tracing::warn!("i18n: cannot read {}: {e}", path.display());
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("i18n: cannot scan {dir}: {e}; every key will render as itself");
        }
    }
    let _ = CATALOG.set(catalog);
}

fn catalog() -> &'static HashMap<String, HashMap<String, String>> {
    CATALOG.get_or_init(HashMap::new)
}

/// The locales the site is served in, sorted so the order is stable.
pub fn available_langs() -> Vec<String> {
    let mut out: Vec<String> = catalog().keys().cloned().collect();
    out.sort();
    out
}

/// `lang_map.tr`'s locale → display name (en → English, th → ไทย); the code
/// itself when unmapped, so a new locale renders even before it is added.
pub fn lang_display(lang: &str) -> String {
    static MAP: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
        match std::fs::read_to_string("translations/lang_map.tr") {
            Ok(text) => parse(&text),
            Err(e) => {
                tracing::warn!("i18n: lang_map.tr unavailable ({e}); language names fall back to codes");
                HashMap::new()
            }
        }
    });
    MAP.get(lang)
        .cloned()
        .unwrap_or_else(|| lang.to_string())
}

/// One locale's data as the language modal needs it: code, display name from
/// `lang_map.tr`, and a small flag so the choice reads at a glance.
pub struct LangOption {
    pub code: String,
    pub display: String,
    pub flag: &'static str,
}

/// Per-locale flag art as an inline SVG snippet (rounded badge, sized by the
/// template's utility classes). Currently en → UK, th → Thailand; a locale
/// without an entry falls back to a neutral globe so a newly added language
/// never renders without an icon.
pub fn lang_flag(lang: &str) -> &'static str {
    match lang {
        "en" => r##"<svg viewBox="0 0 24 24" aria-hidden="true"><defs><clipPath id="flagclip-en"><circle cx="12" cy="12" r="12"/></clipPath></defs><g clip-path="url(#flagclip-en)"><rect width="24" height="24" fill="#012169"/><path d="M0 0 24 24M24 0 0 24" stroke="#fff" stroke-width="4.8"/><path d="M0 0 24 24M24 0 0 24" stroke="#C8102E" stroke-width="2.4"/><path d="M12 0v24M0 12h24" stroke="#fff" stroke-width="8"/><path d="M12 0v24M0 12h24" stroke="#C8102E" stroke-width="4.4"/></g></svg>"##,
        "th" => r##"<svg viewBox="0 0 24 24" aria-hidden="true"><defs><clipPath id="flagclip-th"><circle cx="12" cy="12" r="12"/></clipPath></defs><g clip-path="url(#flagclip-th)"><rect width="24" height="24" fill="#A51931"/><rect y="4" width="24" height="16" fill="#F4F5F8"/><rect y="8" width="24" height="8" fill="#2D2A4A"/></g></svg>"##,
        _ => r##"<svg viewBox="0 0 24 24" aria-hidden="true"><defs><clipPath id="flagclip-globe"><circle cx="12" cy="12" r="12"/></clipPath></defs><g clip-path="url(#flagclip-globe)" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.5 3.8 5.6 3.8 9s-1.3 6.5-3.8 9c-2.5-2.5-3.8-5.6-3.8-9S9.5 5.5 12 3Z"/></g></svg>"##,
    }
}

/// Every available locale as a `LangOption`, sorted by code (the same order
/// `available_langs` gives the dialog).
pub fn lang_options() -> Vec<LangOption> {
    available_langs()
        .into_iter()
        .map(|code| LangOption {
            display: lang_display(&code),
            flag: lang_flag(&code),
            code,
        })
        .collect()
}

pub fn is_available(lang: &str) -> bool {
    catalog().contains_key(lang)
}

pub fn t(lang: &str, key: &str) -> String {
    let c = catalog();
    c.get(lang)
        .and_then(|m| m.get(key))
        .or_else(|| c.get(DEFAULT_LANG).and_then(|m| m.get(key)))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// One locale's prose beside another's: the requested language's string when
/// it carries text, English otherwise. Every `(en, th)` column pair on a
/// record reads through this, so a third locale adds no new call sites.
pub fn pick_text(lang: &str, en: &str, localized: &str) -> String {
    if lang != DEFAULT_LANG && !localized.is_empty() {
        localized.to_string()
    } else {
        en.to_string()
    }
}

/// A language bound to a translator, so templates can write `l.t("key")`
/// instead of threading the locale through every call.
#[derive(Clone, Debug)]
pub struct Loc {
    pub lang: String,
}

impl Loc {
    pub fn new(lang: impl Into<String>) -> Self {
        Self { lang: lang.into() }
    }
    pub fn t(&self, key: &str) -> String {
        t(&self.lang, key)
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    #[test]
    fn translations_load_with_fallback_chain() {
        load("translations");
        assert!(available_langs().contains(&"en".to_string()));
        assert!(available_langs().contains(&"th".to_string()));
        assert_eq!(t("en", "changelog_page_header"), "Changelog");
        assert_eq!(t("en", "changelog_unavailable"), "No changelog available.");
        assert_ne!(t("th", "changelog_page_header"), "Changelog");
        // a miss falls back to English, then to the key itself
        assert_eq!(t("th", "no_such_key_at_all"), "no_such_key_at_all");
    }

    #[test]
    fn available_langs_covers_both_catalogs() {
        load("translations");
        assert!(is_available("th"));
        assert!(!is_available("zz"));
    }

    #[test]
    fn lang_display_reads_map_and_falls_back_to_code() {
        assert_eq!(lang_display("en"), "English");
        assert_eq!(lang_display("th"), "ไทย");
        // an unmapped locale renders as the code, never empty
        assert_eq!(lang_display("zz"), "zz");
    }

    #[test]
    fn lang_options_carry_flag_and_display() {
        load("translations");
        let opts = lang_options();
        let en = opts.iter().find(|o| o.code == "en").expect("en option");
        assert_eq!(en.display, "English");
        assert!(en.flag.contains("<svg"));
        let th = opts.iter().find(|o| o.code == "th").expect("th option");
        assert_eq!(th.display, "ไทย");
        assert!(th.flag.contains("<svg"));
    }

    #[test]
    fn pick_text_picks_locale_then_english() {
        assert_eq!(pick_text("th", "en text", "th text"), "th text");
        // an empty translation never shows as an empty field
        assert_eq!(pick_text("th", "en text", ""), "en text");
        // English reads its own column, never the other locale's
        assert_eq!(pick_text("en", "en text", "th text"), "en text");
    }
}
