//! The .tr files, loaded once at startup.
//!
//! Format is the V one: key line, value line, `-----` separator. veb verifies
//! keys at compile time; askama cannot, so `t` falls back to English and then
//! to the key itself rather than panicking on a miss — a missing string must
//! not take a page down.

use std::collections::HashMap;
use std::sync::OnceLock;

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
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("tr") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) if s != "lang_map" => s.to_string(),
                _ => continue,
            };
            if let Ok(text) = std::fs::read_to_string(&path) {
                catalog.insert(stem, parse(&text));
            }
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
    fn pick_text_picks_locale_then_english() {
        assert_eq!(pick_text("th", "en text", "th text"), "th text");
        // an empty translation never shows as an empty field
        assert_eq!(pick_text("th", "en text", ""), "en text");
        // English reads its own column, never the other locale's
        assert_eq!(pick_text("en", "en text", "th text"), "en text");
    }
}
