//! The catalog list pages. Cookies, pets and treasures paginate 30 at a time
//! behind the htmx sentinel; the smaller catalogs render whole, as they do in
//! the V app.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db::{self, Card};
use crate::state::AppState;

use super::CommonQuery;

pub const PAGE_SIZE: i64 = 30;

#[derive(Template)]
#[template(path = "catalog.html")]
struct CatalogPage {
    ctx: Ctx,
    section: String,
    /// the .tr keys are named for the section, plural
    title_key: String,
    desc_key: String,
    cards: Vec<Card>,
    next_page: i64,
    tab: String,
    tabbed: bool,
}

#[derive(Template)]
#[template(path = "catalog_cards.html")]
struct CatalogCards {
    ctx: Ctx,
    section: String,
    cards: Vec<Card>,
    next_page: i64,
    tab: String,
}

/// One handler for every list: the section comes off the path, which keeps
/// the eight pages from being eight near-identical functions.
pub async fn list(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(section): Path<String>,
    q: Query<CommonQuery>,
) -> Response {
    render(state, ctx, section, q).await
}

/// The sections that have a list page. Anything else is a 404 rather than an
/// empty grid: the path segment is user input, and every unknown word would
/// otherwise render a page titled after itself.
fn known(section: &str) -> bool {
    matches!(
        section,
        "cookies" | "pets" | "treasures" | "episodes" | "ingredients" | "jellies" | "relics"
            | "skins"
    )
}

async fn render(
    state: AppState,
    ctx: Ctx,
    section: String,
    q: Query<CommonQuery>,
) -> Response {
    if !known(&section) {
        return super::errors::not_found(ctx);
    }
    let page = q.page.unwrap_or(1).max(1);
    let offset = page.saturating_sub(1).saturating_mul(PAGE_SIZE);
    let tab = match q.tab.as_deref() {
        Some(t @ ("normal" | "evo")) => t.to_string(),
        _ => "all".to_string(),
    };

    let paginated = matches!(section.as_str(), "cookies" | "pets" | "treasures");
    let cards = match section.as_str() {
        "cookies" => db::select_cookies(&state.db, &ctx.lang, PAGE_SIZE, offset).await,
        "pets" => db::select_pets(&state.db, &ctx.lang, PAGE_SIZE, offset).await,
        "treasures" => db::select_treasures(&state.db, &ctx.lang, &tab, PAGE_SIZE, offset).await,
        other => db::select_simple(&state.db, &ctx.lang, other).await,
    }
    .unwrap_or_default();

    let next_page = if paginated && i64::try_from(cards.len()).unwrap_or(0) == PAGE_SIZE {
        page.saturating_add(1)
    } else {
        0
    };

    let html = if ctx.is_fragment() {
        CatalogCards { ctx, section, cards, next_page, tab }.render()
    } else {
        let title_key = format!("{section}_page_title");
        let desc_key = format!("{section}_page_description");
        CatalogPage {
            ctx,
            tabbed: section == "treasures",
            section,
            title_key,
            desc_key,
            cards,
            next_page,
            tab,
        }
        .render()
    };
    Html(html.unwrap_or_else(|e| format!("template error: {e}"))).into_response()
}

