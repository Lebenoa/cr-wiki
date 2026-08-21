//! Detail pages. One handler across the kinds: they differ in which prose
//! fields they carry and whether they show effects or combo bonuses, which
//! the template decides from the section.

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
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
    combi: Vec<CombiRow>,
    links: TreasureLinks,
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
        return (StatusCode::NOT_FOUND, "not found").into_response();
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
        // rich text needs the pool too, so it is rendered on this thread
        let abilities = richtext::render(&db, &lang, &item.abilities);
        let description = richtext::render(&db, &lang, &item.description);
        let power_plus = richtext::render(&db, &lang, &item.power_plus);
        let ppr = richtext::render(&db, &lang, &item.power_plus_requirement);
        let goal = richtext::render(&db, &lang, &item.unlock_goal);
        Ok(Some((item, effects, combi, links, abilities, description, power_plus, ppr, goal)))
    })
    .await;

    let Ok(Ok(Some((item, effects, combi, links, abilities_html, description_html, power_plus_html, power_plus_requirement_html, unlock_goal_html)))) = loaded
    else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    let page = DetailPage {
        ctx,
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
    };
    Html(page.render().unwrap_or_else(|e| format!("template error: {e}"))).into_response()
}
