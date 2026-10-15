//! The editor's write side: the two entity saves and the combo-row removal.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;

use crate::ctx::Ctx;
use crate::db;
use crate::section::Section;

use crate::routes::errors::{self, AppError};

use super::form::{editable, form_lang, EntityForm};
use super::render::page;

pub async fn create(
    ctx: Ctx,
    Path(section): Path<String>,
    Form(form): Form<EntityForm>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(section) = editable(&section) else {
        return Ok(errors::not_found(ctx));
    };
    if !ctx.is_admin() {
        return Ok(errors::not_found(ctx));
    }
    if form.name.trim().is_empty() {
        let body = page(ctx, section, None, "admin_error_name").await;
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    }
    let created = db::insert_entity(&state.db, &form_lang(&form, &ctx), section, &form).await?;

    if created <= 0 {
        let body = page(ctx, section, None, "admin_error_save").await;
        return Ok((StatusCode::INTERNAL_SERVER_ERROR, body).into_response());
    }
    Ok(Redirect::to(&format!("/{}/{created}", section.as_str())).into_response())
}

pub async fn update(
    ctx: Ctx,
    Path((section, id)): Path<(String, i64)>,
    Form(form): Form<EntityForm>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(section) = editable(&section) else {
        return Ok(errors::not_found(ctx));
    };
    if !ctx.is_admin() {
        return Ok(errors::not_found(ctx));
    }
    if form.name.trim().is_empty() {
        let body = page(ctx, section, None, "admin_error_name").await;
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    }
    let ok = db::update_entity(&state.db, &form_lang(&form, &ctx), section, id, &form).await?;

    if !ok {
        let body = page(ctx, section, None, "admin_error_save").await;
        return Ok((StatusCode::INTERNAL_SERVER_ERROR, body).into_response());
    }
    Ok(Redirect::to(&format!("/{}/{}", section.as_str(), id)).into_response())
}

/// Removes one combo pairing row from the entity's editor. The delete only
/// lands when the row hangs off the entity being edited (its cookie side on
/// a cookie's editor, its pet side on a pet's), so a guessed row id from a
/// different editor's context cannot delete someone else's pairing.
pub async fn delete_combi(
    ctx: Ctx,
    Path((section, id, row_id)): Path<(String, i64, i64)>,
) -> Response {
    let state = crate::state::state();
    let Some(section) = Section::parse(&section) else {
        return errors::not_found(ctx);
    };
    if !ctx.is_admin() || !matches!(section, Section::Cookies | Section::Pets) {
        return errors::not_found(ctx);
    }
    if let Err(e) = db::delete_combi(&state.db, row_id, section.as_str(), id).await {
        tracing::error!("delete combi row {row_id}: {e}");
    }
    Redirect::to(&format!("/{}/{}", section.as_str(), id)).into_response()
}
