//! The planner page itself: the template, the one place it is composed, and
//! the two handlers that only read it — the blank form and the prefilled edit.

use askama::Template;
use axum::extract::Path;
use axum::response::{Html, IntoResponse, Response};

use crate::builds::BuildCard;
use crate::ctx::Ctx;
use crate::prefill::Prefill;

use crate::routes::errors::{self, AppError};

use super::write::load_owned;

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

pub(super) fn page(ctx: Ctx, build: Option<BuildCard>, error: &str) -> Response {
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
pub async fn edit_form(ctx: Ctx, Path(id): Path<String>) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(build) = load_owned(state, &ctx, &id).await? else {
        return Ok(errors::not_found(ctx));
    };
    Ok(page(ctx, Some(build), ""))
}
