//! The editor's read side: the blank form for a new entity, and the prefilled
//! one for an existing row in whichever locale is being edited.

use axum::extract::{Path, Query};
use axum::response::Response;

use crate::ctx::Ctx;
use crate::db;
use crate::section::Section;

use crate::routes::errors::{self, AppError};
use crate::routes::CommonQuery;

use super::form::editable;
use super::render::{page, page_with};

pub async fn new_form(ctx: Ctx, Path(section): Path<String>) -> Response {
    let Some(section) = editable(&section) else {
        return errors::not_found(ctx);
    };
    if !ctx.is_admin() {
        return errors::not_found(ctx);
    }
    page(ctx, section, None, "").await
}

pub async fn edit_form(
    ctx: Ctx,
    Path((section, id)): Path<(String, i64)>,
) -> Result<Response, AppError> {
    edit_form_lang(ctx, Query::<CommonQuery>::default(), Path((section, id))).await
}

pub async fn edit_form_lang(
    ctx: Ctx,
    q: Query<CommonQuery>,
    Path((section, id)): Path<(String, i64)>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(section) = editable(&section) else {
        return Ok(errors::not_found(ctx));
    };
    if !ctx.is_admin() {
        return Ok(errors::not_found(ctx));
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
        return Ok(errors::not_found(ctx));
    };
    // the preview box resolves [[kind:id]] names in the locale being edited
    // (the name lists carry the en fallback), not the viewer's UI locale.
    // The edited entity highlights rather than self-links, same as the
    // detail page.
    let mut memo = crate::richtext::LinkCache::new();
    let self_ref = match section {
        Section::Cookies => Some(("cookie", id)),
        Section::Pets => Some(("pet", id)),
        Section::Treasures => Some(("treasure", id)),
        _ => None,
    };
    let preview_html = crate::richtext::render_with(
        &state.db,
        &edit_lang,
        &item.description,
        &mut memo,
        self_ref,
    )
    .await;
    let combi = db::combi_edit_rows(&state.db, &ctx.lang, section.as_str(), id).await?;
    Ok(page_with(ctx, section, Some(item), combi, "", edit_lang, preview_html).await)
}
