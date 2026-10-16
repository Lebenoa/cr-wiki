//! Login, register and logout, plus the pieces the other auth modules share.
//!
//! This file is the password session core: the sign-in and registration
//! forms, the two rejection shapes every auth page answers with (`page` and
//! `auth_error`), the gates (`register_policy`, `email_policy`), the plain
//! page responses a POST navigates with, the reset-token generator and the
//! public base that mail links and the OAuth callback are built from. The
//! other facets of the same surface live beside it — `password.rs` (the
//! forgot/reset flow), `google.rs` (Google sign-in) and `account.rs` (the
//! recovery address page).
//!
//! Every handler is re-exported here, so the router keeps addressing them as
//! `auth::login`, `auth::reset`, `auth::google_start`, and so on.

use askama::Template;
use axum::extract::Query;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Form;
use axum::Router;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db;
use crate::session::{self, SessionUser, SESSION_COOKIE};

use super::errors::AppError;

mod account;
mod google;
mod password;

use self::google::google_reason;

/// The auth surface in one table. The handlers are the re-exports at the top
/// of this file.
pub fn routes() -> Router {
    Router::new()
        .route("/login", get(login_form).post(login))
        .route("/register", get(register_form).post(register))
        // the Google flow: a GET pair, because both ends are browser
        // navigations Google and the visitor's browser follow themselves — the
        // callback cannot be a POST of ours or the session cookie would have
        // to ride a cross-site request. Its state parameter is what carries the
        // CSRF protection the cookie rules normally would (see
        // `auth::google_callback`).
        .route("/auth/google", get(google_start))
        .route("/auth/google/callback", get(google_callback))
        // the reset pair sits with the other auth routes, well before the
        // catalog captures, so neither path can be read as a `/{section}/{id}`
        // detail page
        .route("/forgot-password", get(forgot_form).post(forgot))
        .route("/settings", get(settings_form).post(update_account_email))
        .route("/reset-password", get(reset_form).post(reset))
        // GET logout with the SameSite=Lax session cookie lets any
        // cross-site link end the session: Lax still rides a top-level GET
        // navigation, so a linked /logout is a forced logout. POST only —
        // Lax never sends the cookie on a cross-site POST, so a foreign
        // page cannot forge it — matching /revoke-sessions below.
        .route("/logout", post(logout))
        .route("/revoke-sessions", post(revoke_sessions))
}

pub use self::account::{settings_form, update_account_email};
pub use self::google::{google_callback, google_start};
pub use self::password::{forgot, forgot_form, reset, reset_form};

/// A name has to be typeable by humans and hard to squat; 3 keeps
/// "am" and "zz" out while letting every two-letter word-that-matters in.
pub const MIN_USERNAME_LEN: usize = 3;
/// Argon2id has no length floor of its own; 8 is the conventional minimum
/// for anything a visitor can brute-force against a stolen hash list.
pub const MIN_PASSWORD_LEN: usize = 8;

#[derive(Template)]
#[template(path = "auth.html")]
struct AuthPage {
    ctx: Ctx,
    /// "login" or "register", which picks the strings and the action
    mode: String,
    error: String,
    /// A success line — "your password is updated" — kept in its own field
    /// because the error slot is styled and worded for failures.
    notice: String,
}

impl AuthPage {
    fn is_register(&self) -> bool {
        self.mode == "register"
    }

