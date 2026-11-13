//! Google sign-in: the consent redirect, the callback, and the account it
//! resolves to.
//!
//! Nothing in the callback's query string is trusted. The state has to match
//! the `HttpOnly` cookie this browser carries *and* be unspent and young: the
//! store alone cannot tie a callback to a browser, since a URL started on an
//! attacker's machine and handed to a victim would otherwise sign the victim
//! in as the attacker. Only a Google-*verified* address may match an existing
//! account, an account already carrying a different subject is refused rather
//! than handed over, and a brand new account gets a random password nobody is
//! ever shown — so the password form stays closed until the owner resets.
//! Every failure ends on the sign-in page with one whitelisted reason and no
//! session.
//!
//! No PKCE, deliberately: this is a confidential client redeeming the code
//! server-side (see [`crate::oauth`]).

use axum::extract::Query;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db;
use crate::db::sessions::{self, SessionUser};
use crate::oauth::{self, Identity};
use crate::routes::errors::{self, AppError};
use crate::session;

use super::{home_response, navigation_response, public_base, random_hex_256, set_session_cookie};

/// The callback's query string as Google sends it: the code and the state on
/// success, an `error` code when the visitor refused consent.
#[derive(Debug, Deserialize)]
pub struct GoogleCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// One of the three reasons a failed Google sign-in lands back on `/login`
/// with. Each maps to a translation key; anything else is the generic one, so
/// the message on the page is never attacker-chosen.
pub(super) fn google_reason(raw: Option<&str>) -> &'static str {
    match raw {
        Some("cancelled") => "google_sign_in_cancelled",
        Some("conflict") => "google_account_conflict",
        Some("failed") => "google_sign_in_failed",
        _ => "",
    }
}

