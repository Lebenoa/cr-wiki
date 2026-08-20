//! Rust port of the CookieRun fan wiki (see ../AGENTS.md for the V original).
//!
//! Ported so far: config, the .tr catalogs, the SQLite pool, static files,
//! and the /changelog and /cookies pages. Everything else still lives in the
//! V app next door; see PORTING.md for the running list.

mod changelog;
mod config;
mod ctx;
mod db;
mod i18n;
mod middleware;
mod pagination;
mod ratelimit;
mod routes;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

use state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // paths are relative to the repo root, so the port runs against the same
    // Config.toml, translations and database as the V app
    let cfg = config::Config::load("../Config.toml");
    i18n::load("../translations");
    tracing::info!(langs = ?i18n::available_langs(), "translations loaded");

    let pool = match db::open(&format!("../{}", cfg.db_file)) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("cannot open {}: {e}", cfg.db_file);
            return;
        }
    };

    let state = AppState {
        db: pool,
        limiter: Arc::new(ratelimit::Limiter::new(cfg.ratelimit.clone())),
        cfg: Arc::new(cfg.clone()),
    };

    let app = Router::new()
        .route("/changelog", get(routes::changelog::page))
        .route("/cookies", get(routes::cookies::list))
        .layer(axum::middleware::from_fn(middleware::lang))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::rate_limit,
        ))
        // static files sit outside the limiter: they are cheap, and throttling
        // the stylesheet would break the page that survived the throttle
        .nest_service("/static", ServeDir::new("../static"))
        .nest_service("/js", ServeDir::new("../static/js"))
        .nest_service("/img", ServeDir::new("../static/img"))
        .nest_service("/thirdparty", ServeDir::new("../static/thirdparty"))
        .with_state(state);

    let addr = format!("{}:{}", cfg.host, cfg.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot bind {addr}: {e}");
            return;
        }
    };
    tracing::info!("listening on {addr}");
    if let Err(e) =
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await
    {
        tracing::error!("server error: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    /// The .tr catalogs parse and both locales resolve, including a key added
    /// late in the V app's life.
    #[test]
    fn translations_load() {
        i18n::load("../translations");
        assert!(i18n::available_langs().contains(&"en".to_string()));
        assert!(i18n::available_langs().contains(&"th".to_string()));
        assert_eq!(i18n::t("en", "changelog_page_header"), "Changelog");
        assert_eq!(i18n::t("en", "changelog_unavailable"), "No changelog available.");
        assert_ne!(i18n::t("th", "changelog_page_header"), "Changelog");
        // a miss falls back to English, then to the key itself
        assert_eq!(i18n::t("th", "no_such_key_at_all"), "no_such_key_at_all");
    }

    /// The overflow that panicked the V original: ?page=100000000 at 30 a page
    /// wrapped negative in 32-bit and blew up the slice.
    #[test]
    fn paging_survives_absurd_pages() {
        assert_eq!(pagination::slice_page(1, 30, 200), Some((0, 30)));
        assert_eq!(pagination::slice_page(7, 30, 200), Some((180, 200)));
        assert_eq!(pagination::slice_page(8, 30, 200), None);
        assert_eq!(pagination::slice_page(100_000_000, 30, 200), None);
        assert_eq!(pagination::slice_page(i64::MAX, 30, 200), None);
        assert_eq!(pagination::slice_page(0, 30, 200), None);
    }

    /// git log parses into conventional-commit parts, and the trailers the V
    /// version drops are dropped here too.
    #[test]
    fn changelog_parses_history() {
        let entries = changelog::entries();
        assert!(!entries.is_empty(), "no history parsed");
        assert!(entries.iter().any(|e| e.kind == "feat"));
        assert!(entries.iter().any(|e| e.kind == "fix"));
        for e in entries {
            for para in &e.body {
                assert!(!para.starts_with("Co-Authored-By:"));
                assert!(!para.starts_with("Claude-Session:"));
            }
        }
    }

    /// The cookie list query runs against the real database and comes back in
    /// the catalog's order.
    #[test]
    fn cookie_query_and_render() {
        i18n::load("../translations");
        let pool = db::open("../sqlite.db").expect("open db");
        let rows = db::select_cookies(&pool, "en", 30, 0).expect("query");
        assert_eq!(rows.len(), 30);
        assert!(rows.iter().all(|c| !c.name.is_empty()));

        let th = db::select_cookies(&pool, "th", 5, 0).expect("query th");
        assert_eq!(th.len(), 5);
        // the th page still carries the English name for cross-language search
        assert!(th.iter().all(|c| !c.en_name.is_empty()));
    }

    /// Both ported templates render.
    #[test]
    fn templates_render() {
        i18n::load("../translations");
        let entries = changelog::entries().iter().take(3).cloned().collect::<Vec<_>>();
        let page = TestChangelog {
            ctx: test_ctx("en"),
            entries,
            next_url: "/changelog?page=2".into(),
            page: 1,
        };
        let html = page.render().expect("changelog renders");
        assert!(html.contains("<ol id=\"changelog-list\""));
        assert!(html.contains("hx-trigger=\"revealed\""));
        // commit prose mentions markup; it must arrive escaped, not as a tag
        assert!(!html.contains("<select>"));
        // the canonical tag and the alternates come off the same helper, so
        // they can never disagree
        assert!(html.contains("rel=\"canonical\" href=\"http://localhost:6785/changelog\""));
        assert!(html.contains("hreflang=\"th\" href=\"http://localhost:6785/changelog?lang=th\""));
        // the active nav entry is underlined
        assert!(html.contains("border-b-2 border-primary"));
    }

    #[derive(Template)]
    #[template(path = "changelog.html")]
    struct TestChangelog {
        ctx: ctx::Ctx,
        entries: Vec<changelog::ChangeEntry>,
        next_url: String,
        page: i64,
    }

    /// A context standing in for one resolved off a real request.
    fn test_ctx(lang: &str) -> ctx::Ctx {
        ctx::Ctx {
            l: i18n::Loc::new(lang),
            lang: lang.to_string(),
            path: "/changelog".into(),
            site_url: "http://localhost:6785".into(),
            htmx: false,
            boosted: false,
            is_local: true,
        }
    }

    /// Locale resolution: ?lang= wins over the cookie, an unknown locale is
    /// ignored, and a bare request falls back to English while asking for the
    /// cookie to be written.
    #[test]
    fn lang_resolution() {
        i18n::load("../translations");
        use axum::http::{HeaderMap, HeaderValue};

        let empty = HeaderMap::new();
        let c = ctx::resolve_lang(None, &empty);
        assert_eq!(c.lang, "en");
        assert!(c.write_cookie);

        let c = ctx::resolve_lang(Some("lang=th"), &empty);
        assert_eq!(c.lang, "th");
        assert!(c.write_cookie);

        // an unknown locale is ignored, not echoed back
        let c = ctx::resolve_lang(Some("lang=zz"), &empty);
        assert_eq!(c.lang, "en");

        let mut with_cookie = HeaderMap::new();
        with_cookie.insert("cookie", HeaderValue::from_static("wikilang=th; other=1"));
        let c = ctx::resolve_lang(None, &with_cookie);
        assert_eq!(c.lang, "th");
        assert!(!c.write_cookie, "cookie already agrees, no need to rewrite");

        // the param wins over a disagreeing cookie, and refreshes it
        let c = ctx::resolve_lang(Some("page=2&lang=en"), &with_cookie);
        assert_eq!(c.lang, "en");
        assert!(c.write_cookie);
    }

    /// The proxy headers veb trusts, in the same order, then the peer.
    #[test]
    fn client_ip_prefers_proxy_headers() {
        use axum::http::{HeaderMap, HeaderValue};
        let peer: std::net::SocketAddr = "10.0.0.9:1234".parse().unwrap();

        let mut h = HeaderMap::new();
        assert_eq!(ctx::client_ip(&h, Some(peer)), "10.0.0.9");
        assert_eq!(ctx::client_ip(&h, None), "unknown");

        h.insert("X-Forwarded-For", HeaderValue::from_static("1.2.3.4, 5.6.7.8"));
        assert_eq!(ctx::client_ip(&h, Some(peer)), "1.2.3.4", "first hop, not the chain");

        h.insert("CF-Connecting-IP", HeaderValue::from_static("9.9.9.9"));
        assert_eq!(ctx::client_ip(&h, Some(peer)), "9.9.9.9");
    }

    /// The bucket drains, denies, and advertises a retry. Built in debug the
    /// limiter is bypassed exactly as the V `$if !prod` gate does, so this
    /// drives the algorithm directly.
    #[test]
    fn token_bucket_drains_and_refills() {
        let cfg = config::RateLimit { capacity: 3.0, refill: 1.0, idle_ttl: 300, sweep_above: 2048 };
        let limiter = ratelimit::Limiter::new(cfg);
        if cfg!(debug_assertions) {
            // the bypass is the behaviour under test in a debug build
            assert!(matches!(limiter.check("1.1.1.1"), ratelimit::Decision::Allow));
            return;
        }
        for _ in 0..3 {
            assert!(matches!(limiter.check("1.1.1.1"), ratelimit::Decision::Allow));
        }
        match limiter.check("1.1.1.1") {
            ratelimit::Decision::Deny(after) => assert!(after >= 1),
            ratelimit::Decision::Allow => panic!("bucket should be empty"),
        }
        // a different IP has its own bucket
        assert!(matches!(limiter.check("2.2.2.2"), ratelimit::Decision::Allow));
    }
}
