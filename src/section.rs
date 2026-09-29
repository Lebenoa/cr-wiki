//! The eight catalog sections as a type.
//!
//! Every list, detail, upload and admin gate agrees on what exists because
//! they all read this one table instead of restating their own match arms.

/// Rows per page for every paginated grid — catalog lists, builds,
/// changelog, picker. One constant so the grids cannot drift apart.
pub const PAGE_SIZE: i64 = 30;

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
    pub const ALL: [Self; 8] = [
        Self::Cookies,
        Self::Pets,
        Self::Treasures,
        Self::Episodes,
        Self::Ingredients,
        Self::Jellies,
        Self::Relics,
        Self::Skins,
    ];

    /// The path segment. An unknown word parses to `None` and the route
    /// answers 404 — the segment is user input.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "cookies" => Self::Cookies,
            "pets" => Self::Pets,
            "treasures" => Self::Treasures,
            "episodes" => Self::Episodes,
            "ingredients" => Self::Ingredients,
            "jellies" => Self::Jellies,
            "relics" => Self::Relics,
            "skins" => Self::Skins,
            _ => return None,
        })
    }

    /// The URL segment, which doubles as the image directory.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cookies => "cookies",
            Self::Pets => "pets",
            Self::Treasures => "treasures",
            Self::Episodes => "episodes",
            Self::Ingredients => "ingredients",
            Self::Jellies => "jellies",
            Self::Relics => "relics",
            Self::Skins => "skins",
        }
    }

    /// The singular slug the admin editor's .tr keys build from
    /// (`save_cookie_button`).
    pub const fn singular(self) -> &'static str {
        match self {
            Self::Cookies => "cookie",
            Self::Pets => "pet",
            Self::Treasures => "treasure",
            Self::Episodes => "episode",
            Self::Ingredients => "ingredient",
            Self::Jellies => "jelly",
            Self::Relics => "relic",
            Self::Skins => "skin",
        }
    }

    /// The table the section's records live in.
    pub const fn table(self) -> &'static str {
        match self {
            Self::Cookies => "cookie",
            Self::Pets => "pet",
            Self::Treasures => "treasure",
            Self::Episodes => "episode",
            Self::Ingredients => "ingredient",
            Self::Jellies => "jelly",
            Self::Relics => "relic",
            Self::Skins => "skin",
        }
    }

    /// Rows carry a grade and a maintained display rank column.
    pub const fn graded(self) -> bool {
        matches!(
            self,
            Self::Cookies
                | Self::Pets
                | Self::Treasures
                | Self::Ingredients
                | Self::Skins
        )
    }

    /// The table has a `release_date` column worth showing.
    pub const fn dated(self) -> bool {
        matches!(self, Self::Cookies | Self::Pets | Self::Treasures)
    }

    /// The list page paginates 30 at a time behind the htmx sentinel.
    pub const fn paginated(self) -> bool {
        matches!(self, Self::Cookies | Self::Pets | Self::Treasures)
    }

    /// The admin editor has forms for these sections.
    pub const fn editable(self) -> bool {
        matches!(self, Self::Cookies | Self::Pets | Self::Treasures)
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