/// What `account_for_google` decided: the account to open a session for, or
/// the reason a whitelisted message goes back to the sign-in page with.
enum GoogleOutcome {
    Account(db::User),
    Refused(&'static str),
}

/// Starts Google sign-in: a fresh state goes into the single-use store and
/// into a cookie, and the visitor is sent to Google's consent screen.
///
/// The cookie is what ties the callback to *this* browser. The store alone
/// cannot: a callback URL started on an attacker's machine and handed to a
/// victim would sign the victim in as the attacker, which is the whole attack
/// `state` exists to stop.
pub async fn google_start(ctx: Ctx) -> Response {
    let state = crate::state::state();
    if !state.cfg.google.is_configured() {
        // unconfigured answers exactly like nonexistent, as the admin routes
        // do; the login templates offer no button either
        return errors::not_found(ctx);
    }
    let redirect_uri = google_redirect_uri(&ctx, &state.cfg);
    let token = state.oauth_states.issue();
    let url = oauth::authorize_url(&state.cfg.google, &redirect_uri, &token);
    let mut res = external_redirect(&url);
    let cookie = oauth::STATE_COOKIE;
    let path = oauth::STATE_COOKIE_PATH;
    if let Ok(v) = HeaderValue::from_str(&format!(
        "{cookie}={token}; path={path}; HttpOnly; Secure; SameSite=Lax"
    )) {
        res.headers_mut().append(header::SET_COOKIE, v);
    }
    res
}

/// Google's answer. Nothing in the query string is trusted: the state has to
/// match the cookie this browser carries *and* be unspent and young, and the
/// code is forwarded only after both hold. Every failure ends on the sign-in
/// page with one whitelisted reason and no session.
pub async fn google_callback(
    ctx: Ctx,
    headers: HeaderMap,
    Query(q): Query<GoogleCallbackQuery>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    if !state.cfg.google.is_configured() {
        return Ok(errors::not_found(ctx));
    }
    if q.error.is_some() {
        // the visitor pressed cancel, or Google refused the request. The value
        // itself is not logged: it is request input, and its length is not
        // ours to bound.
        tracing::info!("google: consent was not granted");
        return Ok(google_failure(&ctx, "cancelled"));
    }
    let Some(query_state) = q.state.as_deref().filter(|s| !s.is_empty()) else {
        tracing::warn!("google: callback carried no state");
        return Ok(google_failure(&ctx, "failed"));
    };
    let cookie_ok =
        crate::ctx::cookie(&headers, oauth::STATE_COOKIE).is_some_and(|v| v == query_state);
    // consumed either way: a state that reached the wrong browser is spent
    // rather than left standing for a second attempt
    let store_ok = state.oauth_states.consume(query_state);
    if !cookie_ok || !store_ok {
        tracing::warn!(
            cookie_ok,
            store_ok,
            "google: the callback state did not verify"
        );
        return Ok(google_failure(&ctx, "failed"));
    }
    let Some(code) = oauth::usable_code(q.code.as_deref()) else {
        tracing::warn!("google: callback carried no usable code");
        return Ok(google_failure(&ctx, "failed"));
    };
    let redirect_uri = google_redirect_uri(&ctx, &state.cfg);
    let Some(access_token) = oauth::exchange_code(&state.cfg.google, &redirect_uri, code).await
    else {
        return Ok(google_failure(&ctx, "failed"));
    };
    let Some(identity) = oauth::userinfo(&access_token).await else {
        return Ok(google_failure(&ctx, "failed"));
    };
    let user = match account_for_google(&state.db, &identity).await? {
        GoogleOutcome::Account(user) => user,
        GoogleOutcome::Refused(reason) => return Ok(google_failure(&ctx, reason)),
    };
    tracing::info!(user_id = user.id, "google: signed in");
    let key = sessions::start(
        &state.db,
        &SessionUser {
            id: user.id.clone(),
            username: user.username.clone(),
            is_admin: user.is_admin,
            avatar: user.avatar.clone(),
        },
    )
    .await?;
    let mut res = home_response(&ctx);
    set_session_cookie(res.headers_mut(), &key);
    clear_state_cookie(res.headers_mut());
    Ok(res)
}

/// The account a Google identity signs in as: the one already linked to that
/// subject, otherwise the account holding the same *verified* address (linked
/// on the spot), otherwise a brand new one.
///
/// `Refused` covers one case: the verified address belongs to an account that
/// already carries a different Google subject. Something is wrong with the
/// link — a reassigned address, a shared mailbox, a hand-edited row — and
/// choosing either account would be a guess, so nothing is written and the
/// visitor gets the generic failure.
async fn account_for_google(db: &db::Db, identity: &Identity) -> Result<GoogleOutcome, AppError> {
    if let Some(existing) = db::find_user_by_google_sub(db, &identity.sub).await? {
        return Ok(GoogleOutcome::Account(existing));
    }
    if let Some(email) = identity.email.as_deref() {
        if let Some(existing) = db::find_user_by_email(db, email).await? {
            if existing
                .google_sub
                .as_deref()
                .is_some_and(|sub| sub != identity.sub)
            {
                tracing::warn!(
                    user_id = existing.id,
                    "google: the account already carries another Google subject"
                );
                return Ok(GoogleOutcome::Refused("conflict"));
            }
            db::link_user_google(db, &existing.id, &identity.sub).await?;
            tracing::info!(user_id = %existing.id, "google: linked an existing account");
            return Ok(GoogleOutcome::Account(existing));
        }
    }
    let base = oauth::derive_username(&identity.name, identity.email.as_deref());
    // A random password, hashed and never shown to anyone: the account's front
    // door is Google. The hash is what keeps the password form closed — no
    // guess verifies against it — and `/forgot-password` on the verified
    // address is how the owner sets one they know.
    let hash = session::hash_password(db, &random_hex_256()).await?;
    for attempt in 0..=oauth::MAX_USERNAME_ATTEMPTS {
        let candidate = oauth::username_candidate(&base, attempt);
        // asked before the insert rather than after: the create's own answer
        // rests on the username unique index, and the numbering must hold on a
        // database that has not got one
        if db::find_user(db, &candidate).await?.is_some() {
            continue;
        }
        let created = db::create_user(
            db,
            &candidate,
            &hash,
            identity.email.as_deref(),
            Some(&identity.sub),
        )
        .await?;
        if let Some(user) = created {
            tracing::info!(user_id = user.id, "google: created an account");
            return Ok(GoogleOutcome::Account(user));
        }
    }
    // every candidate was taken: a name contested by thousands, not a sign-in
    // this visitor can complete today
    tracing::warn!("google: every candidate username was taken");
    Ok(GoogleOutcome::Refused("failed"))
}

/// The way back from a refused callback: the sign-in page with one reason
/// from this module. The reason is never query input, and the destination is a
/// literal, so the redirect cannot be aimed anywhere else.
fn google_failure(ctx: &Ctx, reason: &str) -> Response {
    navigation_response(
        ctx,
        match reason {
            "cancelled" => "/login?google=cancelled",
            "conflict" => "/login?google=conflict",
            _ => "/login?google=failed",
        },
    )
}

/// The callback URL for this deployment. `[google] redirect_uri` wins, so a
/// deployment behind a proxy can name the exact URL registered with Google;
/// otherwise it is the public base plus the callback path. Google compares the
/// value byte for byte between the consent request and the token exchange, and
/// both halves of the flow build it here.
fn google_redirect_uri(ctx: &Ctx, cfg: &crate::config::Config) -> String {
    let configured = cfg.google.redirect_uri.trim_end_matches('/');
    if !configured.is_empty() {
        return configured.to_string();
    }
    format!("{}/auth/google/callback", public_base(ctx, cfg))
}

/// A 303 to an absolute URL this module built. Its parameters are
/// percent-encoded, so the value is printable ASCII and can be put in the
/// header as-is — a `Location` carrying request input could otherwise smuggle
/// a response header.
fn external_redirect(url: &str) -> Response {
    match HeaderValue::from_str(url) {
        Ok(value) => {
            let mut res = StatusCode::SEE_OTHER.into_response();
            res.headers_mut().insert(header::LOCATION, value);
            res
        }
        Err(e) => {
            tracing::error!("google: cannot build the consent redirect: {e}");
            errors::server_error(Ctx::minimal())
        }
    }
}

/// Drops the flow cookie once the callback has been answered — every path
/// through it, since the state is spent either way.
fn clear_state_cookie(headers: &mut HeaderMap) {
    let cookie = oauth::STATE_COOKIE;
    let path = oauth::STATE_COOKIE_PATH;
    if let Ok(v) = HeaderValue::from_str(&format!("{cookie}=; path={path}; Max-Age=0")) {
        headers.append(header::SET_COOKIE, v);
    }
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

    /// Removes a probe row, so a half-finished earlier run cannot make the
    /// create the test asserts on fail.
    async fn drop_probe_user(pool: &crate::db::Db, username: &str) {
        pool.query("DELETE user WHERE username = $u")
            .bind(("u", username.to_string()))
            .await
            .expect("cleanup");
    }

    /// Every numbered candidate a half-finished earlier run could have left
    /// behind, so a probe run starts from the same username it always does.
    async fn drop_probe_candidates(pool: &crate::db::Db, name: &str) {
        for attempt in 0..crate::oauth::MAX_USERNAME_ATTEMPTS {
            drop_probe_user(pool, &crate::oauth::username_candidate(name, attempt)).await;
        }
    }

    /// The profile row as the database holds it, for asserting what a sign-in
    /// wrote rather than what it returned.
    async fn stored_google_sub(pool: &crate::db::Db, username: &str) -> Option<String> {
        db::find_user(pool, username)
            .await
            .expect("find")
            .expect("row")
            .google_sub
    }

    /// The subject the account is keyed by. `None` would be a hard failure
    /// here, so the test panics rather than pattern-matching twice.
    fn expect_account(outcome: GoogleOutcome, what: &str) -> crate::db::User {
        match outcome {
            GoogleOutcome::Account(user) => user,
            GoogleOutcome::Refused(reason) => panic!("{what}: refused ({reason})"),
        }
    }

    /// The callback URL Google is told to return to. A configured
    /// `[google] redirect_uri` wins because Google compares it byte for byte —
    /// the deployment may well be reached through a proxy whose host differs
    /// from the one registered — and otherwise it comes off the same public
    /// base the reset links use, so a forged Host cannot aim the callback
    /// elsewhere.
    #[test]
    fn google_redirect_uri_prefers_configuration() {
        let ctx = crate::testutil::test_ctx("en");
        let mut cfg = crate::config::Config::default();
        assert_eq!(
            google_redirect_uri(&ctx, &cfg),
            "http://localhost:6785/auth/google/callback"
        );
        cfg.email.site_url = "https://cookierun.example".into();
        assert_eq!(
            google_redirect_uri(&ctx, &cfg),
            "https://cookierun.example/auth/google/callback"
        );
        cfg.google.redirect_uri = "https://wiki.example/oauth/google".into();
        assert_eq!(
            google_redirect_uri(&ctx, &cfg),
            "https://wiki.example/oauth/google",
            "the registered URL is echoed exactly, path and all"
        );
    }

    /// A `?google=` value a stranger chose is not a message: only the three
    /// reasons this module writes map to a translation key.
    #[test]
    fn google_reasons_are_whitelisted() {
        assert_eq!(google_reason(Some("failed")), "google_sign_in_failed");
        assert_eq!(google_reason(Some("cancelled")), "google_sign_in_cancelled");
        assert_eq!(google_reason(Some("conflict")), "google_account_conflict");
        assert_eq!(google_reason(None), "");
        assert_eq!(google_reason(Some("<script>alert(1)</script>")), "");
        assert_eq!(
            google_reason(Some("google_sign_in_failed")),
            "",
            "a key name is not a reason"
        );
    }

    /// A new Google subject creates an account: the derived username, the
    /// verified address, the subject, no admin — and a password hash nobody can
    /// guess, which is what keeps the password form closed for an account
    /// whose owner never set one. A second sign-in with the same subject finds
    /// that same account rather than making another.
    #[tokio::test]
    async fn google_sign_in_creates_an_account_with_an_unguessable_password() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let name = "CrGoogleProbe";
        drop_probe_candidates(&pool, name).await;

        // no subject is not a wildcard: an unlinked row has no field at all
        assert!(db::find_user_by_google_sub(&pool, "")
            .await
            .expect("empty subject")
            .is_none());

        let address = "cr_google_probe@example.com";
        let identity = Identity {
            sub: "probe-subject-1".into(),
            name: name.into(),
            email: Some(address.into()),
        };
        let created = expect_account(
            account_for_google(&pool, &identity).await.expect("create"),
            "create",
        );
        assert_eq!(created.username, name);
        assert_eq!(created.email.as_deref(), Some(address));
        assert_eq!(created.google_sub.as_deref(), Some("probe-subject-1"));
        assert!(!created.is_admin, "signing in with Google grants nothing");
        for guess in ["", "password", name, address] {
            assert!(
                !session::verify_password(&pool, guess, &created.password).await,
                "{guess:?} must not open the account"
            );
        }
        assert_eq!(
            db::find_user_by_google_sub(&pool, "probe-subject-1")
                .await
                .expect("lookup")
                .map(|u| u.id),
            Some(created.id.clone())
        );

        let again = expect_account(
            account_for_google(&pool, &identity).await.expect("again"),
            "second sign-in",
        );
        assert_eq!(again.id, created.id, "the subject is the key, not the name");

        drop_probe_user(&pool, &created.username).await;
    }