    /// The .tr keys differ per mode rather than sharing one form string, so
    /// the template asks for the pair it needs.
    fn title_key(&self) -> &'static str {
        if self.is_register() {
            "register"
        } else {
            "login"
        }
    }

    fn submit_key(&self) -> &'static str {
        if self.is_register() {
            "register_button_text"
        } else {
            "login_button_text"
        }
    }

    fn loading_key(&self) -> &'static str {
        if self.is_register() {
            "register_loading"
        } else {
            "login_loading"
        }
    }

    /// "Don't have an account?" / "Already have an account?"
    fn switch_prompt_key(&self) -> &'static str {
        if self.is_register() {
            "already_have_account"
        } else {
            "dont_have_account"
        }
    }

    fn other_mode(&self) -> &'static str {
        if self.is_register() {
            "login"
        } else {
            "register"
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    #[serde(rename = "cf-turnstile-response")]
    pub turnstile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterForm {
    pub username: String,
    /// Optional on purpose: the form asks for it for password reset only, and
    /// an account without one is still an account. Absent and empty both mean
    /// "not given" here rather than a missing-field rejection.
    pub email: Option<String>,
    pub password: String,
    pub confirm_password: String,
    #[serde(rename = "cf-turnstile-response")]
    pub turnstile: Option<String>,
}

/// The flags the auth pages read out of the query string: `?sent=1` on the
/// forgot page and `?reset=1` on login. They say "we tried" and "it worked",
/// never which address or account was involved.
#[derive(Debug, Deserialize)]
pub struct AuthQuery {
    pub sent: Option<String>,
    pub reset: Option<String>,
    pub token: Option<String>,
    /// why a Google sign-in came back without a session. Only the three values
    /// this module writes are honoured — see [`google_reason`] — so the page
    /// can never be made to say something a stranger chose.
    pub google: Option<String>,
}

#[derive(Template)]
#[template(path = "security_notice.html")]
struct SecurityNoticeHtml {
    message: String,
}

#[derive(Template)]
#[template(path = "security_notice.txt")]
struct SecurityNoticeText {
    message: String,
}

/// Sends localized security alerts best-effort after an account change.
async fn send_security_notice(
    ctx: &Ctx,
    cfg: &crate::config::Config,
    to: &str,
    subject_key: &str,
    message_key: &str,
) -> Result<bool, askama::Error> {
    let message = ctx.l.t(message_key);
    let mail = crate::mail::Mail {
        subject: ctx.l.t(subject_key),
        text: SecurityNoticeText {
            message: message.clone(),
        }
        .render()?,
        html: SecurityNoticeHtml { message }.render()?,
    };
    Ok(crate::mail::send(cfg, to, &mail).await)
}

/// The error paragraph on its own, for the htmx form: the form swaps it into
/// `#form-error`, so a rejected attempt does not re-render the page (and does
/// not throw away what the visitor already typed).
#[derive(Template)]
#[template(path = "auth_error.html")]
struct AuthError {
    message: String,
}

fn page(ctx: Ctx, mode: &str, error: &str) -> Response {
    page_notice(ctx, mode, error, "")
}

fn page_notice(ctx: Ctx, mode: &str, error: &str, notice: &str) -> Response {
    let html = AuthPage {
        ctx,
        mode: mode.to_string(),
        error: error.to_string(),
        notice: notice.to_string(),
    }
    .render()
    .unwrap_or_else(|e| format!("template error: {e}"));
    Html(html).into_response()
}

/// The translated message paragraph on its own, for an htmx form: it swaps
/// into `#form-error` instead of re-rendering the page, which also keeps what
/// the visitor already typed.
fn error_fragment(ctx: &Ctx, key: &str) -> String {
    AuthError {
        message: ctx.l.t(key),
    }
    .render()
    .unwrap_or_else(|e| format!("template error: {e}"))
}

/// One rejection, both shapes: the htmx form gets just the message to swap
/// into `#form-error`, everything else gets the whole page. The status code is
/// the same either way — htmx only withholds a swap for 204/304, so a 400
/// still reaches the `#form-error` div.
fn auth_error(ctx: &Ctx, status: StatusCode, mode: &str, key: &str) -> Response {
    if ctx.is_fragment() {
        (status, Html(error_fragment(ctx, key))).into_response()
    } else {
        (status, page(ctx.clone(), mode, key)).into_response()
    }
}

/// Where a successful sign-in lands. htmx fetches follow the 303 themselves
/// and hand the index page back to a form whose `hx-swap` drops it: the
/// visitor would sit on /login with a valid session and no navigation at all.
/// `HX-Redirect` is read before any swap and makes htmx navigate for real, so
/// an htmx request gets a 200 carrying it; every other client keeps the plain
/// 303 the browser follows on its own.
fn home_response(ctx: &Ctx) -> Response {
    navigation_response(ctx, "/")
}

/// A navigation after a POST. htmx's own fetch follows a 3xx and hands the
/// final page to the form's `hx-swap`, which drops it, so an htmx request gets
/// a 200 carrying `HX-Redirect` — read before any swap — while every other
/// client keeps the plain 303 it follows itself.
fn navigation_response(ctx: &Ctx, to: &'static str) -> Response {
    if ctx.htmx {
        let mut res = StatusCode::OK.into_response();
        // a literal from this module, so no request input reaches the header
        if let Ok(v) = HeaderValue::from_str(to) {
            res.headers_mut().insert("HX-Redirect", v);
        }
        res
    } else {
        Redirect::to(to).into_response()
    }
}

pub async fn login_form(ctx: Ctx, Query(q): Query<AuthQuery>) -> Response {
    let notice = if q.reset.as_deref() == Some("1") {
        "reset_password_done"
    } else {
        ""
    };
    // a declined Google consent or a failed callback lands here rather than on
    // an error page of its own: the sign-in form is where another attempt
    // starts
    page_notice(ctx, "login", google_reason(q.google.as_deref()), notice)
}

pub async fn register_form(ctx: Ctx) -> Response {
    page(ctx, "register", "")
}

/// 256 bits of OS randomness as hex. Two v4 UUIDs rather than a new crypto
/// dependency: `uuid` already draws from the process CSPRNG, and both the
/// reset token and a Google-created account's password are exactly the values
/// that must not be guessable.
fn random_hex_256() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn reset_token() -> String {
    random_hex_256()
}

/// The absolute base for links inside mail and for the OAuth callback. The
/// configured `[email] site_url` wins because the request's own host is
/// attacker-controlled: a forged `Host` header would otherwise have the app
/// mail the visitor a link to the attacker's site with a live token appended —
/// or ask Google to deliver the sign-in code to it. The fallback exists so
/// local development works with nothing configured.
fn public_base(ctx: &Ctx, cfg: &crate::config::Config) -> String {
    let configured = cfg.email.site_url.trim_end_matches('/');
    if !configured.is_empty() {
        return configured.to_string();
    }
    if cfg!(not(debug_assertions)) {
        tracing::warn!(
            "no configured public base: reset links and the Google callback are built from the request host, which a forged Host header can redirect"
        );
    }
    ctx.site_url.trim_end_matches('/').to_string()
}

/// The registration policy gate, pure so it is testable without a server:
/// `None` passes, `Some(key)` is the .tr key for the BAD_REQUEST response.
/// One-character accounts are squatting and brute-force bait; the only cost
/// of a couple of characters is a keystroke, so the floor is cheap. Login
/// deliberately does not enforce it — old accounts must keep working.
fn register_policy(username: &str, password: &str) -> Option<&'static str> {
    if username.is_empty() || password.is_empty() {
        return Some("register_required");
    }
    if username.chars().count() < MIN_USERNAME_LEN {
        return Some("register_username_short");
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Some("register_password_short");
    }
    None
}

