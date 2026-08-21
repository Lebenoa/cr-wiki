//! The build planner's write side: submit, edit, delete and verify.

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::builds::{self, BuildCard};
use crate::ctx::Ctx;
use crate::prefill::Prefill;
use crate::state::AppState;

/// Anonymous submissions live 24 hours; a signed-in one is permanent.
const ANON_TTL_SECS: i64 = 24 * 60 * 60;

#[derive(Template)]
#[template(path = "build_form.html")]
struct PlannerPage {
    ctx: Ctx,
    /// filled when editing, empty when composing a new build
    build: Option<BuildCard>,
    /// the same loadout flattened, so the template reads one shape either way
    prefill: Prefill,
    error: String,
}

/// The planner posts hidden inputs for every slot, so the shape is the same
/// whether the pick came from a dialog or a prefilled edit.
#[derive(Debug, Default, Deserialize)]
pub struct BuildForm {
    #[serde(default)]
    pub cookie: i64,
    #[serde(default)]
    pub c2: i64,
    #[serde(default)]
    pub pet: i64,
    #[serde(default)]
    pub t1: i64,
    #[serde(default)]
    pub t2: i64,
    #[serde(default)]
    pub t3: i64,
    #[serde(default)]
    pub blessed1: i64,
    #[serde(default)]
    pub blessed2: i64,
    #[serde(default)]
    pub blessed3: i64,
    pub t1_level: Option<String>,
    pub t2_level: Option<String>,
    pub t3_level: Option<String>,
    pub ep: Option<String>,
    pub score: Option<String>,
    pub coin: Option<String>,
    pub time: Option<String>,
    pub boxes: Option<String>,
    pub description: Option<String>,
    pub youtube_url: Option<String>,
    pub author: Option<String>,
    #[serde(default)]
    pub tag_score: Option<String>,
    #[serde(default)]
    pub tag_coin: Option<String>,
    #[serde(default)]
    pub tag_autofarm: Option<String>,
    #[serde(rename = "cf-turnstile-response")]
    pub turnstile: Option<String>,
}

/// A blank level field means the treasure sits at max. An unparseable one
/// does too, rather than silently storing level 0 — which is a legitimate
/// level, so a naive parse would persist the wrong value.
fn level_field(raw: Option<&String>) -> i64 {
    match raw.map(|s| s.trim()) {
        None | Some("") => 9,
        Some(s) if s.bytes().all(|b| b.is_ascii_digit()) => s.parse::<i64>().unwrap_or(9).min(9),
        _ => 9,
    }
}

fn number(raw: Option<&String>) -> i64 {
    raw.and_then(|s| s.trim().parse::<i64>().ok()).filter(|n| *n >= 0).unwrap_or(0)
}

/// "1".."7" is a regular tier, "s1".."s3" a special one.
fn parse_ep(raw: &str) -> (i64, i64) {
    if let Some(rest) = raw.strip_prefix('s') {
        let n = rest.parse::<i64>().unwrap_or(0);
        return (0, if (1..=3).contains(&n) { n } else { 0 });
    }
    let n = raw.parse::<i64>().unwrap_or(0);
    (if (1..=7).contains(&n) { n } else { 0 }, 0)
}

fn tags_of(form: &BuildForm) -> Vec<String> {
    let mut out = Vec::new();
    for (present, name) in [
        (&form.tag_score, "score"),
        (&form.tag_coin, "coin"),
        (&form.tag_autofarm, "autofarm"),
    ] {
        if present.as_deref().is_some_and(|v| !v.is_empty()) {
            out.push(name.to_string());
        }
    }
    out
}

