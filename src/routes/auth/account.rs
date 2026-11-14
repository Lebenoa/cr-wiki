//! The account page: the recovery address a signed-in visitor can change,
//! and the security notices that go out when it changes.
//!
//! Changing the address asks for the current password, because the address is
//! the reset channel — a hijacked session must not be able to aim it at
//! somewhere the owner cannot see. Both the old and the new address are then
//! told what happened, so a change made without the owner's knowledge does
//! not go unnoticed.

use askama::Template;
use axum::extract::Multipart;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use serde_json::json;

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
    /// resolved URL of the account's avatar, `None` renders the initial
    /// circle
    avatar_src: Option<String>,
    /// the URL the visitor submitted, kept so a rejected "set from URL"
    /// attempt does not lose what they typed
    avatar_url: String,
    /// translation key of the avatar-URL form's error; empty means none
    avatar_error: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountEmailForm {
    pub email: Option<String>,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct AvatarUrlForm {
    pub url: String,
}

#[derive(Template)]
#[template(path = "avatar_url_error.html")]
struct AvatarUrlError {
    message: String,
}

/// What the "set from URL" avatar form shows on a full-page re-render: the
/// visitor's typed URL and the error key, both empty on a plain visit. One
/// parameter instead of two keeps the render helper under the arg lint bar.
#[derive(Clone, Copy)]
struct AvatarUrlFormState<'a> {
    url: &'a str,
    error: &'a str,
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
    avatar_src: Option<String>,
    avatar: AvatarUrlFormState<'_>,
) -> Response {
    let html = AccountEmailPage {
        ctx,
        email: email.to_string(),
        error: error.to_string(),
        notice: notice.to_string(),
        google,
        avatar_src,
        avatar_url: avatar.url.to_string(),
        avatar_error: avatar.error.to_string(),
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
        let avatar = AvatarUrlFormState { url: "", error: "" };
        (
            StatusCode::BAD_REQUEST,
            render_account_email(ctx.clone(), email, key, "", google, None, avatar),
        )
            .into_response()
    }
}

/// The account row behind the signed-in session: what the settings page
/// renders from. `None` when the session names a user who no longer exists
/// (deleted account, seed churn) — the page answers 404 like every other
/// stale-session surface.
async fn current_account(user: &db::sessions::SessionUser) -> Result<Option<db::User>, AppError> {
    let state = crate::state::state();
    Ok(db::find_user(&state.db, &user.username)
        .await?
        .filter(|account| account.id == user.id))
}

pub async fn settings_form(ctx: Ctx) -> Result<Response, AppError> {
    let Some(user) = ctx.user.as_ref() else {
        let avatar = AvatarUrlFormState { url: "", error: "" };
        return Ok(render_account_email(ctx, "", "", "", false, None, avatar));
    };
    let Some(account) = current_account(user).await? else {
        return Ok(errors::not_found(ctx));
    };
    let avatar = AvatarUrlFormState { url: "", error: "" };
    Ok(render_account_email(
        ctx,
        account.email.as_deref().unwrap_or(""),
        "",
        "",
        account.google_sub.is_some(),
        crate::imagehost::resolve_avatar(account.avatar.as_deref().unwrap_or("")),
        avatar,
    ))
}

/// 2 MB: an avatar is a square thumbnail, not a banner; the image host's own
/// 20 MB ceiling would let one account park a poster on every page render.
pub const AVATAR_MAX_BYTES: usize = 2 * 1024 * 1024;

