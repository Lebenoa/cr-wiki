//! Cache only catalog GET representations, inside locale/session resolution.
use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use super::page_cache::{cache, Key, Page};
use crate::ctx::Ctx;

pub async fn serve(ctx: Ctx, request: Request, next: Next) -> Response {
    if request.method() != Method::GET {
        return next.run(request).await;
    }
    let key = Key::new(&ctx, &request.uri().to_string());
    let headers = request.headers().clone();
    let db = &crate::state::state().db;
    // A failed revision read must never resurrect a retained representation.
    let revision = crate::db::catalog_revision(db).await.ok();
    if let Some(revision) = revision.as_deref() {
        if let Some(page) = cache().get(&key, revision) {
            return page.response(&headers);
        }
    }
    let mut response = next.run(request).await;
    if response.status() != StatusCode::OK {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }
    let (mut parts, body) = response.into_parts();
    // These handlers already produce finite String bodies, not streams.
    let bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::error!(%error, "catalog response body failed");
            return super::errors::server_error(ctx);
        }
    };
    let html = match String::from_utf8(bytes.into()) {
        Ok(html) => html,
        Err(error) => {
            parts
                .headers
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            return Response::from_parts(parts, Body::from(error.into_bytes()));
        }
    };
    // A write overlapping a render must not cache mixed/pre-write data under
    // the post-write generation. Compare both sides, not only the later read.
    let page = if let Some(before) = revision {
        match crate::db::catalog_revision(db).await {
            Ok(after) if before == after => cache().insert(key, before, html),
            _ => std::sync::Arc::new(Page::new(html)),
        }
    } else {
        std::sync::Arc::new(Page::new(html))
    };
    page.response(&headers)
}