fn page(ctx: Ctx, build: Option<BuildCard>, error: &str) -> Response {
    let prefill = match build.as_ref() {
        Some(b) => Prefill::from_build(b),
        None => Prefill::blank(),
    };
    Html(
        PlannerPage { ctx, build, prefill, error: error.to_string() }
            .render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response()
}

pub async fn new_form(ctx: Ctx) -> Response {
    page(ctx, None, "")
}

/// The prefilled edit form. 404 rather than 403 for someone else's build,
/// matching the V routes: whether a build exists is not worth revealing.
pub async fn edit_form(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(id): Path<i64>,
) -> Response {
    let Some(build) = load_owned(&state, &ctx, id).await else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    page(ctx, Some(build), "")
}

pub async fn update(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(id): Path<i64>,
    Form(form): Form<BuildForm>,
) -> Response {
    let Some(existing) = load_owned(&state, &ctx, id).await else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let (ep, ep_special) = parse_ep(form.ep.as_deref().unwrap_or(""));
    let tags = tags_of(&form);
    if form.cookie <= 0 || form.pet <= 0 || form.t1 <= 0 || form.t2 <= 0 || form.t3 <= 0 {
        return (StatusCode::BAD_REQUEST, page(ctx, Some(existing), "build_error_loadout"))
            .into_response();
    }
    if (ep == 0 && ep_special == 0) || tags.is_empty() {
        return (StatusCode::BAD_REQUEST, page(ctx, Some(existing), "build_error_ep_tag"))
            .into_response();
    }
    let record = record_from(&form, ep, ep_special, tags, String::new(), 0, None);
    let db = state.db.clone();
    let _ = tokio::task::spawn_blocking(move || builds::update_build(&db, id, &record)).await;
    Redirect::to(&format!("/builds/{id}")).into_response()
}

/// Loads a build only when the caller may change it.
async fn load_owned(state: &AppState, ctx: &Ctx, id: i64) -> Option<BuildCard> {
    let lang = ctx.lang.clone();
    let db = state.db.clone();
    let found = tokio::task::spawn_blocking(move || builds::select_build(&db, &lang, id))
        .await
        .ok()?
        .ok()??;
    can_edit(ctx, &found).then_some(found)
}

pub async fn create(
    State(state): State<AppState>,
    ctx: Ctx,
    Form(form): Form<BuildForm>,
) -> Response {
    if !crate::turnstile::verify(&state.cfg, form.turnstile.as_deref(), "build").await {
        return (StatusCode::FORBIDDEN, page(ctx, None, "turnstile_form_failed")).into_response();
    }
    let (ep, ep_special) = parse_ep(form.ep.as_deref().unwrap_or(""));
    let tags = tags_of(&form);

    if form.cookie <= 0 || form.pet <= 0 || form.t1 <= 0 || form.t2 <= 0 || form.t3 <= 0 {
        return (StatusCode::BAD_REQUEST, page(ctx, None, "build_error_loadout")).into_response();
    }
    if (ep == 0 && ep_special == 0) || tags.is_empty() {
        return (StatusCode::BAD_REQUEST, page(ctx, None, "build_error_ep_tag")).into_response();
    }

    let user_id = ctx.user.as_ref().map(|u| u.id).unwrap_or(0);
    let author = match ctx.user.as_ref() {
        Some(u) => u.username.clone(),
        None => form.author.clone().unwrap_or_default(),
    };
    // an anonymous build expires; a signed-in one does not
    let expires_at = if user_id > 0 { None } else { Some(now_unix() + ANON_TTL_SECS) };

    let record = record_from(&form, ep, ep_special, tags, author, user_id, expires_at);

    let db = state.db.clone();
    let created = tokio::task::spawn_blocking(move || builds::insert_build(&db, &record))
        .await
        .unwrap_or_else(|_| Ok(0))
        .unwrap_or(0);

    if created <= 0 {
        return (StatusCode::INTERNAL_SERVER_ERROR, page(ctx, None, "build_error_save"))
            .into_response();
    }
    Redirect::to(&format!("/builds/{created}")).into_response()
}

pub async fn delete(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(id): Path<i64>,
) -> Response {
    let lang = ctx.lang.clone();
    let db = state.db.clone();
    let found = tokio::task::spawn_blocking(move || builds::select_build(&db, &lang, id))
        .await
        .unwrap_or_else(|_| Ok(None))
        .unwrap_or(None);

    let Some(build) = found else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    // 404 rather than 403 for someone else's build, matching the V routes:
    // whether a build exists is not worth revealing to a stranger
    if !can_edit(&ctx, &build) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let db = state.db.clone();
    let _ = tokio::task::spawn_blocking(move || builds::delete_build(&db, id)).await;
    Redirect::to("/builds").into_response()
}

/// The author or an admin. An anonymous build has no owner, so only an admin
/// can touch it.
fn can_edit(ctx: &Ctx, build: &BuildCard) -> bool {
    if ctx.is_admin() {
        return true;
    }
    match ctx.user.as_ref() {
        Some(u) => build.user_id > 0 && build.user_id == u.id,
        None => false,
    }
}

/// The shared field mapping, so create and update cannot drift apart.
fn record_from(
    form: &BuildForm,
    ep: i64,
    ep_special: i64,
    tags: Vec<String>,
    author: String,
    user_id: i64,
    expires_at: Option<i64>,
) -> builds::NewBuild {
    builds::NewBuild {
        cookie: form.cookie,
        cookie2: if form.c2 > 0 { Some(form.c2) } else { None },
        pet: form.pet,
        treasures: [form.t1, form.t2, form.t3],
        blessed: [form.blessed1 != 0, form.blessed2 != 0, form.blessed3 != 0],
        levels: [
            level_field(form.t1_level.as_ref()),
            level_field(form.t2_level.as_ref()),
            level_field(form.t3_level.as_ref()),
        ],
        ep,
        ep_special,
        tags: tags.join(","),
        score: number(form.score.as_ref()),
        coin: number(form.coin.as_ref()),
        // the form is in seconds, the column in milliseconds
        time_ms: number(form.time.as_ref()) * 1000,
        boxes: number(form.boxes.as_ref()),
        description: form.description.clone().unwrap_or_default(),
        youtube_url: form.youtube_url.clone().unwrap_or_default(),
        author,
        user_id,
        expires_at,
    }
}

#[derive(Debug, Deserialize)]
pub struct VerifyForm {
    /// "1" confirms the build works, anything else reports an issue
    pub verified: Option<String>,
    pub reason: Option<String>,
}

/// A signed-in visitor's verdict on someone's build. Anonymous visitors
/// cannot vote, so there is nothing to rate-limit per identity.
pub async fn verify(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(id): Path<i64>,
    Form(form): Form<VerifyForm>,
) -> Response {
    let Some(user) = ctx.user.as_ref().map(|u| u.id) else {
        return (StatusCode::FORBIDDEN, "sign in to verify").into_response();
    };
    let lang = ctx.lang.clone();
    let db = state.db.clone();
    let exists = tokio::task::spawn_blocking(move || builds::select_build(&db, &lang, id))
        .await
        .unwrap_or_else(|_| Ok(None))
        .unwrap_or(None);
    if exists.is_none() {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    let ok = form.verified.as_deref() == Some("1");
    let reason = form.reason.clone().unwrap_or_default();
    let db = state.db.clone();
    let _ = tokio::task::spawn_blocking(move || {
        builds::upsert_review(&db, id, user, ok, &reason)
    })
    .await;
    Redirect::to(&format!("/builds/{id}")).into_response()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
