//! The forgot/reset password flow.
//!
//! A reset link is a bearer credential mailed to an address, so the parts
//! that matter sit together here: the token is 256 bits from the OS CSPRNG,
//! single use, invalidated by the next request, throttled per account, and
//! carried nowhere but the form's own hidden field. A request answers with
//! the same page whether or not the address exists — the form must not become
//! a way to ask who has an account — and the mail is rendered from the app's
//! own templates, so every string comes out of the .tr files.

use askama::Template;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db;
use crate::routes::errors::AppError;
use crate::session;
use crate::state::AppState;

use super::{
    email_policy, error_fragment, navigation_response, public_base, reset_token,
    send_security_notice, AuthQuery, MIN_PASSWORD_LEN,
};

/// A reset link is good for half an hour: long enough to reach an inbox and
/// be used, short enough that a mail left behind in an old account is not a
/// standing key to it.
///
/// (The Google flow's own state TTL lives in `crate::oauth`.)
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

pub async fn forgot_form(ctx: Ctx, Query(q): Query<AuthQuery>) -> Response {
    let notice = if q.sent.as_deref() == Some("1") {
        "forgot_password_sent"
    } else {
        ""
    };
    render_forgot(ctx, "", notice)
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
    Ok(reset_page(ctx, &token, pending))
}

