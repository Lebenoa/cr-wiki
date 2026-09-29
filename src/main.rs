//! Server startup: load config, connect, bind, serve. `router()` lives in
//! `routes`, so the interface tests drive is the one prod serves through.

mod builds;
mod changelog;
mod chrome;
mod config;
mod ctx;
mod db;
mod grade;
mod i18n;
mod middleware;
mod options;
mod pagination;
mod prefill;
mod ratelimit;
mod richtext;
mod routes;
mod section;
mod session;
mod state;
#[cfg(test)]
mod testutil;
mod time;
mod turnstile;
mod upload;

use std::net::SocketAddr;

use axum::extract::Request;
use axum::http::{header, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;

use state::AppState;

/// Static assets are named for their content (entity sprites never change;
/// `styles.css` and `js/` ride deploys), so images cache for a year while
/// everything else revalidates after a day. `ServeDir` already answers
/// conditional requests with `Last-Modified`, so the day-old entries cost a
/// 304, not a re-download. The layer sits on the whole router, so paths
/// outside `/static` must pass through untouched — a cached day on a
/// dynamic page would serve stale content after a deploy.
async fn static_cache_headers(req: Request, next: Next) -> Response {
    // the path must be read before the request moves into the inner service
    let rest = req.uri().path().strip_prefix("/static/").map(str::to_string);
    let mut res = next.run(req).await;
    let Some(rest) = rest else {
        return res;
    };
    let value = if rest.starts_with("img/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=86400"
    };
    if let Ok(v) = HeaderValue::from_str(value) {
        res.headers_mut().insert(header::CACHE_CONTROL, v);
    }
    res
}

/// Paths are relative to the repo root, so the port runs against the same
/// Config.toml, translations and database as the V app.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = config::Config::load("Config.toml");
    i18n::load("translations");
    tracing::info!(langs = ?i18n::available_langs(), "translations loaded");

    if !cfg.surreal.is_configured() {
        tracing::error!("no SurrealDB server configured: set [surreal] in Config.toml");
        return;
    }
    let pool = match db::connect(&cfg.surreal).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("cannot connect to {}: {e}", cfg.surreal.url);
            return;
        }
    };

    let state = AppState {
        db: pool,
        limiter: ratelimit::Limiter::new(cfg.ratelimit.clone()),
        sessions: session::Sessions::new(),
        cfg: cfg.clone(),
    };

    // Warm the wire pool before serving: the SDK establishes its WebSocket
    // lazily, so without this the first visitor paid the whole handshake
    // (~11 s observed) while their request was in flight.
    let warm = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        db::warm_pool(&state.db),
    )
    .await;
    if let Err(e) = warm {
        tracing::warn!("database warmup timed out: {e}");
    }
    state::init(state);

    let app = routes::router();
    // Every asset URL in the templates lives under /static — styles.css,
    // js/, img/, thirdparty/ and the favicon — so one mount serves the whole
    // `static/` tree. The catalog's `/{section}/{id}` detail route cannot
    // shadow these: its two-segment shape never overlaps a /static prefix,
    // and the prefix mount wins the match anyway.
    let app = app
        .nest_service("/static", ServeDir::new("static"))
        .layer(axum::middleware::from_fn(static_cache_headers))
        // gzip/br for HTML, CSS and JS; the default predicate leaves image/*
        // (the bulk of /static) and tiny responses alone
        .layer(CompressionLayer::new());

    let addr = format!("{}:{}", cfg.host, cfg.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot bind {addr}: {e}");
            return;
        }
    };
    tracing::info!("listening on {addr}");
    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        tracing::error!("server error: {e}");
    }
}
