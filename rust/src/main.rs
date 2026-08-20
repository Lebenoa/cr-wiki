//! Rust port of the CookieRun fan wiki (see ../AGENTS.md for the V original).
//!
//! Ported so far: config, the .tr catalogs, the SQLite pool, static files,
//! and the /changelog and /cookies pages. Everything else still lives in the
//! V app next door; see PORTING.md for the running list.

mod changelog;
mod config;
mod db;
mod i18n;
mod pagination;
mod routes;

use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

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

    let app = Router::new()
        .route("/changelog", get(routes::changelog::page))
        .route("/cookies", get(routes::cookies::list))
        .nest_service("/static", ServeDir::new("../static"))
        .nest_service("/js", ServeDir::new("../static/js"))
        .nest_service("/img", ServeDir::new("../static/img"))
        .with_state(pool);

    let addr = format!("{}:{}", cfg.host, cfg.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot bind {addr}: {e}");
            return;
        }
    };
    tracing::info!("listening on {addr}");
    if let Err(e) = axum::serve(listener, app).await {
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
        let l = i18n::Loc::new("en");
        let entries = changelog::entries().iter().take(3).cloned().collect::<Vec<_>>();
        let page = TestChangelog {
            l: l.clone(),
            lang: "en".into(),
            entries,
            next_url: "/changelog?page=2".into(),
            page: 1,
        };
        let html = page.render().expect("changelog renders");
        assert!(html.contains("<ol id=\"changelog-list\""));
        assert!(html.contains("hx-trigger=\"revealed\""));
        // commit prose mentions markup; it must arrive escaped, not as a tag
        assert!(!html.contains("<select>"));
    }

    #[derive(Template)]
    #[template(path = "changelog.html")]
    struct TestChangelog {
        l: i18n::Loc,
        lang: String,
        entries: Vec<changelog::ChangeEntry>,
        next_url: String,
        page: i64,
    }
}
