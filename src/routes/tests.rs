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
    let mut cfg = crate::config::Config::default();
    // Google sign-in is configured here: the shared instance is installed
    // once per process, so the flow's routes can only be exercised through
    // the router with a credential present. The unconfigured answer (404,
    // no button) is covered where it is decided — in `state` and in the
    // auth template tests.
    cfg.google.client_id = "test-client-id.apps.googleusercontent.com".into();
    cfg.google.client_secret = "test-client-secret".into();
    crate::state::init(AppState {
        db: surrealdb::Surreal::init(),
        limiter: Limiter::new(crate::config::RateLimit::default()),
        sessions: Sessions::new(),
        oauth_states: crate::oauth::States::new(),
        cfg,
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
async fn profile_routes_reject_signed_out_and_invalid_users() {
    state();
    let res = get("/profile/me").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        res.headers().get(axum::http::header::LOCATION).unwrap(),
        "/login"
    );

    let res = get("/profile/abc").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // "no such user" needs a database to ask — the unconnected handle this
    // router-test state carries cannot tell a missing row from an outage,
    // so the unknown-id case is covered by the gated tests in the auth
    // module (a probe user's profile, and a 404 for a dead id).
}

#[tokio::test]
async fn settings_are_public_but_account_changes_require_a_session() {
    state();
    let res = get("/settings").await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = text(res).await;
    assert!(!html.contains("id=\"account-email\""));
    assert!(!html.contains("id=\"account\""));

    let res = post("/settings", "email=ginger%40cookie.run&password=longenough").await;
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

/// The news authoring surface is admin-only, answered with the site's 404 the
/// way the catalog editor is: a signed-out visitor can neither reach nor POST
/// any of it. The requests carry no peer address, so the debug loopback bypass
/// fails closed.
#[tokio::test]
async fn news_authoring_is_admin_only() {
    state();
    for uri in ["/admin/news", "/admin/news/new", "/admin/news/1/edit"] {
        assert_eq!(get(uri).await.status(), StatusCode::NOT_FOUND, "{uri}");
    }
    for uri in [
        "/admin/news/new",
        "/admin/news/1/edit",
        "/admin/news/1/delete",
        "/admin/news/1/publish",
    ] {
        assert_eq!(
            post(uri, "title=x").await.status(),
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
}

/// The teaser endpoint is a fragment, not a page: a direct visit is sent to
/// the list instead of rendering bare markup — and it answers without ever
/// touching the database.
#[tokio::test]
async fn news_teaser_redirects_a_direct_visit() {
    state();
    assert_eq!(get("/news/latest").await.status(), StatusCode::SEE_OTHER);
}

/// The landing page renders its news teaser as a lazy htmx GET, so the page
/// itself stays free of database work, and the navbar carries the link.
#[tokio::test]
async fn index_lazy_loads_the_news_teaser() {
    state();
    let html = text(get("/").await).await;
    assert!(html.contains("id=\"news-latest\""), "{html}");
    assert!(html.contains("hx-get=\"/news/latest\""), "{html}");
    assert!(html.contains("href=\"/news\""), "{html}");
    // and every page the base template frames advertises the feed, in the
    // locale being read
    assert!(
        html.contains("rel=\"alternate\" type=\"application/rss+xml\""),
        "{html}"
    );
    assert!(
        html.contains("href=\"http://localhost:6785/news/feed.xml\""),
        "{html}"
    );
}

/// The feed is a route of its own, matched ahead of the `/news/{id}` capture:
/// reaching that handler would answer 400, because "feed.xml" is not an id.
/// The post read behind it cannot run here — this handle is unconnected, which
/// is the only failure this route has — so a 500 is the site's own error page
/// and proof the static path won.
#[tokio::test]
async fn the_feed_is_a_route_of_its_own() {
    state();
    let res = get("/news/feed.xml").await;
    assert_eq!(
        res.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "feed.xml must not be read as a post id"
    );
}

/// A crawler's revalidation through the real stack, headers and all: the
/// sitemap answers 200 carrying its validators, 304 with no body and no
/// content type when the tag comes back, and the document itself for a tag
/// that does not match ours.
///
/// The date half is naturally absent here — the news read behind it cannot run
/// on this unconnected handle — which is itself the rule being pinned: a
/// document with nothing to date it by advertises no `Last-Modified` and is
/// never validated by one, so the tag is the only way to a 304.
#[tokio::test]
async fn the_sitemap_answers_a_conditional_get() {
    state();
    let first = get("/sitemap.xml").await;
    assert_eq!(first.status(), StatusCode::OK);
    let etag = header_of(&first, axum::http::header::ETAG);
    assert!(etag.starts_with('"'), "the tag is an entity-tag: {etag}");
    assert_eq!(
        header_of(&first, axum::http::header::CONTENT_TYPE),
        "application/xml; charset=utf-8"
    );
    assert_eq!(
        header_of(&first, axum::http::header::LAST_MODIFIED),
        "",
        "no live post to date it by, so no date is advertised"
    );
    assert!(text(first).await.contains("<urlset"));

    let again = get_with("/sitemap.xml", "if-none-match", &etag).await;
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        header_of(&again, axum::http::header::CONTENT_TYPE),
        "",
        "a 304 carries no representation to describe"
    );
    assert_eq!(header_of(&again, axum::http::header::ETAG), etag);
    assert_eq!(
        header_of(&again, axum::http::header::CACHE_CONTROL),
        crate::conditional::CACHE_CONTROL,
        "revalidating is what refreshes the client's window"
    );
    assert!(text(again).await.is_empty(), "and no body comes with it");

    // someone else's tag, an unreadable one, and a plain request all get the
    // document: a 304 is only ever an answer to a validator we sent
    for tag in ["\"0-0000000000000000\"", "not-a-tag"] {
        let stale = get_with("/sitemap.xml", "if-none-match", tag).await;
        assert_eq!(stale.status(), StatusCode::OK, "{tag}");
        assert!(text(stale).await.contains("<urlset"), "{tag}");
    }
    let dated = get_with(
        "/sitemap.xml",
        "if-modified-since",
        "Fri, 16 Oct 2026 00:00:00 GMT",
    )
    .await;
    assert_eq!(
        dated.status(),
        StatusCode::OK,
        "a date cannot revalidate a document that never advertised one"
    );
}

/// A GET carrying one extra request header — the conditional ones below are
/// the whole point of the request.
async fn get_with(uri: &str, name: &str, value: &str) -> axum::response::Response {
    router()
        .oneshot(
            Request::get(uri)
                .header(name, value)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("infallible")
}

fn header_of(res: &axum::response::Response, name: axum::http::header::HeaderName) -> String {
    res.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
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

fn location(res: &axum::response::Response) -> String {
    res.headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

/// Sign-in with Google starts at our own route: the visitor is sent to
/// Google's consent screen with the configured client and the callback URL
/// this deployment answers on, and the state rides a cookie as well as the
/// store — the cookie is what binds the callback to this browser, without
/// which a callback URL started elsewhere would sign the visitor in as
/// somebody else.
#[tokio::test]
async fn google_start_sends_the_visitor_to_consent_with_a_bound_state() {
    let app = state();
    let res = get("/auth/google").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let target = location(&res);
    assert!(
        target.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"),
        "{target}"
    );
    assert!(
        target.contains("client_id=test-client-id.apps.googleusercontent.com"),
        "{target}"
    );
    assert!(
        target.contains("redirect_uri=http%3A%2F%2Flocalhost%3A6785%2Fauth%2Fgoogle%2Fcallback"),
        "{target}"
    );
    assert!(target.contains("response_type=code"), "{target}");
    assert!(
        !target.contains("client_secret"),
        "the secret never leaves the server: {target}"
    );

    let cookies: Vec<String> = res
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::to_string)
        .collect();
    let cookie = cookies
        .iter()
        .find(|c| c.starts_with(crate::oauth::STATE_COOKIE))
        .unwrap_or_else(|| panic!("no state cookie among {cookies:?}"));
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("Secure"), "{cookie}");
    assert!(cookie.contains("SameSite=Lax"), "{cookie}");
    assert!(
        cookie.contains(&format!("path={}", crate::oauth::STATE_COOKIE_PATH)),
        "{cookie}"
    );
    // the flow leaves nothing behind: the state it handed out is one the
    // store knows, and spends exactly once
    let token = cookie
        .split_once('=')
        .and_then(|(_, rest)| rest.split(';').next())
        .unwrap_or_default()
        .to_string();
    assert!(
        app.oauth_states.consume(&token),
        "the issued state is redeemable"
    );
}

/// A callback that cannot prove it came back to the browser that started
/// the flow is refused with no session — and the state it named is spent
/// either way, so it cannot be replayed at the browser it was meant for.
#[tokio::test]
async fn google_callback_refuses_a_state_that_is_not_this_browsers() {
    let app = state();
    let token = app.oauth_states.issue();
    let res = router()
        .oneshot(
            Request::get(format!("/auth/google/callback?code=fake&state={token}"))
                .header(
                    "cookie",
                    format!("{}={}", crate::oauth::STATE_COOKIE, "someone-elses-state"),
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("infallible");
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&res), "/login?google=failed");
    assert!(
        !app.oauth_states.consume(&token),
        "a state that reached the wrong browser is spent, not left standing"
    );

    // and a callback with no cookie at all is refused the same way
    let token = app.oauth_states.issue();
    let res = get(&format!("/auth/google/callback?code=fake&state={token}")).await;
    assert_eq!(location(&res), "/login?google=failed");
    assert!(!app.oauth_states.consume(&token));
}

/// Every refusal the visitor can meet — no code, a state nobody issued,
/// consent declined — lands on the sign-in page with one whitelisted reason
/// and no session. None of them reaches Google, so the state this test
/// process installed is enough to exercise them.
#[tokio::test]
async fn google_callback_answers_every_refusal_on_the_login_page() {
    state();
    for (uri, expected) in [
        (
            "/auth/google/callback?code=fake&state=never-issued",
            "/login?google=failed",
        ),
        ("/auth/google/callback?code=fake", "/login?google=failed"),
        (
            "/auth/google/callback?state=never-issued",
            "/login?google=failed",
        ),
        (
            "/auth/google/callback?error=access_denied",
            "/login?google=cancelled",
        ),
        ("/auth/google/callback", "/login?google=failed"),
    ] {
        let res = get(uri).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(location(&res), expected, "{uri}");
    }
}

/// The sign-in page offers the Google button and explains a refusal — from
/// the whitelist only, so a `?google=` value a stranger chose renders
/// nothing at all rather than being echoed.
#[tokio::test]
async fn login_page_offers_google_and_explains_a_refusal() {
    state();
    let html = text(get("/login?google=failed").await).await;
    assert!(html.contains("href=\"/auth/google\""), "{html}");
    assert!(html.contains("Sign in with Google"), "{html}");
    // askama escapes the apostrophe, so the check is on the words around
    // it rather than on the exact rendered string
    assert!(
        html.contains("complete the sign-in with Google. Please try again."),
        "{html}"
    );

    let cancelled = text(get("/login?google=cancelled").await).await;
    assert!(
        cancelled.contains("The Google sign-in was cancelled"),
        "{cancelled}"
    );

    let junk = text(get("/login?google=%3Cscript%3Ealert(1)%3C%2Fscript%3E").await).await;
    assert!(
        !junk.contains("<script>alert(1)"),
        "the reason is never echoed: {junk}"
    );
    assert!(
        !junk.contains("complete the sign-in with Google"),
        "an unknown reason says nothing: {junk}"
    );
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

    let partial: CommonQuery = serde_urlencoded::from_str("cookie=89&c2=&pet=50&t1=317&t2=&t3=")
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

/// The tables are spread over eleven modules now, so the captures they share
/// are worth guarding as a set. The unconnected handle makes each answer
/// discriminating: a handler that reads the database 500s, one that only
/// renders answers 200, and a path nobody claims 404s — so these statuses say
/// *which* table took the path, not merely that something answered.
#[tokio::test]
async fn route_tables_claim_the_paths_they_name() {
    state();
    // the planner's form and the picker's grid render without a query, so the
    // 200 proves the static segment beat the `{id}` capture beside it
    // (`builds::show` would have read the database and 500'd)
    assert_eq!(get("/builds/new").await.status(), StatusCode::OK);
    assert_eq!(get("/builds/preview").await.status(), StatusCode::OK);
    assert_eq!(
        get("/cookies/new").await.status(),
        StatusCode::NOT_FOUND,
        "the admin form is debug-only, so here it 404s like a stranger sees"
    );
    // a POST lands on the write route rather than being 405'd by the
    // GET-only read route that owns the same path
    assert_ne!(
        post("/builds/new", "").await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );

    // the `{id}` captures reach their own handlers and never the fallback:
    // both shapes read the catalog, which this handle cannot serve, so the 500
    // is the proof (an unclaimed path would be 404)
    for uri in ["/builds/123", "/cookies", "/cookies/1", "/pets/7"] {
        assert_eq!(
            get(uri).await.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}"
        );
    }
}

/// A page that scrolls sideways is broken whichever way it looks on the
/// machine it was written on, and no assertion about markup can see that: the
/// 320px regression this guards was a navbar whose contents added up to 377px,
/// which `html { display: table }` then turned into a horizontal scrollbar on
/// every page of the site at once.
///
/// So the pages the router answers without a database are rendered here — the
/// real answers through the real middleware, not the templates behind them —
/// and `tools/overflow_check.mjs` serves them beside the repo's `static/`
/// tree and measures each at 320, 768 and 1024px in a headless Chromium. The
/// database-backed surfaces (catalog grids, detail pages, builds, news) are
/// absent here because this handle cannot serve them; the same script measures
/// any directory of saved pages, so a deployment with `CR_SURREAL_URL` set can
/// extend the set without touching the measurement.
///
/// Skips with a notice when bun or a browser is missing (the script answers 2),
/// the way the database tests skip without `CR_SURREAL_URL`.
#[tokio::test]
async fn no_page_scrolls_sideways() {
    state();
    let dir = std::path::Path::new("target/overflow");
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("fixture directory");

    // Thai is here because its labels run longer than the English ones, and
    // the auth and planner forms are the two widest things the site renders
    // without a database.
    let pages = [
        ("01-home", "/"),
        ("02-home-th", "/?lang=th"),
        ("03-changelog", "/changelog"),
        ("04-changelog-th", "/changelog?lang=th"),
        ("05-login", "/login"),
        ("06-register", "/register"),
        ("07-forgot-password", "/forgot-password?sent=1"),
        ("08-reset-dead-link", "/reset-password"),
        ("09-settings", "/settings"),
        ("10-planner-form", "/builds/new"),
        ("11-planner-form-th", "/builds/new?lang=th"),
        ("12-not-found", "/no-such-section"),
        ("13-error-page", "/cookies"),
    ];
    for (name, uri) in pages {
        let res = get(uri).await;
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(!body.is_empty(), "{uri} rendered nothing to measure");
        std::fs::write(dir.join(format!("{name}.html")), &body).expect("fixture");
    }

    let output = std::process::Command::new("bun")
        .arg("tools/overflow_check.mjs")
        .arg("--fixtures")
        .arg(dir)
        .arg("--static")
        .arg("static")
        .arg("--widths")
        .arg("320,768,1024")
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            eprintln!("skip: cannot run bun to measure the rendered pages ({e})");
            return;
        }
    };
    // the report is the test's output: a failure has to say what sticks out
    println!("{}", String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    match output.status.code() {
        Some(0) => {}
        // the script's own "nothing to measure with" answer: no browser here
        // is a skip, like no database is for the CR_SURREAL_URL tests
        Some(2) => eprintln!("skip: {}", stderr.trim()),
        code => panic!(
            "a page does not fit its viewport ({code:?}):\n{stderr}\n{}",
            String::from_utf8_lossy(&output.stdout)
        ),
    }
}

/// The submission guard's contract is a sequence of painted states, not a
/// response: a form with a Turnstile widget has to be locked until the widget
/// hands back a token, open once it does, and locked again — but no longer
/// busy — after the submission comes back refused. Reading the script cannot
/// see any of that, and the ordering bug this guards (disabling the submitter
/// inside the submit event, which makes htmx drop the request entirely) is
/// invisible in the served HTML.
///
/// So `tools/form_guard_check.mjs` loads the real `static/js/form_guard.js`
/// into a fixture page carrying the markup the templates emit and drives it in
/// a headless Chromium, replaying the events htmx 4 dispatches in the order a
/// failed request produces them.
///
/// Skips with a notice when bun or a browser is missing (the script answers 2),
/// the way the database tests skip without `CR_SURREAL_URL`.
#[test]
fn form_guard_locks_until_a_token_lands() {
    let output = std::process::Command::new("bun")
        .arg("tools/form_guard_check.mjs")
        .arg("--static")
        .arg("static")
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            eprintln!("skip: cannot run bun to drive the form guard ({e})");
            return;
        }
    };
    println!("{}", String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    match output.status.code() {
        Some(0) => {}
        // the script's own "nothing to run it in" answer: no browser here
        Some(2) => eprintln!("skip: {}", stderr.trim()),
        code => panic!(
            "the form guard's state machine regressed ({code:?}):\n{stderr}\n{}",
            String::from_utf8_lossy(&output.stdout)
        ),
    }
}

/// The build form is the only place a build token can come from, and the
/// release verifier refuses a tokenless POST: a form with no widget works all
/// through development, where `verify` returns true, and then rejects every
/// submission once the binary is built for release.
///
/// The `data-action` also has to stay equal to the name
/// `planner::write::create` hands `turnstile::verify` — a token minted for a
/// different action is refused, so a rename on either side silently closes the
/// form.
#[tokio::test]
async fn build_form_carries_the_bot_check() {
    state();
    let res = get("/builds/new").await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = text(res).await;
    assert!(
        html.contains("class=\"cf-turnstile\""),
        "the build form renders no widget, so a release build has no token to send"
    );
    assert!(
        html.contains("data-sitekey=\""),
        "the widget would render without a site key"
    );
    assert!(
        html.contains("data-action=\"build\""),
        "the widget's action must match turnstile::verify(.., \"build\")"
    );
    assert!(
        html.contains("data-callback=\"crTurnstileToken\""),
        "the widget's callback is what unlocks the submit button"
    );
    assert!(
        html.contains("data-turnstile-timeout=\""),
        "the guard has no failure line to fall back to"
    );
    assert!(
        html.contains("data-form-guard-hint"),
        "a locked button needs a status line saying why"
    );
}

/// A Turnstile token is only accepted when the widget that minted it carried
/// the same `action` the route checks, so a form rendered without its widget —
/// or with the wrong action — is closed to everyone in a release build while
/// development never notices, because the check is skipped there.
///
/// The actions are read out of `src/` instead of being listed here, so a new
/// check cannot ship uncovered: verifying a sixth action fails this test until
/// its page is added to the table below, and the table then asserts that the
/// page actually renders the matching widget.
#[tokio::test]
async fn every_verified_action_renders_its_widget() {
    state();
    // The page hosting each action's form. An action verified in `src/` and
    // missing here fails, and so does an entry nothing verifies any more — the
    // second is what keeps this table from quietly describing an old site.
    //
    // The reset form is not here: it only renders for a pending token, which
    // the database-free router cannot mint, so it is checked against its
    // template instead (see the fallback below). `auth::password`'s gated
    // `a_pending_token_renders_the_form_with_its_widget` covers it properly
    // when `CR_SURREAL_URL` is set.
    let routable = [
        ("login", "/login"),
        ("register", "/register"),
        ("forgot_password", "/forgot-password"),
        ("build", "/builds/new"),
    ];

    let verified = verified_actions();
    // Pinned both ways round: a new check fails here until its page is added
    // below, and a check the scan stops seeing fails instead of quietly losing
    // the coverage it used to have.
    let expected = [
        "build",
        "forgot_password",
        "login",
        "register",
        "reset_password",
    ];
    let found: Vec<&str> = verified.iter().map(String::as_str).collect();
    assert_eq!(
        found, expected,
        "a Turnstile action was added or removed — cover its page in this test, with the widget it renders"
    );

    for action in &verified {
        let Some((_, uri)) = routable.iter().find(|(a, _)| a == action) else {
            assert!(
                template_renders(action),
                "action {action:?} is verified, but no page here and no template carries a \
                 cf-turnstile widget with data-action=\"{action}\" — nobody can mint a token for it, \
                 so a release build refuses every submission"
            );
            continue;
        };
        let res = get(uri).await;
        assert_eq!(res.status(), StatusCode::OK, "{uri} renders for {action:?}");
        let html = text(res).await;
        assert!(
            html.contains("class=\"cf-turnstile\""),
            "{uri} verifies {action:?} but renders no widget, so a release build has no token to accept"
        );
        assert!(
            html.contains(&format!("data-action=\"{action}\"")),
            "{uri} renders a widget whose data-action is not {action:?}, so its token is minted \
             for a form the server will not accept"
        );
        assert!(
            html.contains("data-callback=\"crTurnstileToken\""),
            "{uri}: the widget's callback is what unlocks the submit button"
        );
        assert!(
            html.contains("data-turnstile-timeout=\""),
            "{uri}: the guard has no failure line to fall back to"
        );
        assert!(
            html.contains("data-form-guard-hint"),
            "{uri}: a locked button needs a status line saying why"
        );
    }
}

/// The action names the code verifies, read from its Turnstile call sites.
///
/// They are single-line calls ending in `.await`, which is what makes a scan
/// safe here without a parser; a call split across lines would drop out of the
/// list, and the stale-entry half of the test above fails rather than letting
/// that pass silently.
fn verified_actions() -> Vec<String> {
    let mut actions = Vec::new();
    for file in rust_sources(std::path::Path::new("src")) {
        let source = std::fs::read_to_string(&file).expect("source");
        for line in source.lines() {
            let line = line.trim_start();
            // a commented-out check is not a check
            if line.starts_with("//") {
                continue;
            }
            // Look for the call in the code rather than in its literals: the
            // quoted spans are blanked first, so this scan's own strings cannot
            // match it.
            let mut code_only = String::with_capacity(line.len());
            let mut quoted = false;
            for c in line.chars() {
                if c == '"' {
                    quoted = !quoted;
                } else if !quoted {
                    code_only.push(c);
                }
            }
            if !code_only.contains("turnstile::verify(") || !code_only.contains(".await") {
                continue;
            }
            // the action is the last quoted literal on the call: the config and
            // the submitted token come before it
            let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
            let action = quoted
                .into_iter()
                .filter(|span| is_action(span))
                .next_back();
            assert!(
                action.is_some(),
                "no action literal in this Turnstile check: {line}"
            );
            if let Some(action) = action {
                actions.push(action.to_string());
            }
        }
    }
    actions.sort();
    actions.dedup();
    actions
}

/// An action name is a plain identifier, which is what keeps a stray quoted
/// string in the call's neighbourhood from being read as one.
fn is_action(span: &str) -> bool {
    !span.is_empty() && span.len() <= 32 && span.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

fn rust_sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                found.push(path);
            }
        }
    }
    found
}

/// Whether a template renders a widget naming this action. This is the check
/// for a form that cannot be rendered here — the reset form only appears for a
/// token the database has to be holding.
fn template_renders(action: &str) -> bool {
    let widget = format!("data-action=\"{action}\"");
    let Ok(entries) = std::fs::read_dir("templates") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            return false;
        }
        std::fs::read_to_string(&path)
            .is_ok_and(|text| text.contains("class=\"cf-turnstile\"") && text.contains(&widget))
    })
}