/// The email gate, pure so it is testable without a server: `None` passes,
/// `Some(key)` names the translation key for the rejected attempt.
///
/// Deliberately not RFC 5322. The only thing worth rejecting is a value that
/// could never receive a reset mail, and a strict parser rejects plenty of
/// addresses that work. So: one `@` with something either side, a dotted
/// domain, no whitespace or control bytes, and a length a mailbox accepts.
fn email_policy(email: Option<&str>) -> Option<&'static str> {
    /// The practical mailbox limit from RFC 5321 (254 including the domain).
    const MAX_EMAIL_LEN: usize = 254;
    let email = email?;
    if email.chars().count() > MAX_EMAIL_LEN
        || email.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Some("register_email_invalid");
    }
    let Some((local, domain)) = email.split_once('@') else {
        return Some("register_email_invalid");
    };
    // a second `@` makes the domain ambiguous; empty dotted parts are typos
    if local.is_empty()
        || domain.contains('@')
        || !domain.contains('.')
        || domain.split('.').any(str::is_empty)
    {
        return Some("register_email_invalid");
    }
    None
}

pub async fn login(ctx: Ctx, Form(form): Form<LoginForm>) -> Result<Response, AppError> {
    let state = crate::state::state();
    if !crate::turnstile::verify(&state.cfg, form.turnstile.as_deref(), "login").await {
        return Ok(auth_error(
            &ctx,
            StatusCode::FORBIDDEN,
            "login",
            "turnstile_form_failed",
        ));
    }
    if form.username.is_empty() || form.password.is_empty() {
        return Ok(auth_error(
            &ctx,
            StatusCode::BAD_REQUEST,
            "login",
            "invalid_credentials",
        ));
    }
    let found = db::find_user(&state.db, &form.username).await?;

    let Some(user) = found else {
        // no such user: still pay for a hash, so the answer takes as long as
        // a real check and cannot be told apart by timing
        session::waste_time(&state.db, &form.password).await;
        return Ok(auth_error(
            &ctx,
            StatusCode::NOT_FOUND,
            "login",
            "invalid_credentials",
        ));
    };
    if !session::verify_password(&state.db, &form.password, &user.password).await {
        return Ok(auth_error(
            &ctx,
            StatusCode::NOT_FOUND,
            "login",
            "invalid_credentials",
        ));
    }

    let key = state.sessions.start(SessionUser {
        id: user.id,
        username: user.username,
        is_admin: user.is_admin,
    });
    let mut res = home_response(&ctx);
    set_session_cookie(res.headers_mut(), &key);
    Ok(res)
}

