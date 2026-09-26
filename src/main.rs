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
use std::sync::Arc;

use tower_http::services::ServeDir;

use state::AppState;

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
        limiter: Arc::new(ratelimit::Limiter::new(cfg.ratelimit.clone())),
        sessions: Arc::new(session::Sessions::new()),
        cfg: Arc::new(cfg.clone()),
    };

    let app = routes::router(state);
    // Every asset URL in the templates lives under /static — styles.css,
    // js/, img/, thirdparty/ and the favicon — so one mount serves the whole
    // `static/` tree. The catalog's `/{section}/{id}` detail route cannot
    // shadow these: its two-segment shape never overlaps a /static prefix,
    // and the prefix mount wins the match anyway.
    let app = app.nest_service("/static", ServeDir::new("static"));

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
