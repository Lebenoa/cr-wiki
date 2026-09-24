//! The two layers every request goes through, in the order `before_request`
//! applies them: rate limit first (a denied request should cost nothing
//! further), then locale resolution.

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::SocketAddr;

use crate::ctx::{self, LANG_COOKIE};
use crate::ratelimit::Decision;
use crate::session::SESSION_COOKIE;
use crate::state::AppState;

/// Resolves the locale and the signed-in user once per request, and
/// refreshes the `wikilang` cookie when the URL carried a new one — the whole
/// of `before_request`.
pub async fn context(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    if let Some(key) = ctx::cookie(req.headers(), SESSION_COOKIE) {
        if let Some(user) = state.sessions.get(&key) {
            req.extensions_mut().insert(user);
        }
    }
    let choice = ctx::resolve_lang(req.uri().query(), req.headers());
    let write = choice.write_cookie;
    let lang = choice.lang.clone();
    req.extensions_mut().insert(choice);

    let mut res = next.run(req).await;
    if write {
        if let Ok(v) = HeaderValue::from_str(&format!("{LANG_COOKIE}={lang}; path=/")) {
            res.headers_mut().append(header::SET_COOKIE, v);
        }
    }
    res
}

/// Per-IP token bucket. A denied fragment swap gets an empty body so the
/// infinite-scroll sentinel is removed by its own outerHTML swap and search
/// results just stop quietly; a full-page or hx-boosted navigation gets the
/// plain text, so the visitor sees why the page is blank.
pub async fn rate_limit(State(state): State<AppState>, req: Request, next: Next) -> Response {
    // read off the extensions rather than as an extractor: ConnectInfo is not
    // an optional extractor, and a request without a peer address must still
    // be servable (it just shares the 'unknown' bucket)
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    let ip = ctx::client_ip(req.headers(), peer, &state.cfg.ratelimit.trusted_proxies);
    match state.limiter.check(&ip) {
        Decision::Allow => next.run(req).await,
        Decision::Deny(retry_after) => {
            tracing::warn!("rate limit hit for {ip}");
            let htmx = req.headers().get("HX-Request").is_some();
            let boosted = req.headers().get("HX-Boosted").is_some();
            let body = if htmx && !boosted {
                ""
            } else {
                "too many requests"
            };
            let mut res = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
            if let Ok(v) = HeaderValue::from_str(&retry_after.to_string()) {
                res.headers_mut().insert(header::RETRY_AFTER, v);
            }
            res
        }
    }
}
