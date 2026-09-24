//! Detail pages. One handler across the kinds: they differ in which prose
//! fields they carry and whether they show effects or combo bonuses, which
//! the template decides from the section.

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db::{self, CombiRow, Detail, EffectLine, TreasureLinks};
use crate::grade::Graded;
use crate::richtext;
use crate::section::Section;
use crate::state::AppState;

use super::errors::AppError;

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
) -> Result<Response, AppError> {
    let Some(sec) = Section::parse(&section) else {
        return Ok(super::errors::not_found(ctx));
    };
    let item = db::select_detail(&state.db, &ctx.lang, &section, id).await?;
    let Some(item) = item else {
        return Ok(super::errors::not_found(ctx));
    };
    let effects = if sec == Section::Treasures {
        db::treasure_effects(&state.db, &ctx.lang, id).await?
    } else {
        Vec::new()
    };
    let combi = db::combi_bonuses(&state.db, &ctx.lang, &section, id).await?;
    let links = if sec == Section::Treasures {
        db::treasure_links(&state.db, &ctx.lang, id).await?
    } else {
        TreasureLinks::default()
    };
    // the reverse link: what this cookie or pet unlocks
    let unlocks = match sec {
        Section::Cookies | Section::Pets => {
            db::unlocked_treasure(&state.db, &ctx.lang, sec.table(), id).await?
        }
        _ => None,
    };
    // one link cache across all five prose fields: an entity named twice on
    // one page costs one lookup
    let mut memo = richtext::LinkCache::new();
    let abilities_html =
        richtext::render_with(&state.db, &ctx.lang, &item.abilities, &mut memo).await;
    let description_html =
        richtext::render_with(&state.db, &ctx.lang, &item.description, &mut memo).await;
    let power_plus_html =
        richtext::render_with(&state.db, &ctx.lang, &item.power_plus, &mut memo).await;
    let power_plus_requirement_html = richtext::render_with(
        &state.db,
        &ctx.lang,
        &item.power_plus_requirement,
        &mut memo,
    )
    .await;
    let unlock_goal_html =
        richtext::render_with(&state.db, &ctx.lang, &item.unlock_goal, &mut memo).await;

    let blessed_differs = db::blessed_differs(&effects);
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
        blessed_differs,
        combi,
        links,
        unlocks,
    };
    Ok(Html(
        page.render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}
