//! The site chrome's presentation data: navigation entries, themes, and the
//! label helpers the navbar and form templates print.
//!
//! This module is deliberately plain data and pure functions — no request, no
//! `Ctx` — so every list here is testable without a web server and has
//! exactly one home. `Ctx` keeps the per-request facts (lang, path, user) and
//! delegates to this module; a template never learns where the split sits.

use crate::i18n::Loc;
use crate::section::Section;

/// The sections behind the Wiki dropdown, in navbar order.
pub const NAV_SECTIONS: [Section; 8] = Section::ALL;

/// The theme names the picker offers. The palettes live in the `UnoCSS`
/// preflight; the dropdown only toggles the data-theme attribute.
pub const THEMES: [&str; 8] = [
    "default",
    "light",
    "tokyo_night",
    "cappuccino",
    "dracula",
    "nord",
    "gruvbox",
    "rose_pine",
];

/// The palette tokens the custom-theme editor may override. The `--on-*`
/// contrast partners are derived from the chosen colour rather than exposed,
/// so a theme cannot end up with unreadable text on a button.
pub const THEME_TOKENS: [&str; 12] = [
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
];

/// The regular EP tiers, for the filter combobox.
pub const EP_TIERS: [i64; 7] = [1, 2, 3, 4, 5, 6, 7];

/// The special EP tiers.
pub const EP_SPECIALS: [i64; 3] = [1, 2, 3];

/// A combo pairs a cookie with a pet, so the partner of one is the other.
// str equality is not a const trait yet, so this cannot be a const fn
#[allow(clippy::missing_const_for_fn)]
pub fn combi_partner_section(section: &str) -> &'static str {
    if section == "cookies" {
        "pets"
    } else {
        "cookies"
    }
}

/// `theme_token_*` keys use an underscore, since a hyphen is not valid in a
/// translation key.
pub fn theme_token_key(token: &str) -> String {
    format!("theme_token_{}", token.replace('-', "_"))
}

/// The per-theme .tr key.
pub fn theme_key(name: &str) -> String {
    format!("theme_{name}")
}

/// True when the path is `/section` or sits under it, so the navbar can mark
/// the active entry.
pub fn nav_section_active(path: &str, section: &str) -> bool {
    let p = path.trim_start_matches('/');
    p == section || p.starts_with(&format!("{section}/"))
}

/// The navbar entry's classes, active or not — the same two class lists
/// `nav_link` builds in `app.v`.
pub fn nav_class(path: &str, section: &str) -> &'static str {
    if nav_section_active(path, section) {
        "font-semibold text-center text-primary border-b-2 border-primary pb-1 transition-all duration-1000"
    } else {
        "font-medium text-center text-foreground-muted hover:text-primary transition-all duration-1000"
    }
}

/// True when the current page sits under any wiki section, so the dropdown
/// trigger can carry the active styling its entries would.
pub fn wiki_active(path: &str) -> bool {
    NAV_SECTIONS
        .iter()
        .any(|s| nav_section_active(path, s.as_str()))
}

/// The Wiki dropdown trigger's classes, active or not.
pub fn wiki_class(path: &str) -> &'static str {
    if wiki_active(path) {
        "font-semibold text-center text-primary border-b-2 border-primary pb-1 transition-all duration-1000"
    } else {
        "font-medium text-center text-foreground-muted hover:text-primary transition-all duration-1000"
    }
}

/// A page title with the site name appended. Older .tr values bake the
/// suffix in and newer ones do not, so it is added here when missing rather
/// than leaving half the sections unbranded.
pub fn page_title(l: &Loc, key: &str) -> String {
    title_of(&l.t(key), l)
}

/// The same, for a title built from an entity name rather than a key.
// the {name} placeholder is consumed by replace(), not a formatting macro
#[allow(clippy::literal_string_with_formatting_args)]
pub fn entity_title(l: &Loc, name: &str) -> String {
    title_of(&l.t("entity_detail_title").replace("{name}", name), l)
}

fn title_of(title: &str, l: &Loc) -> String {
    let suffix = l.t("site_title_suffix");
    if suffix.is_empty() || title.contains(&suffix) {
        return title.to_string();
    }
    format!("{title} | {suffix}")
}

/// A page's own description. A key with no string behind it falls back to
/// the site one rather than printing the key.
pub fn page_desc(l: &Loc, key: &str) -> String {
    let text = l.t(key);
    if text == key || text.is_empty() {
        return l.t("site_description");
    }
    text
}

/// A detail page's description. Only some kinds have a template of their
/// own; the rest read the site line.
// the {name} placeholder is consumed by replace(), not a formatting macro
#[allow(clippy::literal_string_with_formatting_args)]
pub fn entity_desc(l: &Loc, section: &str, name: &str) -> String {
    let key = match section {
        "cookies" | "pets" | "treasures" => "entity_detail_description",
        "episodes" => "episode_detail_description",
        "ingredients" => "ingredient_detail_description",
        "jellies" => "jelly_detail_description",
        _ => return l.t("site_description"),
    };
    page_desc(l, key).replace("{name}", name)
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    #[test]
    fn nav_activation_tracks_the_path() {
        assert!(nav_section_active("/cookies", "cookies"));
        assert!(nav_section_active("/cookies/89", "cookies"));
        assert!(!nav_section_active("/cookiesaurus", "cookies"));
        assert!(!nav_section_active("/pets", "cookies"));
        assert!(wiki_active("/treasures"));
        assert!(!wiki_active("/builds"));
        assert!(!wiki_active("/"));
    }

    #[test]
    fn active_nav_classes_match_between_entries_and_trigger() {
        let active = "font-semibold text-center text-primary border-b-2 border-primary pb-1 transition-all duration-1000";
        assert_eq!(nav_class("/cookies", "cookies"), active);
        assert_eq!(wiki_class("/cookies"), active);
        assert_ne!(nav_class("/builds", "cookies"), active);
    }

    #[test]
    fn theme_keys_use_underscores() {
        assert_eq!(
            theme_token_key("foreground-muted"),
            "theme_token_foreground_muted"
        );
        assert_eq!(theme_key("tokyo_night"), "theme_tokyo_night");
    }

    #[test]
    fn combi_partner_flips_the_pair() {
        assert_eq!(combi_partner_section("cookies"), "pets");
        assert_eq!(combi_partner_section("pets"), "cookies");
    }

    #[test]
    fn titles_carry_the_site_suffix_once() {
        crate::i18n::load("translations");
        let l = Loc::new("en");
        let title = page_title(&l, "changelog_page_header");
        assert!(title.contains("Changelog"));
        // double-suffixing is the failure mode the contains() check guards
        assert!(title.matches('|').count() <= 1);
    }
}
