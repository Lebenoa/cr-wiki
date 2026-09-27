//! The build planner's write side: submit, edit, delete and verify.

use askama::Template;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::builds::{self, BuildCard};
use crate::ctx::Ctx;
use crate::prefill::Prefill;
use crate::state::AppState;
use crate::time::now_unix;

use super::errors::AppError;

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
    raw.and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .unwrap_or(0)
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
    let prefill = build
        .as_ref()
        .map_or_else(Prefill::blank, Prefill::from_build);
    Html(
        PlannerPage {
            ctx,
            build,
            prefill,
            error: error.to_string(),
        }
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
    ctx: Ctx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(build) = load_owned(state, &ctx, id).await? else {
        return Ok(super::errors::not_found(ctx));
    };
    Ok(page(ctx, Some(build), ""))
}

pub async fn update(
    ctx: Ctx,
    Path(id): Path<i64>,
    Form(form): Form<BuildForm>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(existing) = load_owned(state, &ctx, id).await? else {
        return Ok(super::errors::not_found(ctx));
    };
    let (ep, ep_special) = parse_ep(form.ep.as_deref().unwrap_or(""));
    let tags = tags_of(&form);
    if form.cookie <= 0 || form.pet <= 0 || form.t1 <= 0 || form.t2 <= 0 || form.t3 <= 0 {
        return Ok((
            StatusCode::BAD_REQUEST,
            page(ctx, Some(existing), "build_error_loadout"),
        )
            .into_response());
    }
    if (ep == 0 && ep_special == 0) || tags.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            page(ctx, Some(existing), "build_error_ep_tag"),
        )
            .into_response());
    }
    let record = match record_from(&form, ep, ep_special, tags, String::new(), 0, None) {
        Ok(r) => r,
        Err(key) => {
            return Ok((StatusCode::BAD_REQUEST, page(ctx, Some(existing), key)).into_response())
        }
    };
    builds::update_build(&state.db, id, &record).await?;
    Ok(Redirect::to(&format!("/builds/{id}")).into_response())
}

/// Loads a build only when the caller may change it.
async fn load_owned(state: &AppState, ctx: &Ctx, id: i64) -> Result<Option<BuildCard>, AppError> {
    let found = builds::select_build(&state.db, &ctx.lang, id).await?;
    let Some(found) = found else {
        return Ok(None);
    };
    Ok(can_edit(ctx, &found).then_some(found))
}

pub async fn create(
    ctx: Ctx,
    Form(form): Form<BuildForm>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    if !crate::turnstile::verify(&state.cfg, form.turnstile.as_deref(), "build").await {
        return Ok((
            StatusCode::FORBIDDEN,
            page(ctx, None, "turnstile_form_failed"),
        )
            .into_response());
    }
    let (ep, ep_special) = parse_ep(form.ep.as_deref().unwrap_or(""));
    let tags = tags_of(&form);

    if form.cookie <= 0 || form.pet <= 0 || form.t1 <= 0 || form.t2 <= 0 || form.t3 <= 0 {
        return Ok((
            StatusCode::BAD_REQUEST,
            page(ctx, None, "build_error_loadout"),
        )
            .into_response());
    }
    if (ep == 0 && ep_special == 0) || tags.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            page(ctx, None, "build_error_ep_tag"),
        )
            .into_response());
    }

    let user_id = ctx.user.as_ref().map_or(0, |u| u.id);
    let author = ctx.user.as_ref().map_or_else(
        || form.author.clone().unwrap_or_default(),
        |u| u.username.clone(),
    );
    // an anonymous build expires; a signed-in one does not
    let expires_at = if user_id > 0 {
        None
    } else {
        Some(now_unix().saturating_add(ANON_TTL_SECS))
    };

    let record = match record_from(&form, ep, ep_special, tags, author, user_id, expires_at) {
        Ok(r) => r,
        Err(key) => return Ok((StatusCode::BAD_REQUEST, page(ctx, None, key)).into_response()),
    };

    // a failed save keeps the filled-in form and says so; the cause itself
    // reaches the log, which a bare Ok(0) never did
    let created = match builds::insert_build(&state.db, &record).await {
        Ok(0) => {
            return Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                page(ctx, None, "build_error_save"),
            )
                .into_response())
        }
        Ok(id) => id,
        Err(e) => {
            tracing::error!("insert build failed: {e}");
            return Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                page(ctx, None, "build_error_save"),
            )
                .into_response());
        }
    };
    Ok(Redirect::to(&format!("/builds/{created}")).into_response())
}

pub async fn delete(
    ctx: Ctx,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let found = builds::select_build(&state.db, &ctx.lang, id).await?;

    let Some(build) = found else {
        return Ok(super::errors::not_found(ctx));
    };
    // 404 rather than 403 for someone else's build, matching the V routes:
    // whether a build exists is not worth revealing to a stranger
    if !can_edit(&ctx, &build) {
        return Ok(super::errors::not_found(ctx));
    }
    builds::delete_build(&state.db, id).await?;
    Ok(Redirect::to("/builds").into_response())
}

/// The author or an admin. An anonymous build has no owner, so only an admin
/// can touch it.
fn can_edit(ctx: &Ctx, build: &BuildCard) -> bool {
    ctx.is_admin()
        || ctx
            .user
            .as_ref()
            .is_some_and(|u| build.user_id > 0 && build.user_id == u.id)
}

