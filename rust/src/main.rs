//! Rust port of the CookieRun fan wiki (see ../AGENTS.md for the V original).
//!
//! Ported so far: config, the .tr catalogs, the SQLite pool, static files,
//! and the /changelog and /cookies pages. Everything else still lives in the
//! V app next door; see PORTING.md for the running list.

mod builds;
mod changelog;
mod config;
mod ctx;
mod db;
mod grade;
mod i18n;
mod middleware;
mod pagination;
mod ratelimit;
mod richtext;
mod routes;
mod session;
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
        sessions: Arc::new(session::Sessions::new()),
        cfg: Arc::new(cfg.clone()),
    };

    let app = Router::new()
        .route("/", get(routes::misc::index))
        .route("/search", get(routes::misc::search))
        .route("/sitemap.xml", get(routes::misc::sitemap))
        .route("/robots.txt", get(routes::misc::robots))
        .route("/changelog", get(routes::changelog::page))
        .route("/builds", get(routes::builds::list))
        .route("/builds/{id}", get(routes::builds::show))
        .route("/login", get(routes::auth::login_form).post(routes::auth::login))
        .route("/register", get(routes::auth::register_form).post(routes::auth::register))
        .route("/logout", get(routes::auth::logout))
        // one handler per shape rather than per section: the section is a
        // path capture, so eight lists share a function
        .route("/{section}", get(routes::catalog::list))
        .route("/{section}/{id}", get(routes::detail::show))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::context,
        ))
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
            user: None,
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

    /// Every catalog list answers, in both locales, with the English name
    /// kept alongside for the cross-language filter.
    #[test]
    fn catalog_queries() {
        i18n::load("../translations");
        let pool = db::open("../sqlite.db").expect("open db");

        let cookies = db::select_cookies(&pool, "en", 30, 0).unwrap();
        assert_eq!(cookies.len(), 30);
        let pets = db::select_pets(&pool, "th", 30, 0).unwrap();
        assert_eq!(pets.len(), 30);
        assert!(pets.iter().all(|p| !p.en_name.is_empty()));

        // the tabs partition the treasures rather than overlapping
        let all = db::select_treasures(&pool, "en", "all", 30, 0).unwrap();
        let normal = db::select_treasures(&pool, "en", "normal", 30, 0).unwrap();
        let evo = db::select_treasures(&pool, "en", "evo", 30, 0).unwrap();
        assert_eq!(all.len(), 30);
        assert!(normal.iter().all(|t| !t.is_evolved));
        assert!(evo.iter().all(|t| t.is_evolved));

        for kind in ["episodes", "ingredients", "jellies", "skins", "relics"] {
            assert!(!db::select_simple(&pool, "en", kind).unwrap().is_empty(), "{kind} empty");
        }
    }

    /// Grade ordering follows grade_values, where E outranks L.
    #[test]
    fn grade_ordering() {
        assert_eq!(grade::slug(5), "s_plus");
        assert_eq!(grade::label(5), "S+");
        assert_eq!(grade::label(3), "A");
        assert!(grade::rank(0) > grade::rank(6), "E outranks L");
        assert!(grade::rank(6) > grade::rank(5), "L outranks S+");
        assert!(grade::rank(1) < grade::rank(2), "C is the lowest");
    }

    /// Detail rows carry the prose their kind has and nothing else.
    #[test]
    fn detail_rows() {
        let pool = db::open("../sqlite.db").expect("open db");
        let cookie = db::select_detail(&pool, "en", "cookies", 89).unwrap().expect("cookie 89");
        assert!(!cookie.name.is_empty());
        assert!(!cookie.abilities.is_empty());

        // a treasure has no abilities column, and effects come with ladders
        let treasure = db::select_detail(&pool, "en", "treasures", 317).unwrap().expect("treasure");
        assert!(treasure.abilities.is_empty());
        let effects = db::treasure_effects(&pool, "en", 317).unwrap();
        assert!(!effects.is_empty());
        assert!(effects.iter().all(|e| e.values.len() == 10), "ten levels per effect");

        assert!(db::select_detail(&pool, "en", "cookies", 99999).unwrap().is_none());
    }

    /// Rich text: links resolve with their sprite, colours are constrained,
    /// and everything else is escaped.
    #[test]
    fn richtext_renders() {
        let pool = db::open("../sqlite.db").expect("open db");
        let out = richtext::render(&pool, "en", "see [[89]] here");
        assert!(out.contains("href=\"/cookies/89\""));
        assert!(out.contains("<img src=\"/img/cookies/"));

        // an unresolvable ref stays literal
        let miss = richtext::render(&pool, "en", "[[cookie:99999999]]");
        assert!(miss.contains("[[cookie:99999999]]"));

        let colored = richtext::render(&pool, "en", "a {color:red}red{/color} word");
        assert!(colored.contains("<span style=\"color:red\">red</span>"));

        // an injection attempt is not a valid colour, so the whole thing
        // renders as text: no span is opened and the quotes come out escaped
        let bad = richtext::render(&pool, "en", "{color:red\" onclick=\"x}y{/color}");
        assert!(!bad.contains("<span style="), "{bad}");
        assert!(!bad.contains("onclick=\""), "{bad}");
        assert!(bad.contains("&quot;"), "{bad}");

        // pasted markup is escaped
        let script = richtext::render(&pool, "en", "<script>alert(1)</script>");
        assert!(!script.contains("<script>"));
        assert!(script.contains("&lt;script&gt;"));
    }

    /// Search matches the localized and the English name, and a wildcard in
    /// the query is a literal.
    #[test]
    fn search_matches_both_languages() {
        let pool = db::open("../sqlite.db").expect("open db");
        let en = db::search(&pool, "en", "kaymak", 20).unwrap();
        assert!(en.iter().any(|(section, c)| section == "cookies" && c.name.contains("Kaymak")));

        // a th page still finds an entity by its English name
        let th = db::search(&pool, "th", "wizard", 20).unwrap();
        assert!(!th.is_empty());

        // the escape makes % a literal: it finds the treasures actually named
        // with one, rather than matching the whole catalog
        let pct = db::search(&pool, "en", "%", 20).unwrap();
        assert!(!pct.is_empty());
        assert!(pct.iter().all(|(_, c)| c.name.contains('%')), "% matched as a wildcard");
        assert!(db::search(&pool, "en", "   ", 20).unwrap().is_empty());
    }

    /// Argon2 in PHC form: a hash verifies, a wrong password does not, and
    /// two hashes of the same password differ because the salt is fresh.
    #[test]
    fn password_hashing() {
        let hash = session::hash_password("correct horse").expect("hash");
        assert!(hash.starts_with("$argon2"), "PHC format, so V can read it: {hash}");
        assert!(session::verify_password("correct horse", &hash));
        assert!(!session::verify_password("wrong horse", &hash));
        let again = session::hash_password("correct horse").expect("hash");
        assert_ne!(hash, again, "salt must be fresh per hash");
        assert!(!session::verify_password("correct horse", "not-a-hash"));
    }

    /// A session round trip: start, read back, end.
    #[test]
    fn sessions_round_trip() {
        let sessions = session::Sessions::new();
        let key = sessions.start(session::SessionUser {
            id: 7,
            username: "tester".into(),
            is_admin: true,
        });
        let found = sessions.get(&key).expect("session");
        assert_eq!(found.username, "tester");
        assert!(found.is_admin);
        sessions.end(&key);
        assert!(sessions.get(&key).is_none());
        assert!(sessions.get("never-issued").is_none());
    }

    /// The build list runs, and its labels format the way the badges expect.
    /// The table is empty on this checkout, so this covers the query path and
    /// the formatting rather than row content.
    #[test]
    fn build_queries_and_labels() {
        let pool = db::open("../sqlite.db").expect("open db");
        for sort in ["latest", "score", "coin", "time", "nonsense"] {
            let rows = builds::select_builds(&pool, "en", (0, 0, 0, 0, 0), sort, 30, 0)
                .unwrap_or_else(|e| panic!("{sort}: {e}"));
            assert!(rows.len() <= 30);
        }
        // filters compose without tripping the SQL
        assert!(builds::select_builds(&pool, "en", (89, 50, 317, 5, 0), "score", 30, 0).is_ok());
        assert!(builds::select_builds(&pool, "en", (0, 0, 0, 0, 2), "latest", 30, 0).is_ok());
        assert!(builds::select_build(&pool, "en", 999_999).unwrap().is_none());

        let mut b = builds::BuildCard { ep: 5, ..Default::default() };
        assert_eq!(b.ep_label(), "EP 5");
        b.ep_special = 2;
        assert_eq!(b.ep_label(), "Special EP 2", "a special tier wins over the plain one");
        b.time_ms = 95_400;
        assert_eq!(b.time_label(), "1:35");
        b.time_ms = 0;
        assert_eq!(b.time_label(), "");
        assert!(!b.has_stats());
        b.score = 10;
        assert!(b.has_stats());
    }

    /// The sitemap covers every detail id the six sections hold.
    #[test]
    fn sitemap_covers_the_catalog() {
        let pool = db::open("../sqlite.db").expect("open db");
        let entries = db::sitemap_entries(&pool).unwrap();
        for section in ["cookies", "pets", "treasures", "episodes", "ingredients", "jellies"] {
            assert!(entries.iter().any(|(s, _)| s == section), "{section} missing");
        }
        assert!(entries.len() > 1000);
    }
}
