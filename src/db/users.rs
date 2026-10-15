//! Accounts and password-reset tokens. Every lookup binds its parameters;
//! the rows are schemaless like the catalog tables.

use surrealdb::types::SurrealValue;

use super::{Db, Result};

/// A user row, for the session layer.
#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password: String,
    pub is_admin: bool,
    pub email: Option<String>,
    /// Google's immutable subject id when this account signs in with Google.
    /// `None` for password accounts; `Some` also disables the password form's
    /// usefulness — see the account email page.
    pub google_sub: Option<String>,
}

/// The columns every user lookup reads, so the three finders cannot drift
/// apart and hand out a half-populated row.
const USER_COLUMNS: &str = "record::id(id) AS id, username, password, is_admin, email, google_sub";

fn user_from_row(r: UserRow) -> User {
    User {
        id: r.id,
        username: r.username,
        password: r.password,
        is_admin: r.is_admin,
        email: r.email,
        google_sub: r.google_sub,
    }
}

#[derive(Default, SurrealValue)]
#[surreal(default)]
struct UserRow {
    id: i64,
    username: String,
    password: String,
    is_admin: bool,
    email: Option<String>,
    google_sub: Option<String>,
}

pub async fn find_user(db: &Db, username: &str) -> Result<Option<User>> {
    let mut rows: Vec<UserRow> = db
        .query(format!(
            "SELECT {USER_COLUMNS} FROM user WHERE username = $u LIMIT 1"
        ))
        .bind(("u", username.to_string()))
        .await?
        .take(0)?;
    Ok(rows.pop().map(user_from_row))
}

/// Creates a user with an already-hashed password. `None` when the username
/// is taken, which the unique index enforces rather than a prior SELECT.
///
/// `email` is `None` when the visitor left the field blank: it is collected
/// for password reset only, so an account without one still signs in — the
/// row simply has no way back. The table is schemaless, so an `email` no
/// other query reads stays invisible to login, sessions and the admin UI.
pub async fn create_user(
    db: &Db,
    username: &str,
    password_hash: &str,
    email: Option<&str>,
    google_sub: Option<&str>,
) -> Result<Option<User>> {
    let mut res = db
        .query(
            "LET $id = sequence::nextval('user_seq'); \
             CREATE type::record('user', $id) SET username = $u, password = $p, email = $e, \
                 google_sub = $g, is_admin = false, created_at = time::unix(); \
             RETURN $id;",
        )
        .bind(("u", username.to_string()))
        .bind(("p", password_hash.to_string()))
        .bind(("e", email.map(str::to_string)))
        .bind(("g", google_sub.map(str::to_string)))
        .await?;
    // a duplicate username trips the unique index; the form says taken.
    // take_errors() leaves the Ok results in place, so the id reads after.
    if let Some(e) = res.take_errors().into_values().next() {
        if e.to_string().contains("already contains") {
            return Ok(None);
        }
        return Err(e);
    }
    let id = res
        .take::<Option<i64>>(2)?
        .ok_or_else(|| surrealdb::Error::internal("user insert returned no id".to_string()))?;
    Ok(Some(User {
        id,
        username: username.to_string(),
        password: password_hash.to_string(),
        is_admin: false,
        email: email.map(str::to_string),
        google_sub: google_sub.map(str::to_string),
    }))
}

/// The account behind an email address, matched case-insensitively: visitors
/// type `Ginger@Cookie.Run` while the row keeps whatever the register form was
/// given, and an address that differs only in case is the same mailbox.
///
/// `None` covers both "no account has that address" and "the row has no
/// email" — callers must not tell those apart, which is why nothing more
/// specific comes back.
///
/// The `?? ''` is load-bearing, not tidiness: an account registered without a
/// recovery address stores `NULL` there, and `string::lowercase(NULL)` is an
/// error rather than a non-match — one such row failed the whole query, which
/// took password reset, the taken-address check and Google's address match
/// down with it. Coalescing is the same shape the search clauses use.
pub async fn find_user_by_email(db: &Db, email: &str) -> Result<Option<User>> {
    // an empty lookup would otherwise match the coalesced rows above
    if email.trim().is_empty() {
        return Ok(None);
    }
    let mut rows: Vec<UserRow> = db
        .query(format!(
            "SELECT {USER_COLUMNS} FROM user \
               WHERE string::lowercase(email ?? '') = string::lowercase($e) LIMIT 1"
        ))
        .bind(("e", email.to_string()))
        .await?
        .take(0)?;
    Ok(rows.pop().map(user_from_row))
}

/// The account already linked to a Google subject id. The subject — not the
/// address — is Google's identity: an address can be renamed or reassigned,
/// the subject cannot.
///
/// An empty subject answers `None` without querying: rows that never linked
/// Google have no field at all, and matching their absence against an absent
/// subject would hand out whichever row the database returned first.
pub async fn find_user_by_google_sub(db: &Db, sub: &str) -> Result<Option<User>> {
    if sub.is_empty() {
        return Ok(None);
    }
    let mut rows: Vec<UserRow> = db
        .query(format!(
            "SELECT {USER_COLUMNS} FROM user WHERE google_sub = $s LIMIT 1"
        ))
        .bind(("s", sub.to_string()))
        .await?
        .take(0)?;
    Ok(rows.pop().map(user_from_row))
}

