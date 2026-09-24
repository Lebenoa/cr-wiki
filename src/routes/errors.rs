//! The error pages. A miss is answered with the site's own 404 rather than a
//! line of plain text, so a wrong URL still lands somewhere navigable.

use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundPage {
    ctx: Ctx,
}

#[derive(Template)]
#[template(path = "server_error.html")]
struct ServerErrorPage {
    ctx: Ctx,
}

/// 404 with the rendered page. Handlers return this wherever they used to
/// answer a bare "not found", including the deliberate 404s that stand in
/// for a 403 — whether a page exists is not worth confirming to a stranger.
pub fn not_found(ctx: Ctx) -> Response {
    let html = NotFoundPage { ctx }
        .render()
        .unwrap_or_else(|_| "404 not found".to_string());
    (StatusCode::NOT_FOUND, Html(html)).into_response()
}

pub fn server_error(ctx: Ctx) -> Response {
    let html = ServerErrorPage { ctx }
        .render()
        .unwrap_or_else(|_| "500 unexpected error".to_string());
    (StatusCode::INTERNAL_SERVER_ERROR, Html(html)).into_response()
}

/// The router's catch-all, for a path no route claims.
pub async fn fallback(ctx: Ctx) -> Response {
    not_found(ctx)
}

/// A failed database call: logged with its cause and answered with the
/// site's own 500 page. An outage must never masquerade as an empty wiki —
/// that reads as "no data" to visitors, crawlers and monitors alike.
pub struct AppError(surrealdb::Error);

impl From<surrealdb::Error> for AppError {
    fn from(e: surrealdb::Error) -> Self {
        Self(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!("database error: {}", self.0);
        server_error(Ctx::minimal())
    }
}

/// The panic hook has no request to read, so the page renders in the default
/// locale rather than the visitor's. `CatchPanicLayer` requires the box by
/// value, so the lint's advice does not apply here.
#[allow(clippy::needless_pass_by_value)]
pub fn panic_response(err: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let detail = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("unknown");
    tracing::error!("handler panicked: {detail}");
    server_error(Ctx::minimal())
}
