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
