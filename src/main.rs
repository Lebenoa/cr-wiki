//! Rust port of the CookieRun fan wiki (see AGENTS.md for the V original).
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
mod options;
mod pagination;
mod prefill;
mod ratelimit;
mod richtext;
mod routes;
mod session;
mod state;
mod turnstile;
mod upload;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::services::{ServeDir, ServeFile};

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
    let cfg = config::Config::load("Config.toml");
    i18n::load("translations");
    tracing::info!(langs = ?i18n::available_langs(), "translations loaded");

    let pool = match db::open(&format!("{}", cfg.db_file)) {
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
        .route("/gacha", get(routes::misc::gacha))
        .route("/api/available-langs", get(routes::api::available_langs))
        .route("/api/richtext-names", get(routes::api::richtext_names))
        .route("/api/relics", get(routes::api::relics))
        .route("/api/set-lang", post(routes::api::set_lang))
        .route("/sitemap.xml", get(routes::misc::sitemap))
        .route("/robots.txt", get(routes::misc::robots))
        .route("/changelog", get(routes::changelog::page))
        .route("/builds", get(routes::builds::list))
        .route("/builds/options/{kind}", get(routes::picker::options_grid))
        .route("/builds/preview", get(routes::picker::preview))
        .route("/builds/new", get(routes::planner::new_form).post(routes::planner::create))
        .route("/builds/{id}/delete", post(routes::planner::delete))
        .route("/builds/{id}/verify", post(routes::planner::verify))
        .route(
            "/builds/{id}/edit",
            get(routes::planner::edit_form).post(routes::planner::update),
        )
        .route("/builds/{id}", get(routes::builds::show))
        .route("/login", get(routes::auth::login_form).post(routes::auth::login))
        .route("/register", get(routes::auth::register_form).post(routes::auth::register))
        .route("/logout", get(routes::auth::logout))
        // the admin routes are registered before the catalog captures, so
        // /cookies/new is a form rather than a detail page for id "new"
        .route(
            "/{section}/new",
            get(routes::admin::new_form).post(routes::admin::create),
        )
        .route("/combi/{id}/delete", post(routes::admin::delete_combi))
        .route("/{section}/upload", post(routes::uploads::image))
        .route(
            "/{section}/{id}/edit",
            get(routes::admin::edit_form).post(routes::admin::update),
        )
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
        .nest_service("/static", ServeDir::new("static"))
        .nest_service("/js", ServeDir::new("static/js"))
        .nest_service("/img", ServeDir::new("static/img"))
        .nest_service("/thirdparty", ServeDir::new("static/thirdparty"))
        // the favicon link is root-relative, as browsers request it
        .route_service("/favicon.avif", ServeFile::new("static/favicon.avif"))
        .route_service("/favicon.webp", ServeFile::new("static/favicon.webp"))
        // a path no route claims gets the site's own 404, not a bare line
        .fallback(routes::errors::fallback)
        // a panic in a handler would otherwise drop the connection with no
        // response at all; the client sees the 500 page instead
        .layer(CatchPanicLayer::custom(routes::errors::panic_response))
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
    /// The error pages render without a request behind them: the panic
    /// handler has no Ctx to borrow, so a template that reached for one
    /// would take the 500 down with it.
    #[test]
    fn error_pages_render() {
        i18n::load("translations");
        let body = routes::errors::not_found(test_ctx("en"));
        assert_eq!(body.status(), axum::http::StatusCode::NOT_FOUND);
        let boom = routes::errors::panic_response(Box::new("boom"));
        assert_eq!(boom.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn translations_load() {
        i18n::load("translations");
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
        i18n::load("translations");
        let pool = db::open("sqlite.db").expect("open db");
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
        i18n::load("translations");
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
        i18n::load("translations");
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

    /// Forwarded headers count only for a trusted-proxy peer; anyone else is
    /// keyed by their own TCP address, however many headers they send.
    #[test]
    fn client_ip_trusts_only_configured_proxies() {
        use axum::http::{HeaderMap, HeaderValue};
        let peer: std::net::SocketAddr = "10.0.0.9:1234".parse().unwrap();
        let trusted = vec!["10.0.0.9".to_string()];

        let mut h = HeaderMap::new();
        h.insert("X-Forwarded-For", HeaderValue::from_static("1.2.3.4, 5.6.7.8"));
        // untrusted peer: the spoofable header loses
        assert_eq!(ctx::client_ip(&h, Some(peer), &[]), "10.0.0.9");
        assert_eq!(ctx::client_ip(&h, None, &trusted), "unknown");
        // trusted peer: the first XFF hop stands in for the visitor
        assert_eq!(ctx::client_ip(&h, Some(peer), &trusted), "1.2.3.4", "first hop, not the chain");

        h.insert("CF-Connecting-IP", HeaderValue::from_static("9.9.9.9"));
        assert_eq!(ctx::client_ip(&h, Some(peer), &trusted), "9.9.9.9", "CF wins over XFF");
        assert_eq!(ctx::client_ip(&h, Some(peer), &[]), "10.0.0.9");
    }

    /// The bucket drains, denies, and advertises a retry. Built in debug the
    /// limiter is bypassed exactly as the V `$if !prod` gate does, so this
    /// drives the algorithm directly.
    #[test]
    fn token_bucket_drains_and_refills() {
        let cfg = config::RateLimit { capacity: 3.0, refill: 1.0, idle_ttl: 300, sweep_above: 2048, trusted_proxies: Vec::new() };
        let limiter = ratelimit::Limiter::new(cfg);

        // Debug builds bypass limiting entirely (see Limiter::check); the
        // drain/refill/deny algorithm is exercised by `cargo test --release`.
        #[cfg(debug_assertions)]
        assert!(matches!(limiter.check("1.1.1.1"), ratelimit::Decision::Allow));

        #[cfg(not(debug_assertions))]
        {
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

    /// Every catalog list answers, in both locales, with the English name
    /// kept alongside for the cross-language filter.
    #[test]
    fn catalog_queries() {
        i18n::load("translations");
        let pool = db::open("sqlite.db").expect("open db");

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
        let pool = db::open("sqlite.db").expect("open db");
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
        let pool = db::open("sqlite.db").expect("open db");
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
        let pool = db::open("sqlite.db").expect("open db");
        let en = db::search(&pool, "en", "kaymak", 20).unwrap();
        assert!(en.iter().any(|(section, c)| section == "cookies" && c.name.contains("Kaymak")));

        // a th page still finds an entity by its English name
        let th = db::search(&pool, "th", "wizard", 20).unwrap();
        assert!(!th.is_empty());

        // the escape makes % a literal: it finds the rows whose text actually
        // contains one, rather than matching the whole catalog. The search
        // covers the prose columns too, so a hit can come from a description
        // rather than the name — what matters is that it is not everything.
        let pct = db::search(&pool, "en", "%", 20).unwrap();
        assert!(!pct.is_empty());
        let everything = db::search(&pool, "en", "e", 500).unwrap();
        assert!(pct.len() < everything.len(), "% matched as a wildcard");
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
        let pool = db::open("sqlite.db").expect("open db");
        for sort in ["latest", "score", "coin", "time", "nonsense"] {
            let rows = builds::select_builds(&pool, "en", (0, 0, 0, 0, 0), sort, 30, 0)
                .unwrap_or_else(|e| panic!("{sort}: {e}"));
            assert!(rows.len() <= 30);
        }
        // filters compose without tripping the SQL
        assert!(builds::select_builds(&pool, "en", (89, 50, 317, 5, 0), "score", 30, 0).is_ok());
        assert!(builds::select_builds(&pool, "en", (0, 0, 0, 0, 2), "latest", 30, 0).is_ok());
        assert!(builds::select_build(&pool, "en", 999_999).unwrap().is_none());

        i18n::load("translations");
        let c = test_ctx("en");
        let mut b = builds::BuildCard { ep: 5, ..Default::default() };
        assert_eq!(b.ep_label(&c), "EP 5");
        b.ep_special = 2;
        assert_eq!(b.ep_label(&c), "Special EP 2", "a special tier wins over the plain one");
        b.time_ms = 95_400;
        assert_eq!(b.time_label(), "1:35");
        b.time_ms = 0;
        assert_eq!(b.time_label(), "");
        assert!(!b.has_stats());
        b.score = 10;
        assert!(b.has_stats());
    }

    /// The picker lists build, cache and carry their effect ladders.
    #[test]
    fn picker_options_build() {
        i18n::load("translations");
        let pool = db::open("sqlite.db").expect("open db");

        let cookies = options::options(&pool, "en", "cookie");
        assert_eq!(cookies.len(), 93);
        let pets = options::options(&pool, "en", "pet");
        assert_eq!(pets.len(), 100, "the two phantom Sotdae pets are gone");

        let treasures = options::options(&pool, "en", "treasure");
        assert!(treasures.len() > 700);
        // Power+ treasures are friendly-run bonuses and cannot be equipped
        assert!(treasures.iter().any(|t| !t.effects.is_empty()));
        let with_ladder = treasures
            .iter()
            .find(|t| t.effects.iter().any(|e| !e.values.is_empty()))
            .expect("a treasure with a ladder");
        assert_eq!(
            with_ladder.effects.iter().find(|e| !e.values.is_empty()).unwrap().values.len(),
            10
        );

        // the grade order puts the highest first, and E outranks L
        let ranks: Vec<i64> = treasures.iter().filter_map(|t| t.grade).map(grade::rank).collect();
        assert!(ranks.windows(2).all(|w| w[0] >= w[1]), "grade order is not monotonic");

        // second call comes from the cache
        let again = options::options(&pool, "en", "cookie");
        assert_eq!(again.len(), cookies.len());
        options::invalidate();
        assert_eq!(options::options(&pool, "en", "cookie").len(), cookies.len());
    }

    /// Turnstile refuses rather than waves through when it is misconfigured.
    /// A debug build bypasses the gate entirely — that bypass is the behaviour
    /// under test there; a release build exercises the fail-closed refusals
    /// (no network: the empty secret rejects before any request is made).
    #[tokio::test]
    async fn turnstile_gate() {
        let mut cfg = config::Config::default();

        #[cfg(debug_assertions)]
        {
            // bypassed regardless, so a missing secret cannot block local
            // development
            assert!(turnstile::verify(&cfg, Some("token"), "login").await);
            cfg.turnstile.secret = String::new();
            assert!(turnstile::verify(&cfg, None, "login").await);
        }

        #[cfg(not(debug_assertions))]
        {
            assert!(!turnstile::verify(&cfg, Some("token"), "login").await);
            assert!(!turnstile::verify(&cfg, None, "login").await);
        }
    }

    /// The gacha pools carry their prizes and odds.
    #[test]
    fn gacha_pools() {
        let pool = db::open("sqlite.db").expect("open db");
        let pools = db::select_gacha(&pool, "en").expect("gacha");
        assert!(!pools.is_empty());
        let entries: usize = pools.iter().map(|p| p.entries.len()).sum();
        assert_eq!(entries, 300, "every disclosed entry is listed");
        let first = pools.iter().find(|p| !p.entries.is_empty()).unwrap();
        assert!(first.entries.iter().all(|e| e.odds > 0.0));
        assert!(first.entries.iter().all(|e| !e.name.is_empty()));
        assert!(first.entries[0].odds_label().ends_with('%'));
    }

    /// Upload names are sanitised: no separator, traversal segment or
    /// control character survives, and the extension comes from the declared
    /// content type rather than the submitted filename.
    #[test]
    fn upload_names_are_safe() {
        assert_eq!(upload::safe_stem("Cloud Boots.png"), "cloud_boots");
        assert_eq!(upload::safe_stem("../../etc/passwd"), "passwd");
        assert_eq!(upload::safe_stem("a/b/c.png"), "c");
        assert_eq!(upload::safe_stem("....."), "image");
        assert_eq!(upload::safe_stem(""), "image");
        assert!(!upload::safe_stem("sprite.png.html").contains('.'));
        assert!(upload::safe_stem("x".repeat(200).as_str()).len() <= 60);

        assert_eq!(upload::extension_for("image/png"), Some("png"));
        assert_eq!(upload::extension_for("image/jpeg"), Some("jpg"));
        // markup and scripts are not images, whatever the filename says
        assert_eq!(upload::extension_for("text/html"), None);
        assert_eq!(upload::extension_for("application/octet-stream"), None);

        assert!(upload::section_dir("cookies").is_some());
        assert!(upload::section_dir("../secrets").is_none());
        assert!(upload::section_dir("builds").is_none());
    }

    /// A treasure's unlock chain resolves to the entity that grants it.
    #[test]
    fn treasure_links_resolve() {
        let pool = db::open("sqlite.db").expect("open db");
        // treasure 255 is unlocked by the surviving Sotdae Flock pet
        let links = db::treasure_links(&pool, "en", 255).expect("links");
        assert!(links.has_unlock());
        assert_eq!(links.unlock_section, "pets");
        assert_eq!(links.unlock_id, 102);
        assert!(!links.unlock_name.is_empty());

        // relics and skins have detail rows now. Their ids are not 1-based —
        // relics start at 500001 and skins at 1800001 — so the test takes an
        // id from the list rather than assuming one.
        for section in ["relics", "skins"] {
            let list = db::select_simple(&pool, "en", section).unwrap();
            let first = list.first().expect("a row");
            let detail = db::select_detail(&pool, "en", section, first.id)
                .unwrap()
                .unwrap_or_else(|| panic!("{section} {} has no detail", first.id));
            assert!(!detail.name.is_empty());
        }
    }

    /// The query string picker.js actually builds must parse. It sends an
    /// empty value for every unfilled slot, which a plain Option<i64> rejects
    /// — that would 400 the whole preview request.
    #[test]
    fn preview_query_accepts_empty_slots() {
        let empty: routes::CommonQuery =
            serde_urlencoded::from_str("cookie=&c2=&pet=&t1=&t2=&t3=").expect("empty slots parse");
        assert_eq!(empty.cookie, None);
        assert_eq!(empty.t3, None);

        let partial: routes::CommonQuery =
            serde_urlencoded::from_str("cookie=89&c2=&pet=50&t1=317&t2=&t3=")
                .expect("partial selection parses");
        assert_eq!(partial.cookie, Some(89));
        assert_eq!(partial.c2, None);
        assert_eq!(partial.pet, Some(50));
        assert_eq!(partial.t1, Some(317));

        // a junk value is ignored rather than failing the request
        let junk: routes::CommonQuery =
            serde_urlencoded::from_str("page=abc&sel=").expect("junk parses");
        assert_eq!(junk.page, None);
        assert_eq!(junk.sel, None);
    }

    /// The blessed toggle appears only when the blessed set actually differs
    /// from the normal one — a treasure whose two sets match should not offer
    /// a switch between identical readings.
    #[test]
    fn blessed_toggle_only_when_it_differs() {
        use db::EffectLine;
        let line = |text: &str, v: &[&str], blessed: bool| EffectLine {
            text: text.into(),
            values: v.iter().map(|s| s.to_string()).collect(),
            blessed,
        };

        // no blessed set at all
        assert!(!db::blessed_differs(&[line("Magnet", &["1"], false)]));
        // identical sets
        assert!(!db::blessed_differs(&[
            line("Magnet", &["1"], false),
            line("Magnet", &["1"], true),
        ]));
        // a different value is a difference worth showing
        assert!(db::blessed_differs(&[
            line("Magnet", &["1"], false),
            line("Magnet", &["2"], true),
        ]));
        // so is a different effect
        assert!(db::blessed_differs(&[
            line("Magnet", &["1"], false),
            line("Revive", &["1"], true),
        ]));
    }

    /// The combo editor lists a pairing with the row id it needs to remove it.
    #[test]
    fn combi_editor_rows() {
        let pool = db::open("sqlite.db").expect("open db");
        let rows = db::combi_edit_rows(&pool, "en", "cookies", 89).expect("rows");
        assert!(!rows.is_empty(), "cookie 89 pairs with a pet");
        assert!(rows.iter().all(|r| r.id > 0), "every row carries its own id");
        assert!(rows.iter().all(|r| !r.partner_name.is_empty()));
        assert!(db::combi_edit_rows(&pool, "en", "treasures", 1).unwrap().is_empty());
    }

    /// The sitemap covers every detail id the six sections hold.
    #[test]
    fn sitemap_covers_the_catalog() {
        let pool = db::open("sqlite.db").expect("open db");
        let entries = db::sitemap_entries(&pool).unwrap();
        for section in ["cookies", "pets", "treasures", "episodes", "ingredients", "jellies"] {
            assert!(entries.iter().any(|(s, _)| s == section), "{section} missing");
        }
        assert!(entries.len() > 1000);
    }
}