/// Records the Google link on an existing account, which is what turns a
/// password account whose address Google verified into one that signs in
/// either way. Written once, on the sign-in that discovered the match; the
/// password stays as it was, so both routes in keep working.
pub async fn link_user_google(db: &Db, user_id: i64, sub: &str) -> Result<()> {
    if sub.is_empty() {
        return Ok(());
    }
    db.query("UPDATE type::record('user', $u) SET google_sub = $s")
        .bind(("u", user_id))
        .bind(("s", sub.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Reads a user by its stable record id for post-reset security notifications.
pub async fn find_user_by_id(db: &Db, user_id: i64) -> Result<Option<User>> {
    let mut rows: Vec<UserRow> = db
        .query(format!(
            "SELECT {USER_COLUMNS} FROM type::record('user', $u) LIMIT 1"
        ))
        .bind(("u", user_id))
        .await?
        .take(0)?;
    Ok(rows.pop().map(user_from_row))
}

/// Replaces or clears the recovery address on one account. An empty value
/// removes the optional field; non-empty addresses are normalized by the
/// caller and checked against the existing case-insensitive lookup.
pub async fn set_user_email(db: &Db, user_id: i64, email: Option<&str>) -> Result<()> {
    db.query("UPDATE type::record('user', $u) SET email = $e")
        .bind(("u", user_id))
        .bind(("e", email.map(str::to_string)))
        .await?
        .check()?;
    Ok(())
}

/// Whether the account asked for a link within `window_secs`. The throttle
/// lives here rather than in a counter: the row it reads is the one the last
/// request wrote, so a restart cannot lose it and a second node cannot hold a
/// different idea of it.
pub async fn reset_requested_within(db: &Db, user_id: i64, window_secs: i64) -> Result<bool> {
    let rows: Vec<bool> = db
        .query(
            "SELECT VALUE created_at > time::unix() - $w FROM password_reset \
              WHERE user = type::record('user', $u) LIMIT 1",
        )
        .bind(("u", user_id))
        .bind(("w", window_secs))
        .await?
        .take(0)?;
    Ok(rows.into_iter().next().unwrap_or(false))
}

/// Stores a fresh token and drops any outstanding one: one live link per
/// account, so asking again invalidates the earlier mail rather than leaving
/// two working links in an inbox.
///
/// The plaintext token is bound as a parameter and hashed by `SurrealDB`
/// (`crypto::sha256`), so only the digest is ever written down — a leaked row
/// cannot be turned back into a working link.
pub async fn issue_reset_token(db: &Db, user_id: i64, token: &str, ttl_secs: i64) -> Result<()> {
    db.query(
        "DELETE password_reset WHERE user = type::record('user', $u); \
         CREATE password_reset SET user = type::record('user', $u), \
             token_hash = crypto::sha256($t), \
             created_at = time::unix(), \
             expires = time::unix() + $ttl;",
    )
    .bind(("u", user_id))
    .bind(("t", token.to_string()))
    .bind(("ttl", ttl_secs))
    .await?
    .check()?;
    Ok(())
}

/// Whether a token would still be accepted. This is the GET half of the reset
/// page, which has to say "this link is dead" before the visitor types a
/// password — it consumes nothing.
pub async fn reset_token_pending(db: &Db, token: &str) -> Result<bool> {
    let rows: Vec<String> = db
        .query(
            "SELECT VALUE token_hash FROM password_reset \
              WHERE token_hash = crypto::sha256($t) AND expires > time::unix() LIMIT 1",
        )
        .bind(("t", token.to_string()))
        .await?
        .take(0)?;
    Ok(!rows.is_empty())
}

/// Consumes one token and answers with the account it belonged to.
///
/// The DELETE is what makes it single use: the row is removed by the same
/// statement that matches it, so a replayed link finds nothing and two racing
/// attempts cannot both win. An expired token matches nothing and the empty
/// array maps to an empty list rather than an error — `record::id` refuses
/// `NONE`, which is why the id is projected inside the map.
pub async fn consume_reset_token(db: &Db, token: &str) -> Result<Option<i64>> {
    let mut res = db
        .query(
            "LET $rows = DELETE password_reset \
                 WHERE token_hash = crypto::sha256($t) AND expires > time::unix() RETURN BEFORE; \
             RETURN $rows.map(|$r| record::id($r.user));",
        )
        .bind(("t", token.to_string()))
        .await?;
    let ids = res.take::<Vec<i64>>(1)?;
    Ok(ids.into_iter().next())
}

/// Overwrites a user's password hash. The caller revokes that user's sessions
/// itself: a reset is the moment to assume the old password leaked, so every
/// device signed in with it has to be signed out.
pub async fn set_user_password(db: &Db, user_id: i64, password_hash: &str) -> Result<()> {
    db.query("UPDATE type::record('user', $u) SET password = $p")
        .bind(("u", user_id))
        .bind(("p", password_hash.to_string()))
        .await?
        .check()?;
    Ok(())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use surrealdb::types::{Object, Value};

    #[test]
    fn user_row_reads_credentials_and_flag() {
        let mut o = Object::new();
        o.insert("id", 1i64);
        o.insert("username", "admin");
        o.insert("password", "$argon2id$v=19$fake");
        o.insert("is_admin", true);
        let row = UserRow::from_value(Value::Object(o)).expect("decodes");
        assert!(row.is_admin);
        assert_eq!(row.username, "admin");
    }
}