/// The signed-in uploader behind the settings page's avatar file input.
/// Answers JSON like the news thumbnail uploader (`static/js/avatar_upload.js`
/// is its only caller): the vanity goes into the account row and the open
/// sessions, the page reload shows it.
///
/// The vanity carries the record id and the upload instant — deterministic
/// per account per upload, so a re-upload never trips the host's
/// vanity-exists rejection and every URL the page already rendered goes
/// stale-safe (the old file keeps its own name).
pub async fn upload_avatar(ctx: Ctx, mut form: Multipart) -> Response {
    let Some(user) = ctx.user.as_ref() else {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response();
    };
    while let Ok(Some(field)) = form.next_field().await {
        if field.name() != Some("image") {
            continue;
        }
        let content_type = field.content_type().unwrap_or_default().to_string();
        let Some(ext) = crate::upload::extension_for(&content_type) else {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({ "error": "unsupported image type" })),
            )
                .into_response();
        };
        let Ok(bytes) = field.bytes().await else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "could not read upload" })),
            )
                .into_response();
        };
        if bytes.is_empty() || bytes.len() > AVATAR_MAX_BYTES {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({ "error": "image too large (2 MB max)" })),
            )
                .into_response();
        }
        // the declared Content-Type is client input: verify the bytes are
        // actually the image format they claim before shipping them upstream
        if crate::upload::sniff_image(&bytes) != Some(ext) {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({ "error": "file content does not match its type" })),
            )
                .into_response();
        }

        let vanity = format!("avatar-{}-{}", user.id, crate::time::now_unix());
        match crate::imagehost::upload(&vanity, &content_type, &bytes).await {
            Ok(_) => {
                let state = crate::state::state();
                if let Err(e) = db::set_user_avatar(&state.db, &user.id, Some(&vanity)).await {
                    tracing::error!("avatar persist {id}: {e}", id = user.id);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": "avatar stored but could not be saved" })),
                    )
                        .into_response();
                }
                // the session snapshots carry the old (or no) avatar; rewrite
                // them so navbar/profile pick it up on the very next render
                if let Err(e) =
                    db::sessions::refresh_avatars(&state.db, &user.id, Some(&vanity)).await
                {
                    tracing::error!("avatar session refresh {id}: {e}", id = user.id);
                }
                return Json(json!({ "image": vanity })).into_response();
            }
            Err(crate::imagehost::UploadError::NotConfigured) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "image host not configured" })),
                )
                    .into_response();
            }
            Err(crate::imagehost::UploadError::TooLarge) => {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(json!({ "error": "image too large (2 MB max)" })),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::warn!("avatar upload: {e}");
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": "image host refused the upload" })),
                )
                    .into_response();
            }
        }
    }
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "no image field" })),
    )
        .into_response()
}

