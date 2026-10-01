//! The layers every request goes through: rate limit first (a denied
//! request should cost nothing further), then locale resolution, then the
//! security baselines on the way out (see [`security_headers`]).

use axum::extract::{ConnectInfo, Request};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::SocketAddr;

use crate::ctx::{self, LANG_COOKIE};
use crate::ratelimit::Decision;
use crate::session::SESSION_COOKIE;

/// Resolves the locale and the signed-in user once per request, and
/// refreshes the `wikilang` cookie when the URL carried a new one — the whole
/// of `before_request`.
pub async fn context(mut req: Request, next: Next) -> Response {
    let state = crate::state::state();
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

/// Baselines every HTML response: clickjacking, MIME sniffing and referrer
/// leakage are shut off at the header, and a CSP bounds what a page may
/// load. The policy is deliberately permissive about scripts and styles —
/// the templates ship inline `<script>` blocks and `onclick=` attributes,
/// and the richtext renderer emits `style=` color spans — so `script-src`
/// and `style-src` carry `'unsafe-inline'`; the value is in the harder
/// defaults (`object-src 'none'`, `base-uri 'self'`, `frame-ancestors
/// 'none'`, `form-action 'self'`) plus locking connect/img/font sources to
/// the site, Google Fonts, and Cloudflare Turnstile (the login/register
/// widget's script, iframe and token requests all live on
/// `challenges.cloudflare.com`; gate those and auth fails closed in
/// release builds, where the widget is actually enforced).
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("SAMEORIGIN"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; \
             script-src 'self' 'unsafe-inline' https://challenges.cloudflare.com; \
             style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
             img-src 'self' data: https://challenges.cloudflare.com; \
             font-src 'self' https://fonts.gstatic.com; \
             connect-src 'self' https://challenges.cloudflare.com; \
             frame-src https://challenges.cloudflare.com; \
             object-src 'none'; base-uri 'self'; \
             form-action 'self'; frame-ancestors 'none'",
        ),
    );
    res
}

/// Per-IP token bucket. A denied fragment swap gets an empty body so the
/// infinite-scroll sentinel is removed by its own outerHTML swap and search
/// results just stop quietly; a full-page or hx-boosted navigation gets the
/// plain text, so the visitor sees why the page is blank.
pub async fn rate_limit(req: Request, next: Next) -> Response {
    let state = crate::state::state();
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
