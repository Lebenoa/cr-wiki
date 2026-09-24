//! The eight catalog sections as a type.
//!
//! Every list, detail, upload and admin gate agrees on what exists because
//! they all read this one table instead of restating their own match arms.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Cookies,
    Pets,
    Treasures,
    Episodes,
    Ingredients,
    Jellies,
    Relics,
    Skins,
}

impl Section {
    /// Every section, in catalog order.
    pub const ALL: [Section; 8] = [
        Section::Cookies,
        Section::Pets,
        Section::Treasures,
        Section::Episodes,
        Section::Ingredients,
        Section::Jellies,
        Section::Relics,
        Section::Skins,
    ];

    /// The path segment. An unknown word parses to `None` and the route
    /// answers 404 — the segment is user input.
    pub fn parse(s: &str) -> Option<Section> {
        Some(match s {
            "cookies" => Section::Cookies,
            "pets" => Section::Pets,
            "treasures" => Section::Treasures,
            "episodes" => Section::Episodes,
            "ingredients" => Section::Ingredients,
            "jellies" => Section::Jellies,
            "relics" => Section::Relics,
            "skins" => Section::Skins,
            _ => return None,
        })
    }

    /// The URL segment, which doubles as the image directory.
    pub const fn as_str(self) -> &'static str {
        match self {
            Section::Cookies => "cookies",
            Section::Pets => "pets",
            Section::Treasures => "treasures",
            Section::Episodes => "episodes",
            Section::Ingredients => "ingredients",
            Section::Jellies => "jellies",
            Section::Relics => "relics",
            Section::Skins => "skins",
        }
    }

    /// The singular slug the admin editor's .tr keys build from
    /// (`save_cookie_button`).
    pub const fn singular(self) -> &'static str {
        match self {
            Section::Cookies => "cookie",
            Section::Pets => "pet",
            Section::Treasures => "treasure",
            Section::Episodes => "episode",
            Section::Ingredients => "ingredient",
            Section::Jellies => "jelly",
            Section::Relics => "relic",
            Section::Skins => "skin",
        }
    }

    /// The table the section's records live in.
    pub const fn table(self) -> &'static str {
        match self {
            Section::Cookies => "cookie",
            Section::Pets => "pet",
            Section::Treasures => "treasure",
            Section::Episodes => "episode",
            Section::Ingredients => "ingredient",
            Section::Jellies => "jelly",
            Section::Relics => "relic",
            Section::Skins => "skin",
        }
    }

    /// Rows carry a grade and a maintained display rank column.
    pub const fn graded(self) -> bool {
        matches!(
            self,
            Section::Cookies
                | Section::Pets
                | Section::Treasures
                | Section::Ingredients
                | Section::Skins
        )
    }

    /// The table has a `release_date` column worth showing.
    pub const fn dated(self) -> bool {
        matches!(self, Section::Cookies | Section::Pets | Section::Treasures)
    }

    /// The list page paginates 30 at a time behind the htmx sentinel.
    pub const fn paginated(self) -> bool {
        matches!(self, Section::Cookies | Section::Pets | Section::Treasures)
    }

    /// The admin editor has forms for these sections.
    pub const fn editable(self) -> bool {
        matches!(self, Section::Cookies | Section::Pets | Section::Treasures)
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// parse/`as_str` round-trip, and the flag facts the queries lean on.
    #[test]
    fn section_facts_hold_for_every_variant() {
        for section in Section::ALL {
            assert_eq!(Section::parse(section.as_str()), Some(section));
        }
        assert!(Section::parse("builds").is_none());
        assert_eq!(Section::Cookies.singular(), "cookie");
        assert_eq!(Section::Skins.singular(), "skin");
        assert!(Section::Cookies.editable());
        assert!(!Section::Relics.editable());
        assert!(Section::Treasures.dated());
        assert!(!Section::Jellies.dated());
    }
}
