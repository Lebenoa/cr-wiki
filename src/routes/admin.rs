//! The admin catalog forms: create and edit a cookie, pet or treasure.
//!
//! Every handler gates on `ctx.is_admin()` and answers 404 rather than 403,
//! matching the V routes — whether an admin page exists is not worth
//! confirming to a stranger.

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db::{self, CombiEditRow, Detail};
use crate::options;
use crate::section::Section;
use crate::state::AppState;

use super::errors::AppError;

#[derive(Template)]
#[template(path = "admin_form.html")]
struct AdminForm {
    ctx: Ctx,
    /// cookies, pets or treasures
    section: String,
    /// filled when editing
    item: Option<Detail>,
    /// the combo pairings this entity takes part in; empty for a treasure
    combi: Vec<CombiEditRow>,
    error: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct EntityForm {
    pub name: String,
    #[serde(default)]
    pub abilities: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub power_plus: String,
    #[serde(default)]
    pub power_plus_requirement: String,
    #[serde(default)]
    pub unlock_goal: String,
    #[serde(default)]
    pub image: String,
    pub grade: Option<i64>,
}

impl AdminForm {
    /// The `.tr` keys are per-section (`save_cookie_button`), so the template
    /// builds none of them itself. A method because askama calls it on the
    /// context struct.
    #[allow(clippy::unused_self)]
    fn singular(&self) -> &'static str {
        match self.section.as_str() {
            "pets" => "pet",
            "treasures" => "treasure",
            _ => "cookie",
        }
    }

    fn title_key(&self) -> String {
        let verb = if self.item.is_some() { "edit" } else { "new" };
        format!("{verb}_{}_page_header", self.singular())
    }

    fn save_key(&self) -> String {
        let verb = if self.item.is_some() {
            "save"
        } else {
            "create"
        };
        format!("{verb}_{}_button", self.singular())
    }

    fn name_key(&self) -> String {
        format!("{}_name", self.singular())
    }
}

fn known(section: &str) -> bool {
    Section::parse(section).is_some_and(Section::editable)
}

fn page(ctx: Ctx, section: &str, item: Option<Detail>, error: &str) -> Response {
    page_with(ctx, section, item, Vec::new(), error)
}

fn page_with(
    ctx: Ctx,
    section: &str,
    item: Option<Detail>,
    combi: Vec<CombiEditRow>,
    error: &str,
) -> Response {
    Html(
        AdminForm {
            ctx,
            section: section.to_string(),
            item,
            combi,
            error: error.to_string(),
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response()
}

pub async fn new_form(ctx: Ctx, Path(section): Path<String>) -> Response {
    if !ctx.is_admin() || !known(&section) {
        return super::errors::not_found(ctx);
    }
    page(ctx, &section, None, "")
}

pub async fn edit_form(
    State(state): State<AppState>,
    ctx: Ctx,
    Path((section, id)): Path<(String, i64)>,
) -> Result<Response, AppError> {
    if !ctx.is_admin() || !known(&section) {
        return Ok(super::errors::not_found(ctx));
    }
    let found = db::select_detail(&state.db, &ctx.lang, &section, id).await?;

    let Some(item) = found else {
        return Ok(super::errors::not_found(ctx));
    };
    let combi = db::combi_edit_rows(&state.db, &ctx.lang, &section, id).await?;
    Ok(page_with(ctx, &section, Some(item), combi, ""))
}

pub async fn create(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(section): Path<String>,
    Form(form): Form<EntityForm>,
) -> Result<Response, AppError> {
    if !ctx.is_admin() || !known(&section) {
        return Ok(super::errors::not_found(ctx));
    }
    if form.name.trim().is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            page(ctx, &section, None, "admin_error_name"),
        )
            .into_response());
    }
    let created = db::insert_entity(&state.db, &ctx.lang, &section, &form).await?;

    if created <= 0 {
        return Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            page(ctx, &section, None, "admin_error_save"),
        )
            .into_response());
    }
    // the picker lists cache the catalog; a write has to show up next request
    options::invalidate();
    Ok(Redirect::to(&format!("/{section}/{created}")).into_response())
}

pub async fn update(
    State(state): State<AppState>,
    ctx: Ctx,
    Path((section, id)): Path<(String, i64)>,
    Form(form): Form<EntityForm>,
) -> Result<Response, AppError> {
    if !ctx.is_admin() || !known(&section) {
        return Ok(super::errors::not_found(ctx));
    }
    if form.name.trim().is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            page(ctx, &section, None, "admin_error_name"),
        )
            .into_response());
    }
    let ok = db::update_entity(&state.db, &ctx.lang, &section, id, &form).await?;

    if !ok {
        return Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            page(ctx, &section, None, "admin_error_save"),
        )
            .into_response());
    }
    options::invalidate();
    Ok(Redirect::to(&format!("/{section}/{id}")).into_response())
}

/// Removes one combo pairing from the editor. Takes the row id rather than a
/// pair of entity ids: deleting by pair would take every duplicate with it.
pub async fn delete_combi(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(row_id): Path<i64>,
) -> Response {
    if !ctx.is_admin() {
        return super::errors::not_found(ctx);
    }
    if let Err(e) = db::delete_combi(&state.db, row_id).await {
        tracing::error!("delete combi row {row_id}: {e}");
    }
    options::invalidate();
    // back to where the editor was; the referer is the entity's own form
    Redirect::to("/cookies").into_response()
}
