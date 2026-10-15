//! The writes: submitting, editing and deleting a build, plus the ownership
//! check they all answer to.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;

use crate::builds::{self, BuildCard};
use crate::ctx::Ctx;
use crate::state::AppState;
use crate::time::now_unix;

use crate::routes::errors::{self, AppError};

use super::form::{changes_from, parse_ep, record_from, tags_of, BuildForm};
use super::page::page;

/// Anonymous submissions live 24 hours; a signed-in one is permanent.
const ANON_TTL_SECS: i64 = 24 * 60 * 60;

pub async fn create(ctx: Ctx, Form(form): Form<BuildForm>) -> Result<Response, AppError> {
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

pub async fn update(
    ctx: Ctx,
    Path(id): Path<i64>,
    Form(form): Form<BuildForm>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(existing) = load_owned(state, &ctx, id).await? else {
        return Ok(errors::not_found(ctx));
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
    // the update only carries the mutable fields; author, owner and
    // expiry are insert-only (update_build's parameter type has no room
    // for them)
    let changes = match changes_from(&form, ep, ep_special, tags) {
        Ok(c) => c,
        Err(key) => {
            return Ok((StatusCode::BAD_REQUEST, page(ctx, Some(existing), key)).into_response())
        }
    };
    builds::update_build(&state.db, id, &changes).await?;
    Ok(Redirect::to(&format!("/builds/{id}")).into_response())
}

pub async fn delete(ctx: Ctx, Path(id): Path<i64>) -> Result<Response, AppError> {
    let state = crate::state::state();
    let found = builds::select_build(&state.db, &ctx.lang, id).await?;

    let Some(build) = found else {
        return Ok(errors::not_found(ctx));
    };
    // 404 rather than 403 for someone else's build, matching the V routes:
    // whether a build exists is not worth revealing to a stranger
    if !can_edit(&ctx, &build) {
        return Ok(errors::not_found(ctx));
    }
    builds::delete_build(&state.db, id).await?;
    Ok(Redirect::to("/builds").into_response())
}

/// Loads a build only when the caller may change it.
pub(super) async fn load_owned(
    state: &AppState,
    ctx: &Ctx,
    id: i64,
) -> Result<Option<BuildCard>, AppError> {
    let found = builds::select_build(&state.db, &ctx.lang, id).await?;
    let Some(found) = found else {
        return Ok(None);
    };
    Ok(can_edit(ctx, &found).then_some(found))
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
