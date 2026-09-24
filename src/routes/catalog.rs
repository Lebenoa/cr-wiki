//! The catalog list pages. Cookies, pets and treasures paginate 30 at a time
//! behind the htmx sentinel; the smaller catalogs render whole, as they do in
//! the V app.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db::{self, Card};
use crate::grade::Graded;
use crate::section::Section;
use crate::state::AppState;

use super::{errors::AppError, CommonQuery};

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
) -> Result<Response, AppError> {
    render(state, ctx, section, q).await
}

async fn render(
    state: AppState,
    ctx: Ctx,
    section: String,
    q: Query<CommonQuery>,
) -> Result<Response, AppError> {
    // the path segment is user input: anything unknown is a 404 rather than
    // an empty grid titled after itself
    let Some(sec) = Section::parse(&section) else {
        return Ok(super::errors::not_found(ctx));
    };
    let page = q.page.unwrap_or(1).max(1);
    let offset = page.saturating_sub(1).saturating_mul(PAGE_SIZE);
    let tab = match q.tab.as_deref() {
        Some(t @ ("normal" | "evo")) => t.to_string(),
        _ => "all".to_string(),
    };

    let paginated = sec.paginated();
    let cards = match sec {
        Section::Cookies => db::select_cookies(&state.db, &ctx.lang, PAGE_SIZE, offset).await?,
        Section::Pets => db::select_pets(&state.db, &ctx.lang, PAGE_SIZE, offset).await?,
        Section::Treasures => {
            db::select_treasures(&state.db, &ctx.lang, &tab, PAGE_SIZE, offset).await?
        }
        other => db::select_simple(&state.db, &ctx.lang, other.as_str()).await?,
    };

    let next_page = if paginated && i64::try_from(cards.len()).unwrap_or(0) == PAGE_SIZE {
        page.saturating_add(1)
    } else {
        0
    };

    let html = if ctx.is_fragment() {
        CatalogCards {
            ctx,
            section,
            cards,
            next_page,
            tab,
        }
        .render()
    } else {
        let title_key = format!("{section}_page_title");
        let desc_key = format!("{section}_page_description");
        CatalogPage {
            ctx,
            tabbed: sec == Section::Treasures,
            section,
            title_key,
            desc_key,
            cards,
            next_page,
            tab,
        }
        .render()
    };
    Ok(Html(html.unwrap_or_else(|e| format!("template error: {e}"))).into_response())
}
