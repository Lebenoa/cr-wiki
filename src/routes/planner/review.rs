//! A signed-in visitor's verdict on someone's build.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::builds;
use crate::ctx::Ctx;

use crate::routes::errors::{self, AppError};

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
        return Ok(errors::not_found(ctx));
    }

    let ok = form.verified.as_deref() == Some("1");
    let reason = form.reason.clone().unwrap_or_default();
    builds::upsert_review(&state.db, id, user, ok, &reason).await?;
    Ok(Redirect::to(&format!("/builds/{id}")).into_response())
}
