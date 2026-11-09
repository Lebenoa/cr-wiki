//! Server-side sessions, persisted in `SurrealDB`. Restarting the app no longer
//! signs everyone out: the cookie's uuid v4 is the record id (`session:<id>`),
//! created by the OS CSPRNG exactly as the in-memory map did, so the token
//! never crosses a query boundary as attacker-shaped input — it is bound, and
//! a record id that does not exist simply answers no row.
//!
//! The table is schemaless: a session row is `{ user_id, username, is_admin,
//! issued }`, denormalized off the account so the hot per-request read is one
//! plain fetch, not a join. The account id is stored as a string, not a record
//! link — a session needs no referential integrity (the account lookup happens
//! at login), and a plain column keeps every query a table select. `is_admin`
//! is snapshotted at login — revoking a user's admin status takes effect at
//! their next sign-in, the same trade the in-memory store made.
//!
//! TTL: a session dies 7 days after issue even if the cookie survives. The
//! per-request read enforces it (a stale token authenticates nobody and is
//! deleted on sight), and each login sweeps expired rows opportunistically,
//! so an idle table cannot grow without bound. `last_seen` is deliberately
//! not tracked: a write per request buys nothing the 7-day issue cap needs.

use surrealdb::types::SurrealValue;

use super::{Db, Result};

/// A session dies after this long even if its cookie survives: without an
/// issue time every login lived until process restart. Matches the 7-day cap
/// the V app carried and the in-memory store kept.
pub const SESSION_TTL_SECS: i64 = 7 * 24 * 3600;

/// The signed-in identity a session hands to the request context.
#[derive(Debug, Clone)]
pub struct SessionUser {
    /// the random user record id string, not a sequence number
    pub id: String,
    pub username: String,
    pub is_admin: bool,
}

/// Creates the session table and its user lookup index. Called on connect,
/// next to the other schema guarantees.
pub async fn ensure_schema(db: &Db) -> Result<()> {
    db.query(
        "DEFINE TABLE IF NOT EXISTS session SCHEMALESS; \
         DEFINE INDEX IF NOT EXISTS session_user ON session FIELDS user_id;",
    )
    .await?
    .check()?;
    Ok(())
}

/// Starts a session for `user` and returns its key — the new record id, which
/// becomes the cookie value. Sweeps expired rows on the way in: the same
/// on-write sweep the in-memory map did, so the table cannot grow without
/// bound while logins keep arriving.
pub async fn start(db: &Db, user: &SessionUser) -> Result<String> {
    let key = uuid::Uuid::new_v4().to_string();
    db.query(
        "CREATE type::record('session', $k) SET \
                 user_id = $u, username = $n, is_admin = $a, issued = time::unix(); \
             DELETE session WHERE issued < time::unix() - $ttl; \
             RETURN NONE;",
    )
    .bind(("k", key.clone()))
    .bind(("u", user.id.clone()))
    .bind(("n", user.username.clone()))
    .bind(("a", user.is_admin))
    .bind(("ttl", SESSION_TTL_SECS))
    .await?
    .check()?;
    Ok(key)
}

/// Resolves a session key to its signed-in user. A stale session is deleted
/// and answers `None` — a token past its TTL must not authenticate anyone,
/// and deleting it stops the replay the sweep would eventually have caught.
/// The TTL comparison happens in Rust so the rule lives once beside the
/// constant rather than in every query string.
///
/// A transport error also answers `None`: the request renders signed-out
/// rather than failing, and the next request retries. This is the one store
/// read on the hot path, so it must never 500 a page.
#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct Row {
    user_id: Option<String>,
    username: Option<String>,
    is_admin: Option<bool>,
    #[surreal(default)]
    issued: Option<i64>,
}

pub async fn get(db: &Db, key: &str) -> Option<SessionUser> {
    if key.is_empty() {
        return None;
    }
    let mut res = db
        .query(
            "SELECT user_id, username, is_admin, issued \
                 FROM session WHERE id = type::record('session', $k);",
        )
        .bind(("k", key.to_string()))
        .await
        .ok()?;
    let rows: Vec<Row> = res.take(0).ok()?;
    let row = rows.into_iter().next()?;
    // a stale token must not authenticate anyone, and deleting it stops the
    // replay the sweep would eventually have caught
    let issued = row.issued.unwrap_or(0);
    if crate::time::now_unix().saturating_sub(issued) > SESSION_TTL_SECS {
        end(db, key).await;
        return None;
    }
    Some(SessionUser {
        id: row.user_id?,
        username: row.username?,
        is_admin: row.is_admin.unwrap_or(false),
    })
}

/// Ends one session (logout). A table-scoped DELETE with an id equality
/// filter keeps the target a bound parameter rather than a literal record
/// id interpolated into the string.
pub async fn end(db: &Db, key: &str) {
    if key.is_empty() {
        return;
    }
    let _ = db
        .query("DELETE session WHERE id = type::record('session', $k);")
        .bind(("k", key.to_string()))
        .await;
}

/// Revokes every session belonging to one user id. Called by the account
/// menu's revoke-all action and by password resets, not by logout: logout
/// ends only the current session, so multi-device sign-in survives until the
/// owner explicitly revokes everywhere.
pub async fn end_all_for(db: &Db, user_id: &str) -> Result<()> {
    if user_id.is_empty() {
        return Ok(());
    }
    db.query("DELETE session WHERE user_id = $u;")
        .bind(("u", user_id.to_string()))
        .await?
        .check()?;
    Ok(())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// A session round trip: start, read back, end — persisted, so it also
    /// survives a second connection (a restart's shape).
    #[tokio::test]
    async fn sessions_round_trip_and_persist() {
        let Some(url) = std::env::var("CR_SURREAL_URL").ok() else {
            eprintln!("skip: set CR_SURREAL_URL to run the session tests");
            return;
        };
        let user_env = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass_env = std::env::var("SURREAL_PASS").unwrap_or_default();
        let db = crate::db::connect_url(
            &url,
            "cookierun_session_test",
            "cookierun_session_test",
            &user_env,
            &pass_env,
        )
        .await
        .expect("scratch db");
        ensure_schema(&db).await.unwrap();
        let user = SessionUser {
            id: "abc123".into(),
            username: "tester".into(),
            is_admin: true,
        };
        let key = start(&db, &user).await.expect("start");
        let found = get(&db, &key).await.expect("session");
        assert_eq!(found.username, "tester");
        assert!(found.is_admin);
        assert_eq!(found.id, "abc123");

        // persistence is the point: a fresh handle to the same server sees it
        let db2 = crate::db::connect_url(
            &url,
            "cookierun_session_test",
            "cookierun_session_test",
            &user_env,
            &pass_env,
        )
        .await
        .expect("same server");
        let found2 = get(&db2, &key)
            .await
            .expect("session survives a new connection");
        assert_eq!(found2.username, "tester");

        // an unknown key answers None
        assert!(get(&db, "never-issued-uuid").await.is_none());
        // an empty key answers None without querying
        assert!(get(&db, "").await.is_none());

        // revoke-all for another user leaves this session; for this user ends it
        end_all_for(&db, "other-user").await.unwrap();
        assert!(
            get(&db, &key).await.is_some(),
            "other user's revoke must not leak"
        );
        end_all_for(&db, "abc123").await.unwrap();
        assert!(get(&db, &key).await.is_none());
        assert!(
            get(&db2, &key).await.is_none(),
            "revocation must be visible everywhere"
        );
    }
}
