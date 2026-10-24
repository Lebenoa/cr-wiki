//! Detail pages. One handler across the kinds: they differ in which prose
//! fields they carry and whether they show effects or combo bonuses, which
//! the template decides from the section.
//!
//! The page is the join of three concerns, one file each: the row this file
//! fetches, the ten side collections (`load`) and the five prose fields
//! (`prose`).

use askama::Template;
use axum::extract::Path;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::ctx::Ctx;
use crate::db::{self, CombiRow, Detail, EffectLine, TreasureLinks};
use crate::grade::Graded;
use crate::section::Section;

use super::errors::AppError;

mod load;
mod prose;

/// The `/{section}/{id}` capture, last of the catalog-shaped routes.
pub fn routes() -> Router {
    Router::new()
        .route("/{section}/{id}", get(show))
        .route_layer(axum::middleware::from_fn(super::catalog_cache::serve))
}

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
    /// an ingredient's drop/economy tile fields
    ingredient: db::IngredientFacts,
    /// the treasures this ingredient crafts
    recipes: Vec<db::CraftRecipe>,
    /// the ingredients this treasure is crafted from
    craft: Vec<db::CraftIngredient>,
    /// the linked variant: the base for an evolved row, the evolved form for
    /// a normal one
    variant: Option<(i64, String, Option<String>)>,
    /// an episode's child sections; empty for every other kind
    episode: db::EpisodeDetail,
    /// a jelly's producers; empty for every other kind
    makers: Vec<db::JellyMaker>,
}

impl DetailPage {
    /// The section for template expressions, which read `self.` fields.
    fn section(&self) -> &str {
        &self.section
    }

    /// The section's own edit route, shown to an admin only.
    fn can_edit(&self) -> bool {
        self.ctx.is_admin()
    }
}

pub async fn show(
    ctx: Ctx,
    Path((section, id)): Path<(String, i64)>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(sec) = Section::parse(&section) else {
        return Ok(super::errors::not_found(ctx));
    };
    let item = db::select_detail(&state.db, &ctx.lang, &section, id).await?;
    let Some(item) = item else {
        return Ok(super::errors::not_found(ctx));
    };
    let extras = load::extras(&state.db, &ctx.lang, sec, id, item.is_evolved).await?;
    let prose = prose::render(&state.db, &ctx.lang, sec, id, &item).await;
    let blessed_differs = db::blessed_differs(&extras.effects);
    let page = DetailPage {
        ctx,
        section,
        item,
        abilities_html: prose.abilities,
        description_html: prose.description,
        power_plus_html: prose.power_plus,
        power_plus_requirement_html: prose.power_plus_requirement,
        unlock_goal_html: prose.unlock_goal,
        effects: extras.effects,
        blessed_differs,
        combi: extras.combi,
        links: extras.links,
        unlocks: extras.unlocks,
        ingredient: extras.ingredient,
        recipes: extras.recipes,
        craft: extras.craft,
        variant: extras.variant,
        episode: extras.episode,
        makers: extras.makers,
    };
    match page.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(error) => {
            tracing::error!(%error, "detail template failed");
            Ok((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "template error",
            )
                .into_response())
        }
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests;
