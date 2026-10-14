//! Login, register and logout.

use askama::Template;
use axum::extract::Query;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db;
use crate::session::{self, SessionUser, SESSION_COOKIE};
use crate::state::AppState;

use super::errors::AppError;

/// A name has to be typeable by humans and hard to squat; 3 keeps
/// "am" and "zz" out while letting every two-letter word-that-matters in.
pub const MIN_USERNAME_LEN: usize = 3;
/// Argon2id has no length floor of its own; 8 is the conventional minimum
/// for anything a visitor can brute-force against a stolen hash list.
pub const MIN_PASSWORD_LEN: usize = 8;

/// A reset link is good for half an hour: long enough to reach an inbox and
/// be used, short enough that a mail left behind in an old account is not a
/// standing key to it.
const RESET_TTL_SECS: i64 = 30 * 60;

/// One link a minute per account. Without it the form lets anyone aim reset
/// mail at somebody else's inbox as fast as the IP bucket allows, and since
/// each request invalidates the previous link, the account's own mail is the
/// casualty.
const RESET_THROTTLE_SECS: i64 = 60;

/// Where the forgot form's POST lands, and where a completed reset sends the
/// visitor. Both are literals: a `Location` carrying request input is a
/// header-smuggling risk, and an absolute one would be an open redirect.
const SENT_PATH: &str = "/forgot-password?sent=1";
const RESET_DONE_PATH: &str = "/login?reset=1";

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

/// The forgot form asks for one thing: the address the reset link goes to.
/// The username is not enough — the visitor may not remember which address the
/// account carries, and the row may carry none at all.
#[derive(Debug, Deserialize)]
pub struct ForgotForm {
    pub email: Option<String>,
    #[serde(rename = "cf-turnstile-response")]
    pub turnstile: Option<String>,
}

/// The reset form carries the token back in a hidden field. It arrives from a
/// query string, so it is treated as untrusted: escaped where it is rendered,
/// hashed where it is looked up, and never interpolated into SQL.
#[derive(Debug, Deserialize)]
pub struct ResetForm {
    pub token: Option<String>,
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
}

#[derive(Template)]
#[template(path = "forgot_password.html")]
struct ForgotPage {
    ctx: Ctx,
    error: String,
    notice: String,
}

#[derive(Template)]
#[template(path = "reset_password.html")]
struct ResetPage {
    ctx: Ctx,
    /// the token as it arrived; empty once the link has been refused
    token: String,
    error: String,
}

