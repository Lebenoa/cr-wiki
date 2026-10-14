//! Login, register and logout.

use askama::Template;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db;
use crate::session::{self, SessionUser, SESSION_COOKIE};

use super::errors::AppError;

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
    pub password: String,
    pub confirm_password: String,
    #[serde(rename = "cf-turnstile-response")]
    pub turnstile: Option<String>,
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
    let html = AuthPage {
        ctx,
        mode: mode.to_string(),
        error: error.to_string(),
    }
    .render()
    .unwrap_or_else(|e| format!("template error: {e}"));
    Html(html).into_response()
}

/// One rejection, both shapes: the htmx form gets just the message to swap
/// into `#form-error`, everything else gets the whole page. The status code is
/// the same either way — htmx only withholds a swap for 204/304, so a 400
/// still reaches the `#form-error` div.
fn auth_error(ctx: &Ctx, status: StatusCode, mode: &str, key: &str) -> Response {
    if ctx.is_fragment() {
        let html = AuthError {
            message: ctx.l.t(key),
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}"));
        (status, Html(html)).into_response()
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
    if ctx.htmx {
        let mut res = StatusCode::OK.into_response();
        // a constant, so no request input can reach this header value
        res.headers_mut()
            .insert("HX-Redirect", HeaderValue::from_static("/"));
        res
    } else {
        Redirect::to("/").into_response()
    }
}

pub async fn login_form(ctx: Ctx) -> Response {
    page(ctx, "login", "")
}

pub async fn register_form(ctx: Ctx) -> Response {
    page(ctx, "register", "")
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

    let created = db::create_user(&state.db, &form.username, &hash).await?;

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
            res.headers().get("HX-Redirect").and_then(|v| v.to_str().ok()),
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
            res.headers().get(header::LOCATION).and_then(|v| v.to_str().ok()),
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

    /// Argon2 in PHC form, through `SurrealDB`'s crypto functions: a hash
    /// verifies, a wrong password does not, and two hashes of the same
    /// password differ because the salt is fresh.
    #[tokio::test]
    async fn password_hashing() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let hash = session::hash_password(&pool, "correct horse")
            .await
            .expect("hash");
        assert!(
            hash.starts_with("$argon2"),
            "PHC format, so V can read it: {hash}"
        );
        assert!(session::verify_password(&pool, "correct horse", &hash).await);
        assert!(!session::verify_password(&pool, "wrong horse", &hash).await);
        let again = session::hash_password(&pool, "correct horse")
            .await
            .expect("hash");
        assert_ne!(hash, again, "salt must be fresh per hash");
        assert!(!session::verify_password(&pool, "correct horse", "not-a-hash").await);
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
        crate::db::connect_url(&url, &ns, &database, &user, &pass)
            .await
            .ok()
    }
}
