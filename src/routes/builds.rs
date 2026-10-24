//! The community build list and one build's detail page.

use askama::Template;
use axum::extract::{Path, Query};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::builds::{self, BuildCard};
use crate::ctx::Ctx;

use super::{errors::AppError, CommonQuery};

/// The two read-only build pages. `/builds/new` and the write side belong to
/// `planner`, which is merged first so the static segment keeps winning over
/// the `{id}` capture here.
pub fn routes() -> Router {
    Router::new()
        .route("/builds", get(list))
        .route("/builds/{id}", get(show))
}

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
}

#[derive(Template)]
#[template(path = "build_detail.html")]
struct BuildDetail {
    ctx: Ctx,
    build: BuildCard,
    verified: i64,
    issues: i64,
    /// the description through the rich-text renderer: [[kind:id]] become
    /// entity links and {color:...} spans, everything else stays escaped —
    /// the same markup the form's autocomplete and preview produce
    description_html: String,
}

impl BuildDetail {
    fn title(&self) -> String {
        let episode = self.build.ep_label(&self.ctx);
        let subject = self
            .build
            .cookie
            .as_ref()
            .map(|c| c.name.as_str())
            .or_else(|| self.build.pet.as_ref().map(|p| p.name.as_str()))
            .map_or_else(
                || self.ctx.l.t("build_detail_subject_fallback"),
                str::to_string,
            );
        self.ctx
            .l
            .t("build_detail_meta_title")
            .replace("{episode}", &episode)
            .replace("{subject}", &subject)
            .replace("{id}", &self.build.id.to_string())
    }

    fn description(&self) -> String {
        let episode = self.build.ep_label(&self.ctx);
        let mut names = Vec::with_capacity(5);
        if let Some(cookie) = &self.build.cookie {
            names.push(cookie.name.as_str());
        }
        if let Some(cookie) = &self.build.cookie2 {
            names.push(cookie.name.as_str());
        }
        if let Some(pet) = &self.build.pet {
            names.push(pet.name.as_str());
        }
        names.extend(self.build.treasures.iter().map(|t| t.name.as_str()));
        self.ctx
            .l
            .t("build_detail_meta_description")
            .replace("{id}", &self.build.id.to_string())
            .replace("{loadout}", &names.join(", "))
            .replace("{episode}", &episode)
    }
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

pub async fn list(ctx: Ctx, q: Query<CommonQuery>) -> Result<Html<String>, AppError> {
    let state = crate::state::state();
    let lang = ctx.lang.clone();
    let page = crate::pagination::clamp_page(q.page);
    let offset = page
        .saturating_sub(1)
        .saturating_mul(crate::section::PAGE_SIZE);
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
        crate::section::PAGE_SIZE,
        offset,
    )
    .await?;

    let next_page = if i64::try_from(rows.len()).unwrap_or(0) == crate::section::PAGE_SIZE {
        page.saturating_add(1)
    } else {
        0
    };
    let html = if ctx.is_fragment() {
        BuildCards {
            ctx,
            builds: rows,
            next_page,
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

pub async fn show(ctx: Ctx, Path(id): Path<i64>) -> Result<Response, AppError> {
    let state = crate::state::state();
    let found = builds::select_build(&state.db, &ctx.lang, id).await?;

    let Some(build) = found else {
        return Ok(super::errors::not_found(ctx));
    };
    let (verified, issues) = builds::review_counts(&state.db, id).await?;
    let mut memo = crate::richtext::LinkCache::new();
    let description_html =
        crate::richtext::render_with(&state.db, &ctx.lang, &build.description, &mut memo, None)
            .await;
    Ok(Html(
        BuildDetail {
            ctx,
            build,
            verified,
            issues,
            description_html,
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}
