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
