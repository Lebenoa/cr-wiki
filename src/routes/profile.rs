//! `/profile/{id,me}` — the public, read-only profile page: who the account
//! is (initial avatar, username, joined day) and the builds it owns. The
//! account-management surface stays in `/settings`; nothing here writes.

use askama::Template;
use axum::extract::Path;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::builds::{self, BuildCard};
use crate::ctx::Ctx;
use crate::time::format_local_ymd;

use super::errors;

pub fn routes() -> Router {
    Router::new()
        // `me` before `{id}` so the static segment keeps winning over the
        // capture, the way every other static-before-capture table reads
        .route("/profile/me", get(me))
        .route("/profile/{id}", get(by_id))
}

/// The avatar circle's single-character initial, and a hue picked from the
/// username so two different accounts rarely share a colour. No avatar
/// storage: uploads are an admin-only catalog surface and a per-user image
/// path is deliberately out of scope for now.
pub fn initial(username: &str) -> String {
    username
        .chars()
        .next()
        .map(String::from)
        .unwrap_or_else(|| "?".to_string())
}

/// The joined-day line, localized to the site's fixed zone. `None` means the
/// row carries no usable timestamp (written before the column existed) and
/// the template hides the line rather than inventing a date.
pub fn joined_day(created_at: i64, tz_offset: i64) -> Option<String> {
    (created_at > 0).then(|| format_local_ymd(created_at, tz_offset))
}

#[derive(Template)]
#[template(path = "profile.html")]
struct ProfilePage {
    ctx: Ctx,
    user: ProfileUser,
    builds: Vec<BuildCard>,
    initial: String,
    joined: Option<String>,
}

#[derive(Debug, Clone)]
struct ProfileUser {
    username: String,
    google_linked: bool,
}

/// Signed in → own profile; otherwise the sign-in page, because the link
/// that leads here only exists for signed-in visitors anyway.
async fn me(ctx: Ctx) -> Response {
    let Some(user_id) = ctx.user.as_ref().map(|user| user.id) else {
        return axum::response::Redirect::to("/login").into_response();
    };
    profile(ctx, user_id).await
}

/// The `{id}` handler, `pub(crate)` so the DB-gated tests can drive it
/// against a live server — the router test's unconnected handle cannot tell
/// a missing row from an outage.
pub(crate) async fn by_id(ctx: Ctx, Path(id): Path<String>) -> Response {
    // a non-numeric id is a path no profile route could have minted —
    // exactly what the 404 page is for
    let Ok(numeric) = id.parse::<i64>() else {
        return errors::not_found(ctx);
    };
    profile(ctx, numeric).await
}

async fn profile(ctx: Ctx, id: i64) -> Response {
    let state = crate::state::state();
    // a database outage is not a missing user: 500, not 404 — the 404 page
    // would read as "this account never existed" to a visitor and a crawler
    let row = match crate::db::find_user_by_id(&state.db, id).await {
        Ok(row) => row,
        Err(_) => return errors::server_error(ctx),
    };
    let Some(row) = row else {
        return errors::not_found(ctx);
    };
    let builds = match builds::select_by_user(&state.db, &ctx.lang, id).await {
        Ok(b) => b,
        Err(_) => return errors::server_error(ctx),
    };
    let initial = initial(&row.username);
    let joined = joined_day(row.created_at, ctx.tz_offset);
    let page = ProfilePage {
        user: ProfileUser {
            username: row.username,
            google_linked: row.google_sub.is_some(),
        },
        builds,
        ctx,
        initial,
        joined,
    };
    match page.render() {
        Ok(html) => Html(html).into_response(),
        // the same fallback every template render in this tree uses
        Err(e) => Html(format!("500 unexpected error: {e}")).into_response(),
    }
}
