//! The admin catalog forms: create and edit a cookie, pet or treasure.
//!
//! Every handler gates on `ctx.is_admin()` and answers 404 rather than 403,
//! matching the V routes — whether an admin page exists is not worth
//! confirming to a stranger.

use askama::Template;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db::{self, CombiEditRow, Detail};
use crate::section::Section;

use super::errors::AppError;
use super::CommonQuery;

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
    /// which locale's translation the form shows and saves; the old V
    /// editor's language select, restored
    edit_lang: String,
    /// the description rendered for the editing locale, the initial content
    /// of the preview box
    preview_html: String,
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
    /// which locale's translation this submission writes; empty falls back
    /// to the viewer's language
    #[serde(default)]
    pub lang: String,
}

impl AdminForm {
    /// The `.tr` keys are per-section (`save_cookie_button`), so the template
    /// builds none of them itself. A method because askama calls it on the
    /// context struct.
    fn singular(&self) -> &'static str {
        Section::parse(&self.section).map_or("cookie", Section::singular)
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

/// The admin editor writes exactly the editable sections; everything else is
/// a 404 that does not confirm existence. One gate, at the typed edge.
fn editable(section: &str) -> Option<Section> {
    Section::parse(section).filter(|s| s.editable())
}

/// The locale the submitted form edits: the form's own `lang` field when it
/// is a loaded locale (the select the template renders), else the viewer's
/// language. Validated so an unknown value cannot create a rogue `tr.xx`.
fn form_lang(form: &EntityForm, ctx: &Ctx) -> String {
    crate::i18n::is_available(&form.lang)
        .then(|| form.lang.clone())
        .unwrap_or_else(|| ctx.lang.clone())
}

fn page(ctx: Ctx, section: Section, item: Option<Detail>, error: &str) -> Response {
    page_with(
        ctx.clone(),
        section,
        item,
        Vec::new(),
        error,
        ctx.lang.clone(),
        String::new(),
    )
}

#[allow(clippy::too_many_arguments)]
fn page_with(
    ctx: Ctx,
    section: Section,
    item: Option<Detail>,
    combi: Vec<CombiEditRow>,
    error: &str,
    edit_lang: String,
    preview_html: String,
) -> Response {
    Html(
        AdminForm {
            ctx,
            section: section.as_str().to_string(),
            item,
            combi,
            edit_lang,
            preview_html,
            error: error.to_string(),
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response()
}

pub async fn new_form(ctx: Ctx, Path(section): Path<String>) -> Response {
    let Some(section) = editable(&section) else {
        return super::errors::not_found(ctx);
    };
    if !ctx.is_admin() {
        return super::errors::not_found(ctx);
    }
    page(ctx, section, None, "")
}

pub async fn edit_form(
    ctx: Ctx,
    Path((section, id)): Path<(String, i64)>,
) -> Result<Response, AppError> {
    edit_form_lang(ctx, axum::extract::Query::<CommonQuery>::default(), Path((section, id)))
        .await
}

pub async fn edit_form_lang(
    ctx: Ctx,
    q: axum::extract::Query<CommonQuery>,
    Path((section, id)): Path<(String, i64)>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(section) = editable(&section) else {
        return Ok(super::errors::not_found(ctx));
    };
    if !ctx.is_admin() {
        return Ok(super::errors::not_found(ctx));
    }
    // which translation the form edits: ?lang= when it is a loaded locale,
    // otherwise the viewer's resolved language
    let edit_lang = q
        .lang
        .as_deref()
        .filter(|l| crate::i18n::is_available(l))
        .unwrap_or(&ctx.lang)
        .to_string();
    // strict: no English fallback, or the first save would silently
    // overwrite an untranslated locale with the English text
    let found = db::select_detail_strict(&state.db, &edit_lang, section.as_str(), id).await?;

    let Some(item) = found else {
        return Ok(super::errors::not_found(ctx));
    };
    // the preview box resolves [[kind:id]] names in the locale being edited
    // (the name lists carry the en fallback), not the viewer's UI locale
    let mut memo = crate::richtext::LinkCache::new();
    let preview_html =
        crate::richtext::render_with(&state.db, &edit_lang, &item.description, &mut memo).await;
    let combi = db::combi_edit_rows(&state.db, &ctx.lang, section.as_str(), id).await?;
    Ok(page_with(ctx, section, Some(item), combi, "", edit_lang, preview_html))
}

pub async fn create(
    ctx: Ctx,
    Path(section): Path<String>,
    Form(form): Form<EntityForm>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(section) = editable(&section) else {
        return Ok(super::errors::not_found(ctx));
    };
    if !ctx.is_admin() {
        return Ok(super::errors::not_found(ctx));
    }
    if form.name.trim().is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            page(ctx, section, None, "admin_error_name"),
        )
            .into_response());
    }
    let created = db::insert_entity(&state.db, &form_lang(&form, &ctx), section, &form).await?;

    if created <= 0 {
        return Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            page(ctx, section, None, "admin_error_save"),
        )
            .into_response());
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
        return Ok(super::errors::not_found(ctx));
    };
    if !ctx.is_admin() {
        return Ok(super::errors::not_found(ctx));
    }
    if form.name.trim().is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            page(ctx, section, None, "admin_error_name"),
        )
            .into_response());
    }
    let ok = db::update_entity(&state.db, &form_lang(&form, &ctx), section, id, &form).await?;

    if !ok {
        return Ok((
            StatusCode::INTERNAL_SERVER_ERROR,
            page(ctx, section, None, "admin_error_save"),
        )
            .into_response());
    }
    Ok(Redirect::to(&format!("/{}/{}", section.as_str(), id)).into_response())
}

/// Removes one combo pairing from the editor. Takes the row id rather than a
/// pair of entity ids: deleting by pair would take every duplicate with it.
pub async fn delete_combi(
    ctx: Ctx,
    Path(row_id): Path<i64>,
) -> Response {
    let state = crate::state::state();
    if !ctx.is_admin() {
        return super::errors::not_found(ctx);
    }
    if let Err(e) = db::delete_combi(&state.db, row_id).await {
        tracing::error!("delete combi row {row_id}: {e}");
    }
    // back to where the editor was; the referer is the entity's own form
    Redirect::to("/cookies").into_response()
}