/// The signed-in "set from URL" sibling of `upload_avatar`: fetches the
/// pasted link — validated, not trusted — and re-hosts it through the image
/// host like any upload, so the account row keeps holding the same kind of
/// value (a bare vanity) and every render path stays unchanged. The URL must
/// be a public `http(s)` address whose response carries an `image/*`
/// `Content-Type` whose bytes actually are a supported raster; anything else
/// answers the form with a translated message. Errors come back as the
/// fragment the htmx form swaps into `#avatar-url-error`, or the full page
/// for a plain POST — which also keeps what the visitor typed.
pub async fn set_avatar_url(ctx: Ctx, Form(form): Form<AvatarUrlForm>) -> Response {
    let Some(user) = ctx.user.as_ref() else {
        return errors::not_found(ctx);
    };
    let url = form.url.trim();
    if crate::state::state().cfg.images.origin().is_empty() {
        return avatar_url_error(
            &ctx,
            url,
            "avatar_host_not_configured",
            StatusCode::NOT_FOUND,
        )
        .await;
    }
    let (content_type, bytes) = match crate::imagehost::fetch_image(url, AVATAR_MAX_BYTES).await {
        Ok(fetched) => fetched,
        Err(e) => {
            tracing::info!(user_id = user.id, "avatar url rejected: {e}");
            let (status, key) = match e {
                crate::imagehost::FetchError::InvalidUrl => {
                    (StatusCode::BAD_REQUEST, "avatar_url_invalid")
                }
                crate::imagehost::FetchError::ForbiddenHost => {
                    (StatusCode::BAD_REQUEST, "avatar_url_internal")
                }
                crate::imagehost::FetchError::TooManyRedirects
                | crate::imagehost::FetchError::Unreachable(_) => {
                    (StatusCode::BAD_GATEWAY, "avatar_url_fetch_failed")
                }
                crate::imagehost::FetchError::TooLarge => {
                    (StatusCode::PAYLOAD_TOO_LARGE, "avatar_url_too_large")
                }
                crate::imagehost::FetchError::NotImage => {
                    (StatusCode::UNSUPPORTED_MEDIA_TYPE, "avatar_url_not_image")
                }
                crate::imagehost::FetchError::BadContent => {
                    (StatusCode::UNSUPPORTED_MEDIA_TYPE, "avatar_url_bad_content")
                }
            };
            return avatar_url_error(&ctx, url, key, status).await;
        }
    };

    let vanity = format!("avatar-{}-{}", user.id, crate::time::now_unix());
    if let Err(e) = crate::imagehost::upload(&vanity, &content_type, &bytes).await {
        tracing::warn!(user_id = user.id, "avatar url rehost: {e}");
        let (status, key) = match e {
            crate::imagehost::UploadError::NotConfigured => {
                (StatusCode::NOT_FOUND, "avatar_host_not_configured")
            }
            crate::imagehost::UploadError::TooLarge => {
                (StatusCode::PAYLOAD_TOO_LARGE, "avatar_url_too_large")
            }
            _ => (StatusCode::BAD_GATEWAY, "avatar_save_failed"),
        };
        return avatar_url_error(&ctx, url, key, status).await;
    }
    let state = crate::state::state();
    if let Err(e) = db::set_user_avatar(&state.db, &user.id, Some(&vanity)).await {
        tracing::error!("avatar persist {id}: {e}", id = user.id);
        return avatar_url_error(
            &ctx,
            url,
            "avatar_save_failed",
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .await;
    }
    // the session snapshots carry the old (or no) avatar; rewrite them so
    // navbar/profile pick it up on the very next render
    if let Err(e) = db::sessions::refresh_avatars(&state.db, &user.id, Some(&vanity)).await {
        tracing::error!("avatar session refresh {id}: {e}", id = user.id);
    }
    super::navigation_response(&ctx, "/settings")
}

/// One rejection of the "set from URL" form, both shapes: the htmx request
/// swaps the translated message into `#avatar-url-error` without losing the
/// typed URL; everything else gets the whole settings page again, error and
/// URL preserved.
async fn avatar_url_error(ctx: &Ctx, url: &str, key: &str, status: StatusCode) -> Response {
    if ctx.is_fragment() {
        let fragment = AvatarUrlError {
            message: ctx.l.t(key),
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}"));
        return (status, Html(fragment)).into_response();
    }
    let Some(user) = ctx.user.as_ref() else {
        return errors::not_found(ctx.clone());
    };
    let Some(account) = current_account(user).await.ok().flatten() else {
        return errors::not_found(ctx.clone());
    };
    let avatar = AvatarUrlFormState { url, error: key };
    (
        status,
        render_account_email(
            ctx.clone(),
            account.email.as_deref().unwrap_or(""),
            "",
            "",
            account.google_sub.is_some(),
            crate::imagehost::resolve_avatar(account.avatar.as_deref().unwrap_or("")),
            avatar,
        ),
    )
        .into_response()
}

/// Removes the account's avatar: the row field goes, the sessions refresh,
/// and the page reload renders the initial circle again. The image host has
/// no delete endpoint, so the bytes stay there under their vanity — unreferenced,
/// invisible unless someone guesses the name.
pub async fn remove_avatar(ctx: Ctx) -> Response {
    let Some(user) = ctx.user.as_ref() else {
        return errors::not_found(ctx);
    };
    let state = crate::state::state();
    if let Err(e) = db::set_user_avatar(&state.db, &user.id, None).await {
        tracing::error!("avatar remove {id}: {e}", id = user.id);
        return errors::server_error(ctx);
    }
    if let Err(e) = db::sessions::refresh_avatars(&state.db, &user.id, None).await {
        tracing::error!("avatar session refresh {id}: {e}", id = user.id);
    }
    Redirect::to("/settings").into_response()
}

pub async fn update_account_email(
    ctx: Ctx,
    Form(form): Form<AccountEmailForm>,
) -> Result<Response, AppError> {
    let Some(user) = ctx.user.as_ref() else {
        return Ok(errors::not_found(ctx));
    };
    let Some(account) = current_account(user).await? else {
        return Ok(errors::not_found(ctx));
    };
    let state = crate::state::state();
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
        notify_email_change(&ctx, &state.cfg, &account.id, old).await;
    }
    if let Some(new) = email.filter(|new| {
        !old_email
            .as_deref()
            .is_some_and(|old| old.eq_ignore_ascii_case(new))
    }) {
        notify_email_change(&ctx, &state.cfg, &account.id, new).await;
    }
    if ctx.is_fragment() {
        let notice = AccountEmailNotice {
            message: ctx.l.t("account_email_saved"),
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}"));
        return Ok(Html(notice).into_response());
    }
    let avatar_src = crate::imagehost::resolve_avatar(
        ctx.user
            .as_ref()
            .and_then(|u| u.avatar.clone())
            .as_deref()
            .unwrap_or(""),
    );
    let avatar = AvatarUrlFormState { url: "", error: "" };
    Ok(render_account_email(
        ctx,
        email.unwrap_or(""),
        "",
        "account_email_saved",
        google,
        avatar_src,
        avatar,
    ))
}

/// Security mail when the recovery address changes: the old address is told
/// so a hijacked session cannot repoint it silently, the new one so the new
/// owner can confirm. Best-effort — the row already changed, so a mailing
/// failure only logs with the account id.
async fn notify_email_change(ctx: &Ctx, cfg: &crate::config::Config, user_id: &str, addr: &str) {
    match send_security_notice(
        ctx,
        cfg,
        addr,
        "security_email_changed_subject",
        "security_email_changed_message",
    )
    .await
    {
        Ok(_) => {}
        Err(e) => tracing::error!(user_id, "email-change notice template failed: {e}"),
    }
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
            ctx.user = Some(crate::db::sessions::SessionUser {
                id: "1".into(),
                username: "tester".into(),
                is_admin: false,
                avatar: None,
            });
            AccountEmailPage {
                ctx,
                email: "ginger@cookie.run".into(),
                error: String::new(),
                notice: String::new(),
                google,
                avatar_src: None,
                avatar_url: String::new(),
                avatar_error: String::new(),
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
