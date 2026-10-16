//! The HTTP module's interface: `router()` is the seam prod serves through and
//! tests drive with the tower oneshot adapter — same layer order, same
//! fallback, no second assembly to drift.
//!
//! Each surface owns its table (`auth::routes()`, `admin::routes()`, …) beside
//! the handlers it names; this file merges them and is the one place the layers
//! and the fallback are applied, so the order that matters — rate limit, then
//! locale, then routes — is still visible whole.
//!
//! One caveat is deliberate: the static mounts are absent here because the
//! dev working directory they read does not exist in the test process; main
//! adds them around the same routes.

use axum::Router;
use serde::{Deserialize, Deserializer};
use tower_http::catch_panic::CatchPanicLayer;

use crate::middleware;

pub mod admin;
pub mod api;
pub mod auth;
pub mod builds;
pub mod catalog;
mod catalog_cache;
pub mod changelog;
pub mod detail;
pub mod errors;
pub mod misc;
pub mod news;
mod page_cache;
pub mod picker;
pub mod planner;
pub mod uploads;

/// Router assembly in one function: every surface contributes its own table
/// and the layers below are applied to all of them at once. The merge order is
/// the order the single `route` chain used to have — a static segment before
/// the `{id}` capture that would otherwise be read as it — so the pattern
/// comments in those tables still hold.
pub fn router() -> Router {
    misc::routes()
        .merge(api::routes())
        .merge(changelog::routes())
        // before the catalog captures: /news/{id} and /admin/news would
        // otherwise be read as a section detail page
        .merge(news::routes())
        .merge(picker::routes())
        .merge(planner::routes())
        .merge(builds::routes())
        .merge(auth::routes())
        .merge(admin::routes())
        .merge(uploads::routes())
        .merge(catalog::routes())
        .merge(detail::routes())
        .layer(axum::middleware::from_fn(middleware::context))
        .layer(axum::middleware::from_fn(middleware::rate_limit))
        // a path no route claims gets the site's own 404, not a bare line
        .fallback(errors::fallback)
        // a panic in a handler would otherwise drop the connection with no
        // response at all; the client sees the 500 page instead
        .layer(CatchPanicLayer::custom(errors::panic_response))
        // outermost, so even the panic page carries the baseline headers
        .layer(axum::middleware::from_fn(middleware::security_headers))
}

///
/// picker.js builds the preview URL as `cookie=&c2=&pet=&t1=&t2=&t3=`, with
/// an empty value for every slot that has no pick yet. Plain `Option<i64>`
/// fails on the empty string, which fails the whole extractor and 400s the
/// request — so an unfilled planner would get no preview at all.
fn empty_as_none<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(d)?;
    Ok(raw
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok()))
}

/// Query parameters the pages share. `lang` is resolved by the middleware
/// (see `ctx::resolve_lang`) rather than read here; it stays in the struct so
/// serde does not reject the parameter.
#[derive(Debug, Default, Deserialize)]
pub struct CommonQuery {
    #[allow(dead_code)]
    pub lang: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub page: Option<i64>,
    pub q: Option<String>,
    pub tab: Option<String>,
    pub sort: Option<String>,
    pub ep: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub cookie: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub pet: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub treasure: Option<i64>,
    /// the option already in the slot the picker is opening for
    #[serde(default, deserialize_with = "empty_as_none")]
    pub sel: Option<i64>,
    /// the other half of the combo pair, whose partners float to the top
    #[serde(default, deserialize_with = "empty_as_none")]
    pub partner: Option<i64>,
    /// the planner's live preview posts the whole selection
    #[serde(default, deserialize_with = "empty_as_none")]
    pub c2: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t1: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t2: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t3: Option<i64>,
    /// which entity list the rich-text picker wants
    pub kind: Option<String>,
    /// where /api/set-lang should return to
    pub next: Option<String>,
    /// the build list's free-text author filter
    #[serde(default)]
    pub author: Option<String>,
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests;
