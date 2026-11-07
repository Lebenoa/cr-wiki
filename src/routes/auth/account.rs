//! The account page: the recovery address a signed-in visitor can change,
//! and the security notices that go out when it changes.
//!
//! Changing the address asks for the current password, because the address is
//! the reset channel — a hijacked session must not be able to aim it at
//! somewhere the owner cannot see. Both the old and the new address are then
//! told what happened, so a change made without the owner's knowledge does
//! not go unnoticed.

use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Form;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db;
use crate::routes::errors::{self, AppError};
use crate::session;

use super::{email_policy, error_fragment, send_security_notice};

#[derive(Template)]
#[template(path = "settings.html")]
struct AccountEmailPage {
    ctx: Ctx,
    email: String,
    error: String,
    notice: String,
    /// a Google-linked account has no password its owner knows, so the page
    /// says how to set one instead of leaving the form a dead end
    google: bool,
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

fn render_account_email(
    ctx: Ctx,
    email: &str,
    error: &str,
    notice: &str,
    google: bool,
) -> Response {
    let html = AccountEmailPage {
        ctx,
        email: email.to_string(),
        error: error.to_string(),
        notice: notice.to_string(),
        google,
    }
    .render()
    .unwrap_or_else(|e| format!("template error: {e}"));
    (
        [(axum::http::header::CACHE_CONTROL, "private, no-store")],
        Html(html),
    )
        .into_response()
}

fn account_email_error(ctx: &Ctx, email: &str, key: &str, google: bool) -> Response {
    if ctx.is_fragment() {
        (StatusCode::BAD_REQUEST, Html(error_fragment(ctx, key))).into_response()
    } else {
        (
            StatusCode::BAD_REQUEST,
            render_account_email(ctx.clone(), email, key, "", google),
        )
            .into_response()
    }
}

pub async fn settings_form(ctx: Ctx) -> Result<Response, AppError> {
    let Some(user) = ctx.user.as_ref() else {
        return Ok(render_account_email(ctx, "", "", "", false));
    };
    let state = crate::state::state();
    let Some(account) = db::find_user(&state.db, &user.username)
        .await?
        .filter(|account| account.id == user.id)
    else {
        return Ok(errors::not_found(ctx));
    };
    Ok(render_account_email(
        ctx,
        account.email.as_deref().unwrap_or(""),
        "",
        "",
        account.google_sub.is_some(),
    ))
}

pub async fn update_account_email(
    ctx: Ctx,
    Form(form): Form<AccountEmailForm>,
) -> Result<Response, AppError> {
    let Some(user) = ctx.user.as_ref() else {
        return Ok(errors::not_found(ctx));
    };
    let state = crate::state::state();
    let Some(account) = db::find_user(&state.db, &user.username)
        .await?
        .filter(|account| account.id == user.id)
    else {
        return Ok(errors::not_found(ctx));
    };
    let google = account.google_sub.is_some();
    if !session::verify_password(&state.db, &form.password, &account.password).await {
        return Ok(account_email_error(
            &ctx,
            form.email.as_deref().unwrap_or(""),
            "invalid_credentials",
            google,
        ));
    }
    let email = form
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(key) = email_policy(email) {
        return Ok(account_email_error(&ctx, email.unwrap_or(""), key, google));
    }
    if let Some(email) = email {
        if db::find_user_by_email(&state.db, email)
            .await?
            .is_some_and(|other| other.id != account.id)
        {
            return Ok(account_email_error(
                &ctx,
                email,
                "account_email_taken",
                google,
            ));
        }
    }
    let old_email = account.email.clone();
    db::set_user_email(&state.db, &account.id, email).await?;
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
        google,
    ))
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    /// The hint appears only for an account that actually signs in with
    /// Google; an account with a password of its own would be told to set one
    /// it already has.
    #[test]
    fn google_hint_appears_only_for_a_google_account() {
        let account = |google: bool| {
            let mut ctx = crate::testutil::test_ctx("en");
            ctx.user = Some(crate::session::SessionUser {
                id: "1".into(),
                username: "tester".into(),
                is_admin: false,
            });
            AccountEmailPage {
                ctx,
                email: "ginger@cookie.run".into(),
                error: String::new(),
                notice: String::new(),
                google,
            }
            .render()
            .expect("render")
        };
        assert!(
            account(true).contains("signs in with Google"),
            "{}",
            account(true)
        );
        assert!(
            !account(false).contains("signs in with Google"),
            "{}",
            account(false)
        );
    }
}
