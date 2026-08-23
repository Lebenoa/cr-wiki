//! The small JSON endpoints the front end calls.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use serde_json::json;

use crate::ctx::{Ctx, LANG_COOKIE};
use crate::db;
use crate::i18n;
use crate::state::AppState;

use super::CommonQuery;

/// The locales the site is served in.
pub async fn available_langs() -> Json<serde_json::Value> {
    Json(json!({ "langs": i18n::available_langs() }))
}

/// Name and id pairs for the rich-text editor's entity picker, in the
/// requested language. `kind` is cookie, pet or treasure.
pub async fn richtext_names(
    State(state): State<AppState>,
    ctx: Ctx,
    q: Query<CommonQuery>,
) -> Json<serde_json::Value> {
    let kind = match q.kind.as_deref() {
        Some(k @ ("pet" | "treasure")) => k.to_string(),
        _ => "cookie".to_string(),
    };
    let section = match kind.as_str() {
        "pet" => "pets",
        "treasure" => "treasures",
        _ => "cookies",
    };
    let names = crate::options::options(&state.db, &ctx.lang, &kind)
        .await
        .into_iter()
        .map(|o| json!({ "id": o.id, "name": o.name, "en_name": o.en_name }))
        .collect::<Vec<_>>();
    Json(json!({ "kind": section, "names": names }))
}

/// Sets the language cookie and returns to where the visitor was. The value
/// is validated against the loaded catalogs, so an unknown locale is ignored
/// rather than stored.
pub async fn set_lang(q: Query<CommonQuery>) -> Response {
    let Some(lang) = q.lang.as_deref().filter(|l| i18n::is_available(l)) else {
        return Redirect::to("/").into_response();
    };
    let back = q.next.clone().unwrap_or_else(|| "/".to_string());
    // only a same-site path of printable ASCII: an absolute URL here would be
    // an open redirect, and control or non-ASCII bytes (axum's Query
    // percent-decodes BEFORE this check) make HeaderValue construction —
    // and therefore Redirect::to — panic
    let back = if back.starts_with('/')
        && !back.starts_with("//")
        && back.bytes().all(|b| (0x20..=0x7e).contains(&b))
    {
        back
    } else {
        "/".to_string()
    };
    let mut res = Redirect::to(&back).into_response();
    if let Ok(v) =
        axum::http::HeaderValue::from_str(&format!("{LANG_COOKIE}={lang}; path=/"))
    {
        res.headers_mut().append(axum::http::header::SET_COOKIE, v);
    }
    res
}

/// Kept next to the other API handlers: the relic list, which has no page of
/// its own yet but is already queryable.
pub async fn relics(State(state): State<AppState>, ctx: Ctx) -> Json<serde_json::Value> {
    let rows = db::select_simple(&state.db, &ctx.lang, "relics")
        .await
        .unwrap_or_default();
    Json(json!({
        "relics": rows
            .into_iter()
            .map(|c| json!({ "id": c.id, "name": c.name, "image": c.image }))
            .collect::<Vec<_>>()
    }))
}
