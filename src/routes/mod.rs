//! The HTTP module's interface: `router(state)` is the seam prod serves
//! through and tests drive with the tower oneshot adapter — same layer
//! order, same fallback, no second assembly to drift.
//!
//! One caveat is deliberate: the static mounts are absent here because the
//! dev working directory they read does not exist in the test process; main
//! adds them around the same routes.

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Deserializer};
use tower_http::catch_panic::CatchPanicLayer;

use crate::middleware;
use crate::upload;
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
        // the reset pair sits with the other auth routes, well before the
        // catalog captures, so neither path can be read as a `/{section}/{id}`
        // detail page
        .route(
            "/forgot-password",
            get(auth::forgot_form).post(auth::forgot),
        )
        .route(
            "/account/email",
            get(auth::account_email_form).post(auth::update_account_email),
        )
        .route("/reset-password", get(auth::reset_form).post(auth::reset))
        // GET logout with the SameSite=Lax session cookie lets any
        // cross-site link end the session: Lax still rides a top-level GET
        // navigation, so a linked /logout is a forced logout. POST only —
        // Lax never sends the cookie on a cross-site POST, so a foreign
        // page cannot forge it — matching /revoke-sessions below.
        .route("/logout", post(auth::logout))
        .route("/revoke-sessions", post(auth::revoke_sessions))
        // the admin routes are registered before the catalog captures, so
        // /cookies/new is a form rather than a detail page for id "new"
        .route("/{section}/new", get(admin::new_form).post(admin::create))
        .route(
            "/{section}/{id}/combi/{row_id}/delete",
            post(admin::delete_combi),
        )
        // the upload ceiling is declared here, not left to axum's 2 MB
        // default: the handler's own check bounds a single field at
        // MAX_BYTES, so the request limit only needs to cover the
        // multipart envelope on top
        .route(
            "/{section}/upload",
            post(uploads::image).layer(DefaultBodyLimit::max(
                upload::MAX_BYTES.saturating_add(64 * 1024),
            )),
        )
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
        // outermost, so even the panic page carries the baseline headers
        .layer(axum::middleware::from_fn(middleware::security_headers))
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
        for uri in [
            "/robots.txt",
            "/changelog",
            "/login",
            "/register",
            "/reset-password",
        ] {
            let res = get(uri).await;
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
        }
        let miss = get("/no-such-section").await;
        assert_eq!(miss.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn account_menu_links_to_recovery_email_settings() {
        let app = state();
        let key = app.sessions.start(crate::session::SessionUser {
            id: 7,
            username: "tester".to_string(),
            is_admin: false,
        });
        let res = router()
            .oneshot(
                Request::get("/changelog")
                    .header("cookie", format!("CRSESSID={key}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("infallible");
        let menu = text(res).await;
        assert!(menu.contains("href=\"/account/email\""), "{menu}");
        assert!(menu.contains("Recovery email"), "{menu}");
    }

    #[tokio::test]
    async fn account_email_route_requires_an_authenticated_session() {
        state();
        let res = get("/account/email").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        let res = post(
            "/account/email",
            "email=ginger%40cookie.run&password=longenough",
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
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

    async fn post(uri: &str, body: &str) -> axum::response::Response {
        router()
            .oneshot(
                Request::post(uri)
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .body(Body::from(body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("infallible")
    }

    /// Logout must be POST: with SameSite=Lax the session cookie still rides a
    /// cross-site top-level GET, so a GET logout is a forced-logout vector. A
    /// GET now 405s and a cookie-less POST degrades to the home redirect.
    #[tokio::test]
    async fn logout_is_post_only() {
        state();
        let res = get("/logout").await;
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
        let res = post("/logout", "").await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
    }

    /// The email is optional, but a value that could never receive a reset
    /// mail is refused — and refused before the row is created, so this
    /// no-DB state answers 400 rather than 500. A blank field passes the same
    /// gate and reaches the database.
    #[tokio::test]
    async fn register_refuses_a_malformed_email_before_touching_the_db() {
        state();
        let base = "username=ginger&password=longenough&confirm_password=longenough&email=";

        for bad in ["ginger", "ginger%40cookie", "two%40at%40cookie.run"] {
            let res = post("/register", &format!("{base}{bad}")).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
            let body = text(res).await;
            assert!(
                body.contains("That email address does not look right."),
                "{bad}: {body}"
            );
        }
    }

    /// The registration length floor is enforced before anything touches the
    /// database: this no-DB state would 500 past the policy gate, so a 400 with
    /// the password message proves the gate fired first.
    #[tokio::test]
    async fn register_enforces_min_lengths_before_touching_the_db() {
        state();
        let res = post(
            "/register",
            "username=ginger&password=short12&confirm_password=short12",
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        let html = String::from_utf8_lossy(&body);
        assert!(
            html.contains("Password must be at least 8 characters."),
            "expected the min-length message, got: {html}"
        );
    }

    async fn text(res: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8_lossy(&bytes).to_string()
    }

    /// The auth form posts through htmx, so a rejection has to come back as
    /// the fragment `#form-error` swaps when the request carries the header
    /// htmx sends — and as the whole page for a plain submit. Same handler,
    /// same status; only the body differs.
    #[tokio::test]
    async fn register_rejection_matches_the_shape_the_form_asked_for() {
        state();
        let form = "username=ginger&password=short12&confirm_password=short12";

        let res = router()
            .oneshot(
                Request::post("/register")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .header("HX-Request", "true")
                    .body(Body::from(form.to_string()))
                    .expect("request"),
            )
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let fragment = text(res).await;
        assert!(
            !fragment.contains("<!DOCTYPE"),
            "the htmx form swaps a fragment, not a page: {fragment}"
        );
        assert!(
            fragment.contains("Password must be at least 8 characters."),
            "the message still reads: {fragment}"
        );

        let res = post("/register", form).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let page = text(res).await;
        assert!(
            page.contains("<!DOCTYPE"),
            "a plain submit still gets the page: {page}"
        );
    }

    /// htmx reads its configuration from `meta[name=htmx-config]` when the
    /// deferred bundle runs, which is why the view-transition switch lives
    /// there: as an inline `htmx.config…` assignment it executed before
    /// `htmx` existed and threw, and htmx 4 renamed the key to `transitions`.
    /// The template provides it; this guards the rendered head.
    #[tokio::test]
    async fn base_head_enables_htmx_view_transitions() {
        state();
        let html = text(get("/changelog").await).await;
        assert!(
            html.contains(r#"<meta name="htmx-config" content='{"transitions": true}' />"#),
            "{html}"
        );
        assert!(
            !html.contains("globalViewTransitions"),
            "the htmx 4 key is `transitions`: {html}"
        );
        // the config has to be in the document before the deferred bundle runs
        let config = html.find("htmx-config").expect("config meta present");
        let bundle = html
            .find("/static/thirdparty/htmx.js")
            .expect("htmx script present");
        assert!(config < bundle, "config first, bundle second");
    }

    /// The sign-in form is an htmx form, so two things have to hold in the
    /// markup it renders: it swaps into its error slot (htmx drops a
    /// `hx-swap="none"` body, which would silently eat every message) and it
    /// still submits without JS via method/action.
    #[tokio::test]
    async fn auth_form_swaps_into_its_error_slot() {
        state();
        let html = text(get("/login").await).await;
        assert!(
            html.contains("hx-target=\"#form-error\""),
            "the error must land in #form-error: {html}"
        );
        assert!(!html.contains("hx-swap=\"none\""), "{html}");
        assert!(html.contains("action=\"/login\""), "no-JS fallback: {html}");
        assert!(html.contains("id=\"form-error\""), "{html}");
    }

    /// The register form asks for an email for password reset only, so the
    /// markup has to say all of that: an optional field, a collapsed hint, and
    /// the button that reveals it (whose `hidden` class lives in the markup, so
    /// the CSS generator sees it). The sign-in form asks for no email at all.
    #[tokio::test]
    async fn register_form_asks_for_an_optional_email_and_hints_why() {
        state();
        let html = text(get("/register").await).await;
        assert!(html.contains("name=\"email\""), "the field posts: {html}");
        assert!(
            !html.contains("name=\"email\" required"),
            "optional, so no required attribute: {html}"
        );
        assert!(
            html.contains("<p class=\"hidden text-xs text-foreground-muted\" id=\"email-hint\">Use for password reset only</p>"),
            "the hint starts hidden: {html}"
        );
        assert!(html.contains("aria-controls=\"email-hint\""), "{html}");
        assert!(
            html.contains("aria-expanded=\"false\""),
            "revealed only on click: {html}"
        );
        assert!(
            html.contains("h.classList.toggle('hidden')"),
            "the button toggles the hint: {html}"
        );
        assert!(html.contains(">Why?</button>"), "{html}");

        let login = text(get("/login").await).await;
        assert!(
            !login.contains("email-hint"),
            "no email field on the sign-in form: {login}"
        );
    }

    /// The way out of a forgotten password sits on the sign-in page and
    /// nowhere else: the register form has no password to recover, so it
    /// carries no reset link.
    #[tokio::test]
    async fn login_offers_the_reset_link_and_register_does_not() {
        state();
        let login = text(get("/login").await).await;
        assert!(login.contains("href=\"/forgot-password\""), "{login}");
        assert!(login.contains(">Forgot password?</a>"), "{login}");

        let register = text(get("/register").await).await;
        assert!(
            !register.contains("/forgot-password"),
            "nothing to recover on the register form: {register}"
        );
    }

    /// A reset request is answered with the same redirect whatever the address
    /// was, and a malformed one is settled locally: this unconnected handle
    /// would 500 past that gate, so the 303 proves the form neither confirms
    /// nor denies an account. The address never comes back in the header.
    #[tokio::test]
    async fn forgot_answers_neutrally_without_echoing_the_address() {
        state();
        for address in ["ginger", "ginger%40cookie", "two%40at%40cookie.run"] {
            let res = post("/forgot-password", &format!("email={address}")).await;
            assert_eq!(res.status(), StatusCode::SEE_OTHER, "{address}");
            assert_eq!(
                res.headers().get("location").and_then(|v| v.to_str().ok()),
                Some("/forgot-password?sent=1"),
                "{address}"
            );
        }

        let html = text(get("/forgot-password?sent=1").await).await;
        assert!(
            html.contains("If an account uses that address, a reset link is on its way."),
            "{html}"
        );
        assert!(
            html.contains("data-action=\"forgot_password\""),
            "the bot check is bound to this form: {html}"
        );
        assert!(
            !html.contains("ginger@cookie.run"),
            "the page echoes no address back: {html}"
        );
    }

    /// A missing link shows the dead-link page rather than a password form,
    /// and the form validates before the link is spent: a typo costs a retry,
    /// not the token. Both halves answer without a database.
    #[tokio::test]
    async fn reset_pages_refuse_a_dead_link_and_keep_the_token_on_a_typo() {
        state();
        let dead = text(get("/reset-password").await).await;
        assert!(
            dead.contains("That reset link is invalid or has expired."),
            "{dead}"
        );
        assert!(
            !dead.contains("name=\"token\""),
            "nothing to type a password into: {dead}"
        );
        assert!(
            dead.contains("href=\"/forgot-password\""),
            "the way to ask for a fresh link: {dead}"
        );

        let res = post(
            "/reset-password",
            "token=abc&password=short12&confirm_password=short12",
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let html = text(res).await;
        assert!(
            html.contains("Password must be at least 8 characters."),
            "{html}"
        );
        assert!(
            html.contains("value=\"abc\""),
            "the link survives a rejected password: {html}"
        );

        let res = post(
            "/reset-password",
            "token=abc&password=longenough&confirm_password=different1",
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(text(res).await.contains("Passwords do not match."));
    }

    /// Every response carries the security baselines — a page without
    /// them (or whose CSP regresses) fails here, so the middleware cannot
    /// silently drop out of the router.
    #[tokio::test]
    async fn responses_carry_security_headers() {
        state();
        let res = get("/changelog").await;
        let headers = res.headers();
        assert_eq!(
            headers
                .get("x-content-type-options")
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );
        assert_eq!(
            headers.get("x-frame-options").and_then(|v| v.to_str().ok()),
            Some("SAMEORIGIN")
        );
        let csp = headers
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok())
            .expect("CSP present");
        assert!(csp.contains("object-src 'none'"));
        assert!(csp.contains("frame-ancestors 'none'"));
        // the Turnstile widget loads its script, iframe and token requests
        // from challenges.cloudflare.com; a CSP that drops it breaks login
        // and registration in release builds
        assert!(csp.contains("script-src 'self' 'unsafe-inline' https://challenges.cloudflare.com"));
        assert!(csp.contains("frame-src https://challenges.cloudflare.com"));
        assert!(csp.contains("connect-src 'self' https://challenges.cloudflare.com"));
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
