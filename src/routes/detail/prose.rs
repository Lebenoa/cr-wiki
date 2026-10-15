//! The page's five prose fields, rendered through the rich-text markup.

use crate::db::Detail;
use crate::richtext;
use crate::section::Section;

/// The rendered prose, one field per prose column the detail row carries.
pub(super) struct Prose {
    pub abilities: String,
    pub description: String,
    pub power_plus: String,
    pub power_plus_requirement: String,
    pub unlock_goal: String,
}

/// One link cache across all five prose fields: an entity named twice on one
/// page costs one lookup. The page's own entity renders mentions as a
/// highlight, not a self-link.
pub(super) async fn render(
    db: &crate::db::Db,
    lang: &str,
    sec: Section,
    id: i64,
    item: &Detail,
) -> Prose {
    let self_ref: Option<(&'static str, i64)> = match sec {
        Section::Cookies => Some(("cookie", id)),
        Section::Pets => Some(("pet", id)),
        Section::Treasures => Some(("treasure", id)),
        _ => None,
    };
    let mut memo = richtext::LinkCache::new();
    let abilities = richtext::render_with(db, lang, &item.abilities, &mut memo, self_ref).await;
    let description = richtext::render_with(db, lang, &item.description, &mut memo, self_ref).await;
    let power_plus = richtext::render_with(db, lang, &item.power_plus, &mut memo, self_ref).await;
    let power_plus_requirement =
        richtext::render_with(db, lang, &item.power_plus_requirement, &mut memo, self_ref).await;
    let unlock_goal = richtext::render_with(db, lang, &item.unlock_goal, &mut memo, self_ref).await;
    Prose {
        abilities,
        description,
        power_plus,
        power_plus_requirement,
        unlock_goal,
    }
}
