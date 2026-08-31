//! Login, register and logout.

use askama::Template;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db;
use crate::session::{self, SessionUser, SESSION_COOKIE};
use crate::state::AppState;

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
        if self.is_register() { "register" } else { "login" }
    }

    fn submit_key(&self) -> &'static str {
        if self.is_register() { "register_button_text" } else { "login_button_text" }
    }

    fn loading_key(&self) -> &'static str {
        if self.is_register() { "register_loading" } else { "login_loading" }
    }

    /// "Don't have an account?" / "Already have an account?"
    fn switch_prompt_key(&self) -> &'static str {
        if self.is_register() { "already_have_account" } else { "dont_have_account" }
    }

    fn other_mode(&self) -> &'static str {
        if self.is_register() { "login" } else { "register" }
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

fn page(ctx: Ctx, mode: &str, error: &str) -> Response {
    let html = AuthPage { ctx, mode: mode.to_string(), error: error.to_string() }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}"));
    Html(html).into_response()
}

pub async fn login_form(ctx: Ctx) -> Response {
    page(ctx, "login", "")
}

pub async fn register_form(ctx: Ctx) -> Response {
    page(ctx, "register", "")
}

pub async fn login(
    State(state): State<AppState>,
    ctx: Ctx,
    Form(form): Form<LoginForm>,
) -> Response {
    if !crate::turnstile::verify(&state.cfg, form.turnstile.as_deref(), "login").await {
        return (StatusCode::FORBIDDEN, page(ctx, "login", "turnstile_form_failed")).into_response();
    }
    if form.username.is_empty() || form.password.is_empty() {
        return (StatusCode::BAD_REQUEST, page(ctx, "login", "invalid_credentials")).into_response();
    }
    let found = db::find_user(&state.db, &form.username).await.unwrap_or(None);

    let Some(user) = found else {
        // no such user: still pay for a hash, so the answer takes as long as
        // a real check and cannot be told apart by timing
        session::waste_time(&state.db, &form.password).await;
        return (StatusCode::NOT_FOUND, page(ctx, "login", "invalid_credentials")).into_response();
    };
    if !session::verify_password(&state.db, &form.password, &user.password).await {
        return (StatusCode::NOT_FOUND, page(ctx, "login", "invalid_credentials")).into_response();
    }

    let key = state.sessions.start(SessionUser {
        id: user.id,
        username: user.username,
        is_admin: user.is_admin,
    });
    let mut res = Redirect::to("/").into_response();
    set_session_cookie(res.headers_mut(), &key);
    res
}

pub async fn register(
    State(state): State<AppState>,
    ctx: Ctx,
    Form(form): Form<RegisterForm>,
) -> Response {
    if !crate::turnstile::verify(&state.cfg, form.turnstile.as_deref(), "register").await {
        return (StatusCode::FORBIDDEN, page(ctx, "register", "turnstile_form_failed"))
            .into_response();
    }
    if form.username.is_empty() || form.password.is_empty() {
        return (StatusCode::BAD_REQUEST, page(ctx, "register", "register_required")).into_response();
    }
    if form.password != form.confirm_password {
        return (StatusCode::BAD_REQUEST, page(ctx, "register", "register_mismatch")).into_response();
    }
    let Ok(hash) = session::hash_password(&state.db, &form.password).await else {
        return (StatusCode::INTERNAL_SERVER_ERROR, page(ctx, "register", "register_failed"))
            .into_response();
    };

    let created = db::create_user(&state.db, &form.username, &hash)
        .await
        .unwrap_or(None);

    let Some(user) = created else {
        return (StatusCode::BAD_REQUEST, page(ctx, "register", "register_taken")).into_response();
    };
    let key = state.sessions.start(SessionUser {
        id: user.id,
        username: user.username,
        is_admin: user.is_admin,
    });
    let mut res = Redirect::to("/").into_response();
    set_session_cookie(res.headers_mut(), &key);
    res
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
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

/// HttpOnly so script cannot read the session, SameSite=Lax so it survives a
/// normal navigation but not a cross-site form post.
fn set_session_cookie(headers: &mut HeaderMap, key: &str) {
    if let Ok(v) =
        HeaderValue::from_str(&format!("{SESSION_COOKIE}={key}; path=/; HttpOnly; SameSite=Lax"))
    {
        headers.append(header::SET_COOKIE, v);
    }
}
