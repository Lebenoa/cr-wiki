//! The shapes the rest of the crate names when it talks about the catalog:
//! one card, one detail row, one section kind, and the locale-fallback SQL
//! fragment every translated column reads through. Kept in one place so the
//! read families cannot each grow their own copy.

use surrealdb::types::SurrealValue;

use crate::grade;
use crate::section::Section;

/// One catalog card: what the grid needs, with the English name alongside so
/// the filter matches across languages.
#[derive(Debug, Clone, Default)]
pub struct Card {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub is_evolved: bool,
}

impl grade::Graded for Card {
    fn grade(&self) -> Option<i64> {
        self.grade
    }
}

#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
pub(super) struct CardRow {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) en_name: String,
    pub(super) image: Option<String>,
    pub(super) grade: Option<i64>,
    pub(super) is_evolved: bool,
    pub(super) score: Option<f64>,
}

impl From<CardRow> for Card {
    fn from(r: CardRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            en_name: r.en_name,
            image: r.image,
            grade: r.grade,
            is_evolved: r.is_evolved,
        }
    }
}

/// A detail page's fields. Which are filled depends on the kind; each
/// template reads only the ones its kind has.
#[derive(Debug, Clone, Default)]
pub struct Detail {
    #[allow(dead_code)]
    pub id: i64,
    pub name: String,
    /// the share card uses it once the OG tags land
    #[allow(dead_code)]
    pub en_name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    /// treasures only: the evolved row links to its base, the base row to
    /// its evolution
    pub is_evolved: bool,
    /// treasures only: the row describes the Power+ variant
    pub is_power_plus: bool,
    pub abilities: String,
    pub description: String,
    pub power_plus: String,
    pub power_plus_requirement: String,
    pub unlock_goal: String,
    /// unix seconds; the page formats it client-side in the viewer's locale.
    /// Zero for a kind whose table has no such column, and for a row that
    /// never got a date.
    pub release_date: i64,
}

impl grade::Graded for Detail {
    fn grade(&self) -> Option<i64> {
        self.grade
    }
}

impl Detail {
    /// The release date as the edit form's `<input type=date>` wants it:
    /// YYYY-MM-DD, or empty when the row never got a date.
    // The Hinnant civil-from-days arithmetic is provably in-range: the input
    // is a unix seconds value divided by 86400, so every intermediate stays
    // far inside i64.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn release_date_input(&self) -> String {
        if self.release_date <= 0 {
            return String::new();
        }
        let days = self.release_date.div_euclid(86400);
        // inverse days-from-civil (Howard Hinnant's algorithm)
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}-{m:02}-{d:02}")
    }
}

/// Which table a catalog section reads, plus whether it grades its rows and
/// stamps release dates. Every list/detail/search query branches through
/// this instead of carrying per-section SQL.
pub struct Kind {
    pub table: &'static str,
    pub graded: bool,
    pub dated: bool,
}

impl Kind {
    /// The facts one section's queries need, read from the one place each
    /// section's table and flags live (`Section`).
    pub const fn of(section: Section) -> Self {
        Self {
            table: section.table(),
            graded: section.graded(),
            dated: section.dated(),
        }
    }

    /// The typed path's other half: an untyped `&str` (a path capture, a
    /// build row's column, a caller that has not parsed yet) resolves here.
    pub fn of_section(section: &str) -> Option<Self> {
        Section::parse(section).map(Self::of)
    }
}

/// The locale fallback every translated column reads through, as a SQL
/// fragment: the requested locale first, English behind it. Written once so
/// the fallback rule cannot drift between queries — it used to appear 23
/// times across two files. `pub` inside this private module, so the picker
/// lists read the same tables through the same rule.
pub fn tr(field: &str) -> String {
    format!("(tr[$lang].{field} ?? tr.en.{field} ?? '')")
}

/// Table names for one catalog section, for the callers that write.
/// `pub(super)` because the editor lives in a sibling module: the vocabulary
/// stays here rather than being copied into the write path.
pub(super) fn entity_table(section: Section) -> Option<&'static str> {
    // only the sections the admin editor writes
    section.editable().then_some(section.table())
}
#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use surrealdb::types::{Object, Value};

    /// The regression a lint pass once hid: a row struct whose field names
    /// drift from the projected columns decodes every row as `None` — no
    /// error, no missing-field complaint, just silently absent data. This
    /// builds the exact object the query projects and decodes it the way
    /// `take` does, with no database behind it.
    #[test]
    fn card_row_reads_the_card_projection() {
        let mut o = Object::new();
        o.insert("id", 12i64);
        o.insert("name", "Choco");
        o.insert("en_name", "Choco");
        o.insert("grade", 4i64);
        o.insert("is_evolved", false);
        let row = CardRow::from_value(Value::Object(o)).expect("decodes");
        assert_eq!(row.id, 12);
        assert_eq!(row.grade, Some(4));
        assert!(row.image.is_none());
        assert!(!row.is_evolved);
    }

    #[test]
    fn kind_facts_come_from_one_table() {
        // the triplication the lint pass left behind: `Kind::of` and the old
        // hand-written fallbacks must agree, or a fallback queries a wrong
        // table in silence
        let kind = Kind::of(Section::Cookies);
        assert_eq!(kind.table, "cookie");
        assert!(kind.graded);
        assert!(kind.dated);
        assert_eq!(Kind::of_section("cookies").map(|k| k.table), Some("cookie"));
        assert!(Kind::of_section("no-such-section").is_none());
        assert!(Section::parse("skins").is_some());
        assert_eq!(Section::Skins.table(), "skin");
        assert!(Section::Ingredients.graded());
        assert!(!Section::Ingredients.dated());
    }
}