#[derive(Template)]
#[template(path = "account_email.html")]
struct AccountEmailPage {
    ctx: Ctx,
    email: String,
    error: String,
    notice: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountEmailForm {
    pub email: Option<String>,
    pub password: String,
}

#[derive(Template)]
#[template(path = "account_email_notice.html")]
struct AccountEmailNotice {
    message: String,
}

/// The mail's two bodies, rendered from the app's own templates so every
/// string comes out of the .tr files exactly like the pages do.
#[derive(Template)]
#[template(path = "reset_email.html")]
struct ResetEmailHtml {
    ctx: Ctx,
    link: String,
    minutes: i64,
}

#[derive(Template)]
#[template(path = "reset_email.txt")]
struct ResetEmailText {
    ctx: Ctx,
    link: String,
    minutes: i64,
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

fn render_forgot(ctx: Ctx, error: &str, notice: &str) -> Response {
    let html = ForgotPage {
        ctx,
        error: error.to_string(),
        notice: notice.to_string(),
    }
    .render()
    .unwrap_or_else(|e| format!("template error: {e}"));
    Html(html).into_response()
}

fn render_reset(ctx: Ctx, token: &str, error: &str) -> Response {
    let html = ResetPage {
        ctx,
        token: token.to_string(),
        error: error.to_string(),
    }
    .render()
    .unwrap_or_else(|e| format!("template error: {e}"));
    Html(html).into_response()
}

/// One rejection from a password page, both shapes: the htmx form gets the
/// paragraph for `#form-error`, everything else the whole page. The reset page
/// keeps the token in what it re-renders, so a rejected password costs a retry
/// rather than the link.
fn forgot_error(ctx: &Ctx, status: StatusCode, key: &str) -> Response {
    if ctx.is_fragment() {
        (status, Html(error_fragment(ctx, key))).into_response()
    } else {
        (status, render_forgot(ctx.clone(), key, "")).into_response()
    }
}

fn reset_error(ctx: &Ctx, status: StatusCode, token: &str, key: &str) -> Response {
    if ctx.is_fragment() {
        (status, Html(error_fragment(ctx, key))).into_response()
    } else {
        (status, render_reset(ctx.clone(), token, key)).into_response()
    }
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
    page_notice(ctx, "login", "", notice)
}

pub async fn register_form(ctx: Ctx) -> Response {
    page(ctx, "register", "")
}

pub async fn forgot_form(ctx: Ctx, Query(q): Query<AuthQuery>) -> Response {
    let notice = if q.sent.as_deref() == Some("1") {
        "forgot_password_sent"
    } else {
        ""
    };
    render_forgot(ctx, "", notice)
}

fn render_account_email(ctx: Ctx, email: &str, error: &str, notice: &str) -> Response {
    let html = AccountEmailPage {
        ctx,
        email: email.to_string(),
        error: error.to_string(),
        notice: notice.to_string(),
    }
    .render()
    .unwrap_or_else(|e| format!("template error: {e}"));
    Html(html).into_response()
}

fn account_email_error(ctx: &Ctx, email: &str, key: &str) -> Response {
    if ctx.is_fragment() {
        (StatusCode::BAD_REQUEST, Html(error_fragment(ctx, key))).into_response()
    } else {
        (
            StatusCode::BAD_REQUEST,
            render_account_email(ctx.clone(), email, key, ""),
        )
            .into_response()
    }
}

pub async fn account_email_form(ctx: Ctx) -> Result<Response, AppError> {
    let Some(user) = ctx.user.as_ref() else {
        return Ok(super::errors::not_found(ctx));
    };
    let state = crate::state::state();
    let Some(account) = db::find_user(&state.db, &user.username)
        .await?
        .filter(|account| account.id == user.id)
    else {
        return Ok(super::errors::not_found(ctx));
    };
    Ok(render_account_email(
        ctx,
        account.email.as_deref().unwrap_or(""),
        "",
        "",
    ))
}

pub async fn update_account_email(
    ctx: Ctx,
    Form(form): Form<AccountEmailForm>,
) -> Result<Response, AppError> {
    let Some(user) = ctx.user.as_ref() else {
        return Ok(super::errors::not_found(ctx));
    };
    let state = crate::state::state();
    let Some(account) = db::find_user(&state.db, &user.username)
        .await?
        .filter(|account| account.id == user.id)
    else {
        return Ok(super::errors::not_found(ctx));
    };
    if !session::verify_password(&state.db, &form.password, &account.password).await {
        return Ok(account_email_error(
            &ctx,
            form.email.as_deref().unwrap_or(""),
            "invalid_credentials",
        ));
    }
    let email = form
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(key) = email_policy(email) {
        return Ok(account_email_error(&ctx, email.unwrap_or(""), key));
    }
    if let Some(email) = email {
        if db::find_user_by_email(&state.db, email)
            .await?
            .is_some_and(|other| other.id != account.id)
        {
            return Ok(account_email_error(&ctx, email, "account_email_taken"));
        }
    }
    let old_email = account.email.clone();
    db::set_user_email(&state.db, account.id, email).await?;
    if let Some(old) = old_email
        .as_deref()
        .filter(|old| !email.is_some_and(|new| old.eq_ignore_ascii_case(new)))
    {
        match send_security_notice(
            &ctx,
            &state.cfg,
            old,
            "security_email_changed_subject",
            "security_email_changed_message",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => tracing::error!(
                user_id = account.id,
                "email-change notice template failed: {e}"
            ),
        }
    }
    if let Some(new) = email.filter(|new| {
        !old_email
            .as_deref()
            .is_some_and(|old| old.eq_ignore_ascii_case(new))
    }) {
        match send_security_notice(
            &ctx,
            &state.cfg,
            new,
            "security_email_changed_subject",
            "security_email_changed_message",
        )
        .await
        {
            Ok(_) => {}
            Err(e) => tracing::error!(
                user_id = account.id,
                "email-change notice template failed: {e}"
            ),
        }
    }
    if ctx.is_fragment() {
        let notice = AccountEmailNotice {
            message: ctx.l.t("account_email_saved"),
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}"));
        return Ok(Html(notice).into_response());
    }
    Ok(render_account_email(
        ctx,
        email.unwrap_or(""),
        "",
        "account_email_saved",
    ))
}

/// The reset request. Every path that reaches the end answers with the same
/// redirect, and nothing on the page says whether the address was found —
/// otherwise this form is a way to ask "does this person have an account?".
pub async fn forgot(ctx: Ctx, Form(form): Form<ForgotForm>) -> Result<Response, AppError> {
    let state = crate::state::state();
    if !crate::turnstile::verify(&state.cfg, form.turnstile.as_deref(), "forgot_password").await {
        return Ok(forgot_error(
            &ctx,
            StatusCode::FORBIDDEN,
            "turnstile_form_failed",
        ));
    }
    let email = form.email.as_deref().map(str::trim).unwrap_or_default();
    // a malformed address cannot belong to an account, so it is skipped
    // without a query; a well-formed one is looked up, and either way the
    // visitor gets the same page
    if email_policy(Some(email)).is_none() {
        if let Some(user) = db::find_user_by_email(&state.db, email).await? {
            send_reset_mail(&ctx, state, &user, email).await?;
        }
    }
    Ok(navigation_response(&ctx, SENT_PATH))
}

pub async fn reset_form(ctx: Ctx, Query(q): Query<AuthQuery>) -> Result<Response, AppError> {
    let state = crate::state::state();
    let token = q.token.unwrap_or_default();
    // checked before the form is shown: typing a new password into a dead link
    // wastes the visitor's time and fails anyway
    let pending = !token.is_empty() && db::reset_token_pending(&state.db, &token).await?;
    if pending {
        Ok(render_reset(ctx, &token, ""))
    } else {
        Ok(render_reset(ctx, "", "reset_password_invalid"))
    }
}

/// Spends the link and sets the new password. The order matters: cheap local
/// validation, then the bot check, and only then the token — a typo or a failed
/// CAPTCHA must not burn a link the visitor still needs.
pub async fn reset(ctx: Ctx, Form(form): Form<ResetForm>) -> Result<Response, AppError> {
    let state = crate::state::state();
    let token = form.token.unwrap_or_default();
    let policy = if form.password.chars().count() < MIN_PASSWORD_LEN {
        Some("register_password_short")
    } else if form.password != form.confirm_password {
        Some("register_mismatch")
    } else {
        None
    };
    if let Some(key) = policy {
        return Ok(reset_error(&ctx, StatusCode::BAD_REQUEST, &token, key));
    }
    if !crate::turnstile::verify(&state.cfg, form.turnstile.as_deref(), "reset_password").await {
        return Ok(reset_error(
            &ctx,
            StatusCode::FORBIDDEN,
            &token,
            "turnstile_form_failed",
        ));
    }
    let Some(user_id) = db::consume_reset_token(&state.db, &token).await? else {
        // the link is gone: expired, already spent, or never real. The page is
        // re-rendered without the token, so it offers a new link instead of a
        // retry that could only fail again.
        return Ok(reset_error(
            &ctx,
            StatusCode::BAD_REQUEST,
            "",
            "reset_password_invalid",
        ));
    };
    let hash = match session::hash_password(&state.db, &form.password).await {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("password hash failed: {e}");
            return Ok(reset_error(
                &ctx,
                StatusCode::INTERNAL_SERVER_ERROR,
                &token,
                "register_failed",
            ));
        }
    };
    db::set_user_password(&state.db, user_id, &hash).await?;
    if let Some(user) = db::find_user_by_id(&state.db, user_id).await? {
        if let Some(email) = user.email.as_deref() {
            if let Err(e) = send_security_notice(
                &ctx,
                &state.cfg,
                email,
                "security_password_reset_subject",
                "security_password_reset_message",
            )
            .await
            {
                tracing::error!(user_id, "password-reset notice delivery failed: {e}");
            }
        }
    }
    // a reset is the moment to assume the old password is known to somebody
    // else, so every session it opened ends here
    state.sessions.end_all_for(user_id);
    tracing::info!(user_id, "password reset completed");
    Ok(navigation_response(&ctx, RESET_DONE_PATH))
}

/// 256 bits of OS randomness as hex. Two v4 UUIDs rather than a new crypto
/// dependency: `uuid` already draws from the process CSPRNG, and a reset token
/// is exactly the value that must not be guessable.
fn reset_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// The absolute base for links inside mail. `[email] site_url` wins because the
/// request's own host is attacker-controlled: a forged `Host` header would
/// otherwise have the app mail the visitor a link to the attacker's site, with
/// a live token appended. The fallback exists so local development works with
/// nothing configured.
fn mail_base(ctx: &Ctx, cfg: &crate::config::Config) -> String {
    let configured = cfg.email.site_url.trim_end_matches('/');
    if !configured.is_empty() {
        return configured.to_string();
    }
    if cfg!(not(debug_assertions)) {
        tracing::warn!(
            "[email] site_url is unset; the reset link is built from the request host, which a forged Host header can redirect"
        );
    }
    ctx.site_url.trim_end_matches('/').to_string()
}

/// Issues a token and mails the link, logging every failure instead of
/// reporting it: the visitor's answer is the same neutral page, so a mail
/// outage must not turn this form into a test of which addresses exist.
async fn send_reset_mail(
    ctx: &Ctx,
    state: &AppState,
    user: &db::User,
    email: &str,
) -> Result<(), AppError> {
    if db::reset_requested_within(&state.db, user.id, RESET_THROTTLE_SECS).await? {
        tracing::info!(
            user_id = user.id,
            "reset asked for again inside the throttle window; not re-sending"
        );
        return Ok(());
    }
    let token = reset_token();
    let link = format!(
        "{}/reset-password?token={token}",
        mail_base(ctx, &state.cfg)
    );
    let minutes = RESET_TTL_SECS / 60;
    let (subject, text, html) = match reset_mail_bodies(ctx, &link, minutes) {
        Ok(bodies) => bodies,
        Err(e) => {
            tracing::error!("reset mail template failed: {e}");
            return Ok(());
        }
    };
    // stored before the send: the link has to work the moment it lands, and a
    // token whose mail never arrived simply expires
    db::issue_reset_token(&state.db, user.id, &token, RESET_TTL_SECS).await?;
    let mail = crate::mail::Mail {
        subject,
        text,
        html,
    };
    if !crate::mail::send(&state.cfg, email, &mail).await {
        tracing::warn!(user_id = user.id, "the reset mail was not accepted");
    }
    Ok(())
}

/// The three parts of the reset mail, rendered from the app's own templates so
/// every string comes out of the .tr files exactly as the pages do.
fn reset_mail_bodies(
    ctx: &Ctx,
    link: &str,
    minutes: i64,
) -> Result<(String, String, String), askama::Error> {
    let html = ResetEmailHtml {
        ctx: ctx.clone(),
        link: link.to_string(),
        minutes,
    }
    .render()?;
    let text = ResetEmailText {
        ctx: ctx.clone(),
        link: link.to_string(),
        minutes,
    }
    .render()?;
    Ok((ctx.l.t("reset_email_subject"), text, html))
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

    let created = db::create_user(&state.db, &form.username, &hash, email).await?;

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

    /// A probe user's reset rows outlive the user row, so the token helper
    /// cleans them too: a leftover token would point at an id that no longer
    /// exists and linger in whatever namespace the tests ran against.
    async fn drop_probe_tokens(pool: &crate::db::Db, user_id: i64) {
        pool.query("DELETE password_reset WHERE user = type::record('user', $u)")
            .bind(("u", user_id))
            .await
            .expect("token cleanup");
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

        for (username, email) in [
            ("cr_email_probe_with", Some("probe@example.com")),
            ("cr_email_probe_without", None),
        ] {
            // a half-finished earlier run must not fail the create
            drop_probe_user(&pool, username).await;
            let created = db::create_user(&pool, username, &hash, email)
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

    /// The tokens, end to end against a live database: a fresh token is
    /// pending, spending it yields the account exactly once, a replayed or
    /// expired one yields nothing, a new request invalidates the previous
    /// link, the throttle window sees the fresh row, and the password the
    /// handler writes verifies against the stored hash. Cleans up after
    /// itself, so a scratch namespace stays scratch.
    #[tokio::test]
    async fn reset_tokens_are_single_use_and_expire() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let username = "cr_reset_probe";
        drop_probe_user(&pool, username).await;
        let hash = session::hash_password(&pool, "old password")
            .await
            .expect("hash");
        let user = db::create_user(&pool, username, &hash, Some("Probe@Example.com"))
            .await
            .expect("create")
            .expect("created");
        drop_probe_tokens(&pool, user.id).await;

        // the lookup ignores case, and an address nobody uses answers None
        // exactly like a row that never carried one
        assert_eq!(
            db::find_user_by_email(&pool, "probe@example.com")
                .await
                .expect("lookup")
                .map(|u| u.id),
            Some(user.id)
        );
        assert!(db::find_user_by_email(&pool, "nobody@example.com")
            .await
            .expect("lookup")
            .is_none());

        let token = reset_token();
        db::issue_reset_token(&pool, user.id, &token, RESET_TTL_SECS)
            .await
            .expect("issue");
        assert!(db::reset_token_pending(&pool, &token)
            .await
            .expect("pending"));
        assert!(
            db::reset_requested_within(&pool, user.id, RESET_THROTTLE_SECS)
                .await
                .expect("throttle"),
            "the row the throttle reads is the one just written"
        );

        assert_eq!(
            db::consume_reset_token(&pool, &token)
                .await
                .expect("consume"),
            Some(user.id)
        );
        assert!(!db::reset_token_pending(&pool, &token)
            .await
            .expect("pending"));
        assert_eq!(
            db::consume_reset_token(&pool, &token)
                .await
                .expect("replay"),
            None,
            "a link works once"
        );

        // an expired token is refused the same way a spent one is
        let stale = reset_token();
        db::issue_reset_token(&pool, user.id, &stale, -1)
            .await
            .expect("issue stale");
        assert!(!db::reset_token_pending(&pool, &stale)
            .await
            .expect("pending"));
        assert_eq!(
            db::consume_reset_token(&pool, &stale)
                .await
                .expect("consume stale"),
            None
        );

        // asking again kills the previous link rather than leaving two live
        let first = reset_token();
        let second = reset_token();
        db::issue_reset_token(&pool, user.id, &first, RESET_TTL_SECS)
            .await
            .expect("issue first");
        db::issue_reset_token(&pool, user.id, &second, RESET_TTL_SECS)
            .await
            .expect("issue second");
        assert!(!db::reset_token_pending(&pool, &first)
            .await
            .expect("pending"));
        assert!(db::reset_token_pending(&pool, &second)
            .await
            .expect("pending"));

        // the password the reset handler writes is the one that verifies, and
        // the reset leaves the email alone
        let new_hash = session::hash_password(&pool, "brand new password")
            .await
            .expect("hash");
        db::set_user_password(&pool, user.id, &new_hash)
            .await
            .expect("set");
        assert_eq!(
            stored_email(&pool, username).await.as_deref(),
            Some("Probe@Example.com")
        );
        let after = db::find_user(&pool, username)
            .await
            .expect("find")
            .expect("row");
        assert!(session::verify_password(&pool, "brand new password", &after.password).await);
        assert!(!session::verify_password(&pool, "old password", &after.password).await);

        drop_probe_tokens(&pool, user.id).await;
        drop_probe_user(&pool, username).await;
        assert!(db::find_user(&pool, username)
            .await
            .expect("find")
            .is_none());
    }

    /// The mail is rendered by the app's own templates, so its copy comes from
    /// the .tr files and its link is the one the handler built — in the plain
    /// body as well as the HTML one, since a text-only client shows the
    /// former.
    #[test]
    fn reset_mail_bodies_carry_the_link_and_the_translated_copy() {
        let ctx = crate::testutil::test_ctx("en");
        let link = "https://cookierun.example/reset-password?token=deadbeef";
        let (subject, body, html) = reset_mail_bodies(&ctx, link, 30).expect("render");
        assert_eq!(subject, "Reset your password");
        assert!(body.contains(link), "{body}");
        assert!(html.contains(&format!("href=\"{link}\"")), "{html}");
        assert!(body.contains("30 minutes"), "{body}");
        assert!(html.contains("30 minutes"), "{html}");
        assert!(
            !html.contains("<style"),
            "mail clients drop <style>, so the template carries none: {html}"
        );

        let th = crate::testutil::test_ctx("th");
        let (th_subject, th_body, _) = reset_mail_bodies(&th, link, 30).expect("render");
        assert_eq!(th_subject, "รีเซ็ตรหัสผ่าน");
        assert!(
            th_body.contains(link),
            "the link itself is never translated: {th_body}"
        );
    }

    /// The link inside a mail comes from `[email] site_url` whenever it is
    /// set, because the request's own host is attacker-controlled: a forged
    /// `Host` header would otherwise aim the visitor's reset link at another
    /// site. With nothing configured it falls back to the request's host, so
    /// local development still works.
    #[test]
    fn mail_base_prefers_the_configured_site_url() {
        let ctx = crate::testutil::test_ctx("en");
        let mut cfg = crate::config::Config::default();
        assert_eq!(mail_base(&ctx, &cfg), "http://localhost:6785");
        cfg.email.site_url = "https://cookierun.example/".into();
        assert_eq!(mail_base(&ctx, &cfg), "https://cookierun.example");
    }

    /// A reset token is 256 bits of hex from the OS CSPRNG — enough that
    /// guessing one is not a strategy, and long enough that two never repeat.
    #[test]
    fn reset_tokens_are_long_random_hex() {
        let a = reset_token();
        let b = reset_token();
        assert_eq!(a.len(), 64, "{a}");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
        assert_ne!(a, b);
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