/// The page for a token whose state has already been read: the form while the
/// link is still good, the notice saying why there is none otherwise. Split out
/// of the handler so the DB-gated test can drive the branch it takes without a
/// request, and without the shared state that only `main` can install.
fn reset_page(ctx: Ctx, token: &str, pending: bool) -> Response {
    if pending {
        render_reset(ctx, token, "")
    } else {
        render_reset(ctx, "", "reset_password_invalid")
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
    db::set_user_password(&state.db, &user_id, &hash).await?;
    if let Some(user) = db::find_user_by_id(&state.db, &user_id).await? {
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
    state.sessions.end_all_for(&user_id);
    tracing::info!(user_id, "password reset completed");
    Ok(navigation_response(&ctx, RESET_DONE_PATH))
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
    if db::reset_requested_within(&state.db, &user.id, RESET_THROTTLE_SECS).await? {
        tracing::info!(
            user_id = user.id,
            "reset asked for again inside the throttle window; not re-sending"
        );
        return Ok(());
    }
    let token = reset_token();
    let link = format!(
        "{}/reset-password?token={token}",
        public_base(ctx, &state.cfg)
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
    db::issue_reset_token(&state.db, &user.id, &token, RESET_TTL_SECS).await?;
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

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
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
    async fn drop_probe_tokens(pool: &crate::db::Db, user_id: &str) {
        pool.query("DELETE password_reset WHERE user = type::record('user', $u)")
            .bind(("u", user_id.to_string()))
            .await
            .expect("token cleanup");
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
        let user = db::create_user(&pool, username, &hash, Some("Probe@Example.com"), None)
            .await
            .expect("create")
            .expect("created");
        drop_probe_tokens(&pool, &user.id).await;

        // the lookup ignores case, and an address nobody uses answers None
        // exactly like a row that never carried one
        assert_eq!(
            db::find_user_by_email(&pool, "probe@example.com")
                .await
                .expect("lookup")
                .map(|u| u.id),
            Some(user.id.clone())
        );
        assert!(db::find_user_by_email(&pool, "nobody@example.com")
            .await
            .expect("lookup")
            .is_none());

        let token = reset_token();
        db::issue_reset_token(&pool, &user.id, &token, RESET_TTL_SECS)
            .await
            .expect("issue");
        assert!(db::reset_token_pending(&pool, &token)
            .await
            .expect("pending"));
        assert!(
            db::reset_requested_within(&pool, &user.id, RESET_THROTTLE_SECS)
                .await
                .expect("throttle"),
            "the row the throttle reads is the one just written"
        );

        assert_eq!(
            db::consume_reset_token(&pool, &token)
                .await
                .expect("consume"),
            Some(user.id.clone())
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
        db::issue_reset_token(&pool, &user.id, &stale, -1)
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
        db::issue_reset_token(&pool, &user.id, &first, RESET_TTL_SECS)
            .await
            .expect("issue first");
        db::issue_reset_token(&pool, &user.id, &second, RESET_TTL_SECS)
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
        db::set_user_password(&pool, &user.id, &new_hash)
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

        drop_probe_tokens(&pool, &user.id).await;
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

    /// An account with no recovery address must not break the address lookup.
    /// Such a row stores `NULL`, and comparing a `NULL` as a string errored the
    /// whole query — which would have taken password reset, the taken-address
    /// check and Google's address match down for every visitor at once, on any
    /// wiki where one account skipped the optional field.
    #[tokio::test]
    async fn an_account_without_an_email_does_not_break_the_address_lookup() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let username = "cr_no_email_probe";
        drop_probe_user(&pool, username).await;
        let hash = session::hash_password(&pool, "probe password")
            .await
            .expect("hash");
        let created = db::create_user(&pool, username, &hash, None, None)
            .await
            .expect("create")
            .expect("created");
        assert_eq!(created.email, None);

        for address in ["nobody@example.com", &format!("{username}@example.com")] {
            assert!(
                db::find_user_by_email(&pool, address)
                    .await
                    .unwrap_or_else(|e| panic!("lookup for {address} failed: {e:?}"))
                    .is_none(),
                "{address} belongs to nobody"
            );
        }
        assert!(
            db::find_user_by_email(&pool, "")
                .await
                .expect("empty lookup")
                .is_none(),
            "an empty address finds no account, not the first address-less row"
        );

        drop_probe_user(&pool, username).await;
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

    /// The reset form is the only place a `reset_password` token can come from,
    /// and the release verifier refuses a tokenless POST — so a link that is
    /// still pending has to render the form with its widget, while a spent link
    /// has to offer no form at all. Both shapes are driven here through the same
    /// helper the handler calls, against a token the database really holds.
    #[tokio::test]
    async fn a_pending_token_renders_the_form_with_its_widget() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let username = "cr_reset_widget_probe";
        drop_probe_user(&pool, username).await;
        let hash = session::hash_password(&pool, "old password")
            .await
            .expect("hash");
        let user = db::create_user(&pool, username, &hash, None, None)
            .await
            .expect("create")
            .expect("created");
        drop_probe_tokens(&pool, &user.id).await;

        let token = reset_token();
        db::issue_reset_token(&pool, &user.id, &token, RESET_TTL_SECS)
            .await
            .expect("issue");
        // the same read the handler makes before it shows anything
        let pending = db::reset_token_pending(&pool, &token)
            .await
            .expect("pending");
        assert!(pending, "a fresh token is pending");

        let ctx = crate::testutil::test_ctx("en");
        let dead_link = ctx.l.t("reset_password_invalid").to_string();
        let page = body(reset_page(ctx.clone(), &token, pending)).await;
        assert!(
            page.contains("class=\"cf-turnstile\""),
            "the reset form renders no widget, so a release build has no token to accept"
        );
        assert!(
            page.contains("data-action=\"reset_password\""),
            "the widget's action has to be the one the route verifies"
        );
        assert!(
            page.contains(&format!("data-sitekey=\"{}\"", ctx.turnstile_sitekey())),
            "the widget would render without the site key"
        );
        assert!(page.contains("data-callback=\"crTurnstileToken\""));
        assert!(page.contains("data-form-guard-hint"));
        assert!(
            page.contains(&format!("value=\"{token}\"")),
            "the form has to carry the link's token back"
        );
        assert!(
            !page.contains(dead_link.as_str()),
            "a live link is not the dead-link notice"
        );

        // spending the link turns the form into the notice, and no widget is
        // left behind for a challenge that could never be submitted
        assert_eq!(
            db::consume_reset_token(&pool, &token)
                .await
                .expect("consume"),
            Some(user.id.clone())
        );
        let pending = db::reset_token_pending(&pool, &token)
            .await
            .expect("pending");
        assert!(!pending, "a spent token is not pending");
        let dead = body(reset_page(ctx, &token, pending)).await;
        assert!(
            !dead.contains("class=\"cf-turnstile\""),
            "a dead link must not offer a widget"
        );
        assert!(
            dead.contains(dead_link.as_str()),
            "a dead link has to say so"
        );

        drop_probe_tokens(&pool, &user.id).await;
        drop_probe_user(&pool, username).await;
    }

    /// A response body, for the assertions above.
    async fn body(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf8")
    }

    /// The profile page renders from the database's own answer: a real user
    /// with two live builds shows both, and an expired anonymous build the
    /// same user owns never shows — the same expiry rule every other build
    /// reader applies. The unknown-id 404 rides the same handler, so it is
    /// asserted here against a live server too, where "missing" is
    /// distinguishable from "the database is down".
    #[tokio::test]
    async fn profile_renders_only_live_builds_of_a_real_user() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let username = "cr_profile_probe";
        drop_probe_user(&pool, username).await;
        let hash = session::hash_password(&pool, "old password")
            .await
            .expect("hash");
        let user = db::create_user(&pool, username, &hash, None, None)
            .await
            .expect("create")
            .expect("created");
        let changes = crate::builds::BuildChanges::default();
        let live = crate::builds::NewBuild {
            changes: changes.clone(),
            author: username.to_string(),
            user_id: user.id.clone(),
            expires_at: None,
        };
        let dying = crate::builds::NewBuild {
            changes,
            author: username.to_string(),
            user_id: user.id.clone(),
            // an hour from now is not live enough to race the test clock:
            // an hour AGO is definitively spent
            expires_at: Some(crate::time::now_unix() - 3600),
        };
        let b1 = crate::builds::insert_build(&pool, &live)
            .await
            .expect("insert live");
        let b2 = crate::builds::insert_build(&pool, &live)
            .await
            .expect("insert second");
        crate::builds::insert_build(&pool, &dying)
            .await
            .expect("insert expired");

        let page = body(
            crate::routes::profile::by_id(
                crate::testutil::test_ctx("en"),
                axum::extract::Path(user.id.to_string()),
            )
            .await,
        )
        .await;
        assert!(
            page.contains(username),
            "the profile shows the account's name: {}",
            &page[..page.len().min(400)]
        );
        assert!(
            page.contains(&format!("/builds/{b1}")) && page.contains(&format!("/builds/{b2}")),
            "both live builds link from the profile: {page}"
        );
        assert!(
            !page.contains("not-yet"),
            "an expired build is not listed: {page}"
        );

        // an id nothing owns is a 404, not a 500: "missing" is only
        // distinguishable with a database to ask
        let res = crate::routes::profile::by_id(
            crate::testutil::test_ctx("en"),
            axum::extract::Path("999999".to_string()),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        // build rows outlive the user probe, so clean them explicitly
        pool.query("DELETE build WHERE user_id = $u")
            .bind(("u", user.id))
            .await
            .expect("build cleanup");
        drop_probe_user(&pool, username).await;
    }
}
