//! The HTTP module's interface: `router(state)` is the seam prod serves
//! through and tests drive with the tower oneshot adapter — same layer
//! order, same fallback, no second assembly to drift.
//!
//! One caveat is deliberate: the static mounts are absent here because the
//! dev working directory they read does not exist in the test process; main
//! adds them around the same routes.

use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Deserializer};
use tower_http::catch_panic::CatchPanicLayer;

use crate::middleware;
pub mod admin;
pub mod api;
pub mod auth;
pub mod builds;
pub mod catalog;
pub mod changelog;
pub mod detail;
pub mod errors;
pub mod misc;
pub mod picker;
pub mod planner;
pub mod uploads;

/// Router assembly in one function: the layer order (rate limit, then
/// locale, then routes) is the thing worth seeing whole.
#[allow(clippy::too_many_lines)] // splitting it would hide the middleware order
pub fn router() -> Router {
    Router::new()
        .route("/", get(misc::index))
        .route("/search", get(misc::search))
        .route("/gacha", get(misc::gacha))
        .route("/api/available-langs", get(api::available_langs))
        .route("/api/richtext-names", get(api::richtext_names))
        .route("/api/relics", get(api::relics))
        .route("/api/set-lang", post(api::set_lang))
        .route("/sitemap.xml", get(misc::sitemap))
        .route("/robots.txt", get(misc::robots))
        .route("/changelog", get(changelog::page))
        .route("/builds", get(builds::list))
        .route("/builds/options/{kind}", get(picker::options_grid))
        .route("/builds/preview", get(picker::preview))
        .route("/builds/new", get(planner::new_form).post(planner::create))
        .route("/builds/{id}/delete", post(planner::delete))
        .route("/builds/{id}/verify", post(planner::verify))
        .route(
            "/builds/{id}/edit",
            get(planner::edit_form).post(planner::update),
        )
        .route("/builds/{id}", get(builds::show))
        .route("/login", get(auth::login_form).post(auth::login))
        .route("/register", get(auth::register_form).post(auth::register))
        .route("/logout", get(auth::logout))
        .route("/revoke-sessions", post(auth::revoke_sessions))
        // the admin routes are registered before the catalog captures, so
        // /cookies/new is a form rather than a detail page for id "new"
        .route("/{section}/new", get(admin::new_form).post(admin::create))
        .route("/{section}/{id}/combi/{row_id}/delete", post(admin::delete_combi))
        .route("/{section}/upload", post(uploads::image))
        .route(
            "/{section}/{id}/edit",
            get(admin::edit_form).post(admin::update),
        )
        // one handler per shape rather than per section: the section is a
        // path capture, so eight lists share a function
        .route("/{section}", get(catalog::list))
        .route("/{section}/{id}", get(detail::show))
        .layer(axum::middleware::from_fn(middleware::context))
        .layer(axum::middleware::from_fn(middleware::rate_limit))
        // a path no route claims gets the site's own 404, not a bare line
        .fallback(errors::fallback)
        // a panic in a handler would otherwise drop the connection with no
        // response at all; the client sees the 500 page instead
        .layer(CatchPanicLayer::custom(errors::panic_response))
}

/// Treats an empty parameter as absent.
///
/// picker.js builds the preview URL as `cookie=&c2=&pet=&t1=&t2=&t3=`, with
/// an empty value for every slot that has no pick yet. Plain `Option<i64>`
/// fails on the empty string, which fails the whole extractor and 400s the
/// request — so an unfilled planner would get no preview at all.
fn empty_as_none<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(d)?;
    Ok(raw
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok()))
}