pub async fn register(ctx: Ctx, Form(form): Form<RegisterForm>) -> Result<Response, AppError> {
    let state = crate::state::state();
    // cheap local validation before the network round-trip: a malformed
    // submission gets its precise 400 without spending a CAPTCHA check, and
    // a complete one still needs the token below
    if let Some(key) = register_policy(&form.username, &form.password) {
        return Ok(auth_error(&ctx, StatusCode::BAD_REQUEST, "register", key));
    }
    // the field is optional, so a blank one is not an error — only a value
    // that could never receive a reset mail is
    let email = form
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty());
    if let Some(key) = email_policy(email) {
        return Ok(auth_error(&ctx, StatusCode::BAD_REQUEST, "register", key));
    }
    if form.password != form.confirm_password {
        return Ok(auth_error(
            &ctx,
            StatusCode::BAD_REQUEST,
            "register",
            "register_mismatch",
        ));
    }
    if !crate::turnstile::verify(&state.cfg, form.turnstile.as_deref(), "register").await {
        return Ok(auth_error(
            &ctx,
            StatusCode::FORBIDDEN,
            "register",
            "turnstile_form_failed",
        ));
    }
    let hash = match session::hash_password(&state.db, &form.password).await {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("password hash failed: {e}");
            return Ok(auth_error(
                &ctx,
                StatusCode::INTERNAL_SERVER_ERROR,
                "register",
                "register_failed",
            ));
        }
    };

    let created = db::create_user(&state.db, &form.username, &hash, email, None).await?;

    let Some(user) = created else {
        return Ok(auth_error(
            &ctx,
            StatusCode::BAD_REQUEST,
            "register",
            "register_taken",
        ));
    };
    let key = state.sessions.start(SessionUser {
        id: user.id,
        username: user.username,
        is_admin: user.is_admin,
    });
    let mut res = home_response(&ctx);
    set_session_cookie(res.headers_mut(), &key);
    Ok(res)
}

pub async fn logout(headers: HeaderMap) -> Response {
    let state = crate::state::state();
    if let Some(key) = crate::ctx::cookie(&headers, SESSION_COOKIE) {
        state.sessions.end(&key);
    }
    let mut res = Redirect::to("/").into_response();
    // an empty value with Max-Age=0 clears it in every browser
    if let Ok(v) = HeaderValue::from_str(&format!("{SESSION_COOKIE}=; path=/; Max-Age=0")) {
        res.headers_mut().append(header::SET_COOKIE, v);
    }
    res
}