/// http(s) only: the value renders as a live href on the detail page and the
/// list cards, and HTML escaping does not neutralise a `javascript:` target
/// that carries no markup characters. Submission is anonymous, so the scheme
/// check is the only thing standing between a visitor and stored XSS.
fn validated_youtube_url(raw: Option<&String>) -> Result<String, &'static str> {
    let u = raw.map_or("", |s| s.trim());
    if u.is_empty() {
        return Ok(String::new());
    }
    if u.len() > 200 {
        return Err("build_error_video_link");
    }
    let lower = u.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("build_error_video_link");
    }
    if u.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err("build_error_video_link");
    }
    Ok(u.to_string())
}

fn validated_description(raw: Option<&String>) -> Result<String, &'static str> {
    let d = raw.map_or("", |s| s.trim());
    if d.chars().count() > 5000 {
        return Err("build_error_description");
    }
    Ok(d.to_string())
}

/// The shared field mapping, so create and update cannot drift apart.
#[allow(clippy::needless_pass_by_value)] // tags arrives pre-assembled for both callers
fn record_from(
    form: &BuildForm,
    ep: i64,
    ep_special: i64,
    tags: Vec<String>,
    author: String,
    user_id: i64,
    expires_at: Option<i64>,
) -> Result<builds::NewBuild, &'static str> {
    let description = validated_description(form.description.as_ref())?;
    let youtube_url = validated_youtube_url(form.youtube_url.as_ref())?;
    Ok(builds::NewBuild {
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
        time_ms: number(form.time.as_ref()).saturating_mul(1000),
        boxes: number(form.boxes.as_ref()),
        description,
        youtube_url,
        author,
        user_id,
        expires_at,
    })
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
    ctx: Ctx,
    Path(id): Path<i64>,
    Form(form): Form<VerifyForm>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(user) = ctx.user.as_ref().map(|u| u.id) else {
        return Ok((StatusCode::FORBIDDEN, "sign in to verify").into_response());
    };
    let exists = builds::select_build(&state.db, &ctx.lang, id).await?;
    if exists.is_none() {
        return Ok(super::errors::not_found(ctx));
    }

    let ok = form.verified.as_deref() == Some("1");
    let reason = form.reason.clone().unwrap_or_default();
    builds::upsert_review(&state.db, id, user, ok, &reason).await?;
    Ok(Redirect::to(&format!("/builds/{id}")).into_response())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// The build list runs, and its labels format the way the badges expect.
    /// The table is empty on this checkout, so this covers the query path and
    /// the formatting rather than row content.
    #[tokio::test]
    async fn build_queries_and_labels() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        for sort in ["latest", "verified", "score", "coin", "time", "nonsense"] {
            let rows = builds::select_builds(&pool, "en", (0, 0, 0, 0, 0), sort, "", 30, 0)
                .await
                .unwrap_or_else(|e| panic!("{sort}: {e}"));
            assert!(rows.len() <= 30);
        }
        // filters compose without tripping the SQL
        assert!(
            (builds::select_builds(&pool, "en", (89, 50, 317, 5, 0), "score", "", 30, 0))
                .await
                .is_ok()
        );
        assert!(
            (builds::select_builds(&pool, "en", (0, 0, 0, 0, 2), "latest", "", 30, 0))
                .await
                .is_ok()
        );
        // the author filter is bound, not inline, and empty means no filter
        assert!(
            (builds::select_builds(&pool, "en", (0, 0, 0, 0, 0), "latest", "alice", 30, 0))
                .await
                .is_ok()
        );
        assert!(builds::select_build(&pool, "en", 999_999)
            .await
            .unwrap()
            .is_none());

        crate::i18n::load("translations");
        let c = crate::testutil::test_ctx("en");
        let mut b = builds::BuildCard {
            ep: 5,
            ..Default::default()
        };
        assert_eq!(b.ep_label(&c), "EP 5");
        b.ep_special = 2;
        assert_eq!(
            b.ep_label(&c),
            "Special EP 2",
            "a special tier wins over the plain one"
        );
        b.time_ms = 95_400;
        assert_eq!(b.time_label(), "1:35");
        b.time_ms = 0;
        assert_eq!(b.time_label(), "");
        assert!(!b.has_stats());
        b.score = 10;
        assert!(b.has_stats());
    }

    /// "1".."7" is a regular tier, "s1".."s3" a special one, junk is 0 —
    /// the form's only EP gate, so an off-by-one here would store nonsense.
    #[test]
    fn parse_ep_splits_regular_from_special() {
        assert_eq!(parse_ep("3"), (3, 0));
        assert_eq!(parse_ep("7"), (7, 0));
        assert_eq!(parse_ep("s2"), (0, 2));
        assert_eq!(parse_ep("8"), (0, 0));
        assert_eq!(parse_ep("s4"), (0, 0));
        assert_eq!(parse_ep(""), (0, 0));
    }

    /// Blank means max level: an unparseable string must not silently store
    /// level 0, which is a legitimate level and would read as wrong data.
    #[test]
    fn level_field_defaults_to_max() {
        assert_eq!(level_field(None), 9);
        assert_eq!(level_field(Some(&String::new())), 9);
        assert_eq!(level_field(Some(&"4".to_string())), 4);
        assert_eq!(level_field(Some(&"12".to_string())), 9);
        assert_eq!(level_field(Some(&"abc".to_string())), 9);
    }

    /// The DB handle behind the gated tests: unset means they skip, so
    /// `cargo test` stays green without a server. Points at a scratch
    /// namespace/database — never at data you cannot lose.
    async fn live_db() -> Option<crate::db::Db> {
        let url = std::env::var("CR_SURREAL_URL").ok()?;
        let ns = std::env::var("CR_SURREAL_NS").unwrap_or_else(|_| "cookierun".into());
        let database = std::env::var("CR_SURREAL_DB").unwrap_or_else(|_| "cookierun".into());
        let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
        crate::db::connect_url(&url, &ns, &database, &user, &pass)
            .await
            .ok()
    }
}