    /// Linking, and the one case that is refused: a verified address that
    /// matches an existing account adopts it (password intact), while an
    /// account already carrying a different subject is handed to nobody.
    #[tokio::test]
    async fn google_sign_in_links_or_refuses_an_existing_address() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let name = "CrGoogleProbeLink";
        drop_probe_candidates(&pool, name).await;

        let hash = session::hash_password(&pool, "probe password")
            .await
            .expect("hash");
        let address = "cr_google_link@example.com";
        let existing = db::create_user(&pool, name, &hash, Some(address), None)
            .await
            .expect("create")
            .expect("created");

        let adopting = Identity {
            sub: "probe-subject-2".into(),
            name: "ignored name".into(),
            email: Some(address.into()),
        };
        let adopted = expect_account(
            account_for_google(&pool, &adopting).await.expect("link"),
            "link",
        );
        assert_eq!(adopted.id, existing.id, "the existing account answers");
        assert_eq!(
            stored_google_sub(&pool, name).await.as_deref(),
            Some("probe-subject-2"),
            "the link reaches the row"
        );
        assert!(
            session::verify_password(&pool, "probe password", &adopted.password).await,
            "linking does not touch the password"
        );

        // a third subject on that same verified address is refused: the
        // account already belongs to somebody
        db::link_user_google(&pool, &existing.id, "someone-elses-subject")
            .await
            .expect("relink");
        let conflict = Identity {
            sub: "probe-subject-3".into(),
            name: "ignored name".into(),
            email: Some(address.into()),
        };
        match account_for_google(&pool, &conflict)
            .await
            .expect("conflict")
        {
            GoogleOutcome::Refused(reason) => assert_eq!(reason, "conflict"),
            GoogleOutcome::Account(user) => panic!("handed out account {}", user.username),
        }
        assert_eq!(
            stored_google_sub(&pool, name).await.as_deref(),
            Some("someone-elses-subject"),
            "a refused sign-in changes nothing"
        );

        drop_probe_user(&pool, name).await;
    }
}