/// POST /revoke-sessions — the account menu's "revoke all" action: ends
/// every session for the signed-in user, this one included, then clears
/// the cookie. Login does NOT revoke; multi-device sign-in stays valid
/// until the user explicitly revokes here.
pub async fn revoke_sessions(headers: HeaderMap) -> Response {
    let state = crate::state::state();
    if let Some(key) = crate::ctx::cookie(&headers, SESSION_COOKIE) {
        if let Some(user) = state.sessions.get(&key) {
            state.sessions.end_all_for(user.id);
        }
    }
    let mut res = Redirect::to("/").into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("{SESSION_COOKIE}=; path=/; Max-Age=0")) {
        res.headers_mut().append(header::SET_COOKIE, v);
    }
    res
}

/// `HttpOnly` so script cannot read the session, `SameSite=Lax` so it
/// survives a normal navigation but not a cross-site form post. `Secure`
/// keeps it off cleartext HTTP; localhost is exempt by browser fiat, so
/// development is unaffected.
fn set_session_cookie(headers: &mut HeaderMap, key: &str) {
    if let Ok(v) = HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={key}; path=/; HttpOnly; Secure; SameSite=Lax"
    )) {
        headers.append(header::SET_COOKIE, v);
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    async fn body_text(res: Response) -> String {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8_lossy(&bytes).to_string()
    }

    /// The stored email for a username, or `None` when the row has none.
    async fn stored_email(pool: &crate::db::Db, username: &str) -> Option<String> {
        let rows: Vec<Option<String>> = pool
            .query("SELECT VALUE email FROM user WHERE username = $u LIMIT 1")
            .bind(("u", username.to_string()))
            .await
            .expect("select")
            .take(0)
            .expect("take");
        rows.into_iter().flatten().next()
    }

    async fn drop_probe_user(pool: &crate::db::Db, username: &str) {
        pool.query("DELETE user WHERE username = $u")
            .bind(("u", username.to_string()))
            .await
            .expect("cleanup");
    }

    /// The email is optional, so both halves matter: a blank field is the
    /// visitor declining password reset, and anything that could never
    /// receive a mail is refused. The handler trims before asking.
    #[test]
    fn email_policy_accepts_blank_and_rejects_typos() {
        assert_eq!(email_policy(None), None, "blank is allowed");
        for ok in [
            "a@b.co",
            "first.last+tag@sub.example.co.uk",
            "ginger@cookie.run",
        ] {
            assert_eq!(email_policy(Some(ok)), None, "{ok} should pass");
        }
        for bad in [
            "ginger",
            "ginger@",
            "@cookie.run",
            "ginger@cookie",
            "ginger@cookie.",
            "ginger@.run",
            "two@at@cookie.run",
            "white space@cookie.run",
            "ginger@cookie.run\n",
        ] {
            assert_eq!(
                email_policy(Some(bad)),
                Some("register_email_invalid"),
                "{bad} should be refused"
            );
        }
        // the cap is the mailbox limit, not a preference: 254 passes, 255 does not
        let longest = format!("{}@cookie.run", "a".repeat(254 - "@cookie.run".len()));
        assert_eq!(longest.chars().count(), 254);
        assert_eq!(email_policy(Some(&longest)), None);
        assert_eq!(
            email_policy(Some(&format!("{longest}a"))),
            Some("register_email_invalid")
        );
    }

    /// The email only exists to enable a password reset later, so it has to
    /// reach the row — and an account without one is still an account. This is
    /// the one DB-gated test that writes: it creates its own users and removes
    /// them again, so a scratch namespace stays scratch.
    #[tokio::test]
    async fn register_email_reaches_the_row_and_stays_optional() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let hash = session::hash_password(&pool, "probe password")
            .await
            .expect("hash");

        // the address is this test's own: the reset-token test below matches
        // on `probe@example.com`, and these two run at the same time against
        // the same scratch database
        for (username, email) in [
            ("cr_email_probe_with", Some("register-probe@example.com")),
            ("cr_email_probe_without", None),
        ] {
            // a half-finished earlier run must not fail the create
            drop_probe_user(&pool, username).await;
            let created = db::create_user(&pool, username, &hash, email, None)
                .await
                .expect("create");
            assert!(created.is_some(), "{username} created");
            assert_eq!(
                stored_email(&pool, username).await.as_deref(),
                email,
                "{username} keeps what the form sent"
            );
        }

        drop_probe_user(&pool, "cr_email_probe_with").await;
        drop_probe_user(&pool, "cr_email_probe_without").await;
        assert_eq!(stored_email(&pool, "cr_email_probe_with").await, None);
    }

    /// The link inside a mail comes from `[email] site_url` whenever it is
    /// set, because the request's own host is attacker-controlled: a forged
    /// `Host` header would otherwise aim the visitor's reset link at another
    /// site. With nothing configured it falls back to the request's host, so
    /// local development still works.
    #[test]
    fn public_base_prefers_the_configured_site_url() {
        let ctx = crate::testutil::test_ctx("en");
        let mut cfg = crate::config::Config::default();
        assert_eq!(public_base(&ctx, &cfg), "http://localhost:6785");
        cfg.email.site_url = "https://cookierun.example/".into();
        assert_eq!(public_base(&ctx, &cfg), "https://cookierun.example");
    }

    /// A reset token is 256 bits of hex from the OS CSPRNG — enough that
    /// guessing one is not a strategy, and long enough that two never repeat.
    /// The same source backs a Google account's password, so the length is
    /// asserted for both.
    #[test]
    fn reset_tokens_and_google_secrets_are_long_random_hex() {
        let a = reset_token();
        let b = reset_token();
        assert_eq!(a.len(), 64, "{a}");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
        assert_ne!(a, b);

        let secret = random_hex_256();
        assert_eq!(secret.len(), 64, "{secret}");
        assert!(secret.chars().all(|c| c.is_ascii_hexdigit()), "{secret}");
        assert_ne!(secret, random_hex_256());
    }

    /// The Google button appears only on a deployment that configured the
    /// credential, and its label has to come out of the .tr files rather than
    /// fall back to the key itself.
    #[test]
    fn google_button_appears_only_when_configured() {
        let page = |ctx, mode: &str| {
            AuthPage {
                ctx,
                mode: mode.to_string(),
                error: String::new(),
                notice: String::new(),
            }
            .render()
            .expect("render")
        };
        for mode in ["login", "register"] {
            let off = page(crate::testutil::test_ctx("en"), mode);
            assert!(!off.contains("/auth/google"), "{mode}: {off}");
            assert!(!off.contains("Sign in with Google"), "{mode}: {off}");

            let on = page(crate::testutil::test_ctx_with_google("en"), mode);
            assert!(on.contains("href=\"/auth/google\""), "{mode}: {on}");
            assert!(on.contains("Sign in with Google"), "{mode}: {on}");
            // the translation key must exist, not fall back to itself
            assert!(!on.contains(">google_sign_in</a>"), "{mode}: {on}");
        }
    }
    /// A successful sign-in has to reach `/` through htmx too: the fetch
    /// inside htmx follows the 303 by itself and hands the index page back to
    /// a form whose `hx-swap` drops it, which would leave the visitor on
    /// /login with a live session and no navigation. htmx reads `HX-Redirect`
    /// before any swap, so an htmx request gets that header on a 200; every
    /// other client keeps the plain 303 the browser follows on its own.
    #[test]
    fn success_navigates_via_hx_redirect_for_htmx_only() {
        let mut ctx = crate::testutil::test_ctx("en");
        ctx.htmx = true;
        let res = home_response(&ctx);
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get("HX-Redirect")
                .and_then(|v| v.to_str().ok()),
            Some("/")
        );
        assert!(
            res.headers().get(header::LOCATION).is_none(),
            "a 3xx would be followed inside htmx's fetch and lose the header"
        );

        let ctx = crate::testutil::test_ctx("en");
        let res = home_response(&ctx);
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            res.headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok()),
            Some("/")
        );
    }

    /// A rejection keeps its status either way, but the body is what the
    /// caller can use: the message alone for the htmx form (which swaps it
    /// into `#form-error`), the whole page for a plain submit. A boosted
    /// request counts as a page — htmx swaps those into the body.
    #[tokio::test]
    async fn rejections_are_fragments_for_htmx_and_pages_otherwise() {
        let mut ctx = crate::testutil::test_ctx("en");
        ctx.htmx = true;
        let res = auth_error(
            &ctx,
            StatusCode::BAD_REQUEST,
            "register",
            "register_password_short",
        );
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let fragment = body_text(res).await;
        assert!(
            !fragment.contains("<!DOCTYPE"),
            "a fragment, not a page: {fragment}"
        );
        assert!(fragment.contains("text-error"), "{fragment}");
        assert!(
            fragment.contains("Password must be at least 8 characters."),
            "the message still reads: {fragment}"
        );
        // the widget keeps its own copy of the token, which the server has
        // now spent: the visitor's retry needs a fresh one
        assert!(fragment.contains("turnstile.reset()"), "{fragment}");

        ctx.boosted = true;
        let res = auth_error(
            &ctx,
            StatusCode::BAD_REQUEST,
            "register",
            "register_password_short",
        );
        let boosted = body_text(res).await;
        assert!(boosted.contains("<!DOCTYPE"), "{boosted}");

        let ctx = crate::testutil::test_ctx("en");
        let res = auth_error(
            &ctx,
            StatusCode::BAD_REQUEST,
            "register",
            "register_password_short",
        );
        let page = body_text(res).await;
        assert!(page.contains("<!DOCTYPE"), "{page}");
        assert!(
            page.contains("Password must be at least 8 characters."),
            "{page}"
        );
        assert!(
            !page.contains("turnstile.reset()"),
            "a fresh page render hands out a fresh token on its own: {page}"
        );
    }

    /// The one-character accounts the review flagged are rejected; an
    /// existing shorter account can still log in because login never calls
    /// the policy.
    #[test]
    fn register_policy_enforces_lengths() {
        assert_eq!(
            register_policy("", "x".repeat(8).as_str()),
            Some("register_required")
        );
        assert_eq!(
            register_policy("ab", "password"),
            Some("register_username_short")
        );
        assert_eq!(
            register_policy("abc", "short1"),
            Some("register_password_short")
        );
        assert_eq!(
            register_policy("ようこそ", "password"),
            None,
            "3 chars counts characters, not bytes"
        );
        assert_eq!(register_policy("abc", "x".repeat(8).as_str()), None);
    }

    /// The DB handle behind the gated tests: unset means they skip, so
    /// `cargo test` stays green without a server. Points at a scratch
    /// namespace/database — never at data you cannot lose.
    async fn live_db() -> Option<crate::db::Db> {
        let url = std::env::var("CR_SURREAL_URL").ok()?;
        let ns = std::env::var("CR_SURREAL_NS").unwrap_or_else(|_| "cookierun".into());
        let database = std::env::var("CR_SURREAL_DB").unwrap_or_else(|_| "cookierun".into());
        let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
        match crate::db::connect_url(&url, &ns, &database, &user, &pass).await {
            Ok(db) => Some(db),
            // a URL that is set but unusable must not read as "no server
            // configured": that would turn a broken database into a green,
            // silently skipping run
            Err(e) => {
                eprintln!("skip: CR_SURREAL_URL is set but unusable ({e})");
                None
            }
        }
    }

    #[test]
    fn security_notices_render_password_and_email_alerts() {
        let en = crate::testutil::test_ctx("en");
        let password = SecurityNoticeText {
            message: en.l.t("security_password_reset_message"),
        }
        .render()
        .expect("text renders");
        assert!(password.contains("password was reset"), "{password}");

        let html = SecurityNoticeHtml {
            message: en.l.t("security_email_changed_message"),
        }
        .render()
        .expect("html renders");
        assert!(html.contains("recovery email was changed"), "{html}");

        let th = crate::testutil::test_ctx("th");
        assert_ne!(
            th.l.t("security_email_changed_subject"),
            "security_email_changed_subject"
        );
    }
}
