//! The community build list and one build's detail page.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};

use crate::builds::{self, BuildCard};
use crate::ctx::Ctx;
use crate::state::AppState;

use super::{errors::AppError, CommonQuery};

pub const PAGE_SIZE: i64 = 30;

#[derive(Template)]
#[template(path = "builds.html")]
struct BuildsPage {
    ctx: Ctx,
    builds: Vec<BuildCard>,
    next_page: i64,
    sort: String,
    filter_cookie: i64,
    filter_pet: i64,
    filter_treasure: i64,
    ep: String,
    /// free-text author filter, empty when unset
    author: String,
}

#[derive(Template)]
#[template(path = "build_cards.html")]
struct BuildCards {
    ctx: Ctx,
    builds: Vec<BuildCard>,
    next_page: i64,
    sort: String,
    filter_cookie: i64,
    filter_pet: i64,
    filter_treasure: i64,
    ep: String,
    /// free-text author filter, empty when unset
    author: String,
}

#[derive(Template)]
#[template(path = "build_detail.html")]
struct BuildDetail {
    ctx: Ctx,
    build: BuildCard,
    verified: i64,
    issues: i64,
}

/// The EP combobox value: `"1".."7"` for a regular tier, `"s1".."s3"` for a
/// special one. Returns `(ep, ep_special)`.
fn parse_ep(raw: &str) -> (i64, i64) {
    if let Some(rest) = raw.strip_prefix('s') {
        let n = rest.parse::<i64>().unwrap_or(0);
        return (0, if (1..=3).contains(&n) { n } else { 0 });
    }
    let n = raw.parse::<i64>().unwrap_or(0);
    (if (1..=7).contains(&n) { n } else { 0 }, 0)
}

pub async fn list(
    State(state): State<AppState>,
    ctx: Ctx,
    q: Query<CommonQuery>,
) -> Result<Html<String>, AppError> {
    let lang = ctx.lang.clone();
    let page = q.page.unwrap_or(1).max(1);
    let offset = page.saturating_sub(1).saturating_mul(PAGE_SIZE);
    let sort = match q.sort.as_deref() {
        Some(s @ ("score" | "coin" | "time" | "verified")) => s.to_string(),
        _ => "latest".to_string(),
    };
    let ep_raw = q.ep.clone().unwrap_or_default();
    let (ep, ep_special) = parse_ep(&ep_raw);
    let filter_cookie = q.cookie.unwrap_or(0);
    let filter_pet = q.pet.unwrap_or(0);
    let filter_treasure = q.treasure.unwrap_or(0);
    // bound as a parameter downstream; trimmed so a stray space cannot
    // silently filter everything out
    let author = q.author.clone().unwrap_or_default().trim().to_string();

    let rows = builds::select_builds(
        &state.db,
        &lang,
        (filter_cookie, filter_pet, filter_treasure, ep, ep_special),
        &sort,
        &author,
        PAGE_SIZE,
        offset,
    )
    .await?;

    let next_page = if i64::try_from(rows.len()).unwrap_or(0) == PAGE_SIZE {
        page.saturating_add(1)
    } else {
        0
    };
    let html = if ctx.is_fragment() {
        BuildCards {
            ctx,
            builds: rows,
            next_page,
            sort,
            filter_cookie,
            filter_pet,
            filter_treasure,
            ep: ep_raw,
            author,
        }
        .render()
    } else {
        BuildsPage {
            ctx,
            builds: rows,
            next_page,
            sort,
            filter_cookie,
            filter_pet,
            filter_treasure,
            ep: ep_raw,
            author,
        }
        .render()
    };
    Ok(Html(
        html.unwrap_or_else(|e| format!("template error: {e}")),
    ))
}

pub async fn show(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let found = builds::select_build(&state.db, &ctx.lang, id).await?;

    let Some(build) = found else {
        return Ok(super::errors::not_found(ctx));
    };
    let (verified, issues) = builds::review_counts(&state.db, id).await?;
    Ok(Html(
        BuildDetail {
            ctx,
            build,
            verified,
            issues,
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}
