//! Detail pages. One handler across the kinds: they differ in which prose
//! fields they carry and whether they show effects or combo bonuses, which
//! the template decides from the section.

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db::{self, CombiRow, Detail, EffectLine, TreasureLinks};
use crate::richtext;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "detail.html")]
struct DetailPage {
    ctx: Ctx,
    section: String,
    item: Detail,
    /// prose already rendered through the rich-text markup
    abilities_html: String,
    description_html: String,
    power_plus_html: String,
    power_plus_requirement_html: String,
    unlock_goal_html: String,
    effects: Vec<EffectLine>,
    /// true when the blessed set is worth a toggle, i.e. it exists and is
    /// not identical to the normal one
    blessed_differs: bool,
    combi: Vec<CombiRow>,
    links: TreasureLinks,
    /// the treasure this cookie or pet unlocks: id, name, image
    unlocks: Option<(i64, String, Option<String>)>,
}

impl DetailPage {
    /// The section's own edit route, shown to an admin only.
    fn can_edit(&self) -> bool {
        self.ctx.is_admin()
    }
}

pub async fn show(
    State(state): State<AppState>,
    ctx: Ctx,
    Path((section, id)): Path<(String, i64)>,
) -> Response {
    if !matches!(
        section.as_str(),
        "cookies" | "pets" | "treasures" | "episodes" | "ingredients" | "jellies" | "relics"
            | "skins"
    ) {
        return super::errors::not_found(ctx);
    }
    let lang = ctx.lang.clone();
    let db = state.db.clone();
    let section_for_db = section.clone();

    let loaded = tokio::task::spawn_blocking(move || {
        let item = db::select_detail(&db, &lang, &section_for_db, id)?;
        let Some(item) = item else {
            return Ok::<_, rusqlite::Error>(None);
        };
        let effects = if section_for_db == "treasures" {
            db::treasure_effects(&db, &lang, id)?
        } else {
            Vec::new()
        };
        let combi = db::combi_bonuses(&db, &lang, &section_for_db, id)?;
        let links = if section_for_db == "treasures" {
            db::treasure_links(&db, &lang, id)?
        } else {
            TreasureLinks::default()
        };
        // the reverse link: what this cookie or pet unlocks
        let unlocks = match section_for_db.as_str() {
            "cookies" => db::unlocked_treasure(&db, &lang, "cookie", id)?,
            "pets" => db::unlocked_treasure(&db, &lang, "pet", id)?,
            _ => None,
        };
        // rich text needs the pool too, so it is rendered on this thread
        let abilities = richtext::render(&db, &lang, &item.abilities);
        let description = richtext::render(&db, &lang, &item.description);
        let power_plus = richtext::render(&db, &lang, &item.power_plus);
        let ppr = richtext::render(&db, &lang, &item.power_plus_requirement);
        let goal = richtext::render(&db, &lang, &item.unlock_goal);
        Ok(Some((item, effects, combi, links, unlocks, abilities, description, power_plus, ppr, goal)))
    })
    .await;

    let Ok(Ok(Some((item, effects, combi, links, unlocks, abilities_html, description_html, power_plus_html, power_plus_requirement_html, unlock_goal_html)))) = loaded
    else {
        return super::errors::not_found(ctx);
    };

    let blessed_differs = db::blessed_differs(&effects);
    let page = DetailPage {
        ctx,
        blessed_differs,
        section,
        item,
        abilities_html,
        description_html,
        power_plus_html,
        power_plus_requirement_html,
        unlock_goal_html,
        effects,
        combi,
        links,
        unlocks,
    };
    Html(page.render().unwrap_or_else(|e| format!("template error: {e}"))).into_response()
}