/// Query parameters the pages share. `lang` is resolved by the middleware
/// (see `ctx::resolve_lang`) rather than read here; it stays in the struct so
/// serde does not reject the parameter.
#[derive(Debug, Default, Deserialize)]
pub struct CommonQuery {
    #[allow(dead_code)]
    pub lang: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub page: Option<i64>,
    pub q: Option<String>,
    pub tab: Option<String>,
    pub sort: Option<String>,
    pub ep: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub cookie: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub pet: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub treasure: Option<i64>,
    /// the option already in the slot the picker is opening for
    #[serde(default, deserialize_with = "empty_as_none")]
    pub sel: Option<i64>,
    /// the other half of the combo pair, whose partners float to the top
    #[serde(default, deserialize_with = "empty_as_none")]
    pub partner: Option<i64>,
    /// the planner's live preview posts the whole selection
    #[serde(default, deserialize_with = "empty_as_none")]
    pub c2: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t1: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t2: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t3: Option<i64>,
    /// which entity list the rich-text picker wants
    pub kind: Option<String>,
    /// where /api/set-lang should return to
    pub next: Option<String>,
    /// the build list's free-text author filter
    #[serde(default)]
    pub author: Option<String>,
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    use crate::ratelimit::Limiter;
    use crate::session::Sessions;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    /// A connection handle no query ever uses: the non-DB routes below never
    /// dereference it, the DB-outage test counts on its error, and the real
    /// reads are covered by the CR_SURREAL_URL-gated tests in the modules
    /// they belong to. This build carries no embedded engine by design.
    fn state() -> &'static AppState {
        crate::i18n::load("translations");
        crate::state::init(AppState {
            db: surrealdb::Surreal::init(),
            limiter: Limiter::new(crate::config::RateLimit::default()),
            sessions: Sessions::new(),
            cfg: crate::config::Config::default(),
        });
        crate::state::state()
    }

    async fn get(uri: &str) -> axum::response::Response {
        router()
            .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
            .await
            .expect("infallible")
    }

    /// Routes that never read the catalog answer 200 through the real
    /// middleware order — rate limit, then locale, then handler — and an
    /// unknown section 404s at the typed gate before any query runs.
    /// (doc-backtick note: `Section::ALL` drives the catalog list loop)
    #[tokio::test]
    async fn non_db_routes_answer_through_the_middleware_order() {
        state();
        for uri in ["/robots.txt", "/changelog", "/login", "/register"] {
            let res = get(uri).await;
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
        }
        let miss = get("/no-such-section").await;
        assert_eq!(miss.status(), StatusCode::NOT_FOUND);
    }

    /// A database failure answers the site's own 500 page through `AppError`,
    /// never an empty page that would read as "no data": this handle is
    /// unconnected, which is exactly the outage an unreachable server causes.
    #[tokio::test]
    async fn db_failure_is_the_sites_500() {
        state();
        let res = get("/cookies").await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// A section edit form is admin-only. The request carries a loopback
    /// `ConnectInfo`, so in a debug build the bypass admits it; a release
    /// build here would 404. Without the peer address it must 404 even in
    /// debug: no peer address fails closed.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn admin_form_reachable_only_from_a_loopback_peer() {
        use axum::extract::ConnectInfo;
        state();
        let req = Request::get("/cookies/new")
            .extension(ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                4242,
            ))))
            .body(Body::empty())
            .expect("request");
        let res = router().oneshot(req).await.expect("infallible");
        assert_eq!(res.status(), StatusCode::OK);

        let req = Request::get("/cookies/new")
            .body(Body::empty())
            .expect("request");
        let res = router().oneshot(req).await.expect("infallible");
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    /// Every frame template still renders through a real request: the
    /// canonical tag and the hreflang alternates come off the same helper,
    /// so they cannot disagree.
    #[tokio::test]
    async fn changelog_page_is_branded_and_localized() {
        state();
        let res = get("/changelog").await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("<ol id=\"changelog-list\""));
        assert!(html.contains("rel=\"canonical\" href=\"http://localhost:6785/changelog\""));
        assert!(html.contains("hreflang=\"th\" href=\"http://localhost:6785/changelog?lang=th\""));
    }

    /// The query string picker.js actually builds must parse. It sends an
    /// empty value for every unfilled slot, which a plain Option<i64> rejects
    /// — that would 400 the whole preview request.
    #[test]
    fn preview_query_accepts_empty_slots() {
        let empty: CommonQuery =
            serde_urlencoded::from_str("cookie=&c2=&pet=&t1=&t2=&t3=").expect("empty slots parse");
        assert_eq!(empty.cookie, None);
        assert_eq!(empty.t3, None);

        let partial: CommonQuery =
            serde_urlencoded::from_str("cookie=89&c2=&pet=50&t1=317&t2=&t3=")
                .expect("partial selection parses");
        assert_eq!(partial.cookie, Some(89));
        assert_eq!(partial.c2, None);
        assert_eq!(partial.pet, Some(50));
        assert_eq!(partial.t1, Some(317));

        // a junk value is ignored rather than failing the request
        let junk: CommonQuery = serde_urlencoded::from_str("page=abc&sel=").expect("junk parses");
        assert_eq!(junk.page, None);
        assert_eq!(junk.sel, None);
    }
}
