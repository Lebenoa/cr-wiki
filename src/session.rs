//! Logins, held in memory and keyed by the CRSESSID cookie — the same shape
//! as the V app's `sessions` map. Restarting drops every session, which is
//! what the original does too.

use std::collections::HashMap;
use std::sync::Mutex;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

pub const SESSION_COOKIE: &str = "CRSESSID";

#[derive(Debug, Clone)]
pub struct SessionUser {
    /// stamped onto a build once the planner lands (see PORTING.md)
    #[allow(dead_code)]
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Debug)]
pub struct Sessions {
    inner: Mutex<HashMap<String, Entry>>,
}

/// A session dies after this long even if its cookie survives: without an
/// issue time every login lived until process restart and the map grew
/// monotonically. Matches the 7-day cap the V app carried.
const SESSION_TTL_SECS: i64 = 7 * 24 * 3600;

#[derive(Debug)]
struct Entry {
    user: SessionUser,
    issued: i64,
}

impl Sessions {
    pub fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }

    /// Starts a session and returns its key, which becomes the cookie value.
    /// Expired entries are swept on the way in, so the map cannot grow
    /// without bound.
    pub fn start(&self, user: SessionUser) -> String {
        let key = uuid::Uuid::new_v4().to_string();
        let now = now_unix();
        if let Ok(mut map) = self.inner.lock() {
            map.retain(|_, e| now - e.issued <= SESSION_TTL_SECS);
            map.insert(key.clone(), Entry { user, issued: now });
        }
        key
    }

    pub fn get(&self, key: &str) -> Option<SessionUser> {
        let mut map = self.inner.lock().ok()?;
        let expired = match map.get(key) {
            Some(e) => now_unix() - e.issued > SESSION_TTL_SECS,
            None => return None,
        };
        if expired {
            // a stale token must not authenticate anyone; drop it so it also
            // cannot be replayed after the sweep would have caught it
            map.remove(key);
            return None;
        }
        map.get(key).map(|e| e.user.clone())
    }

    pub fn end(&self, key: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(key);
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Argon2id in PHC string format, which is what V's crypto.argon2 writes, so
/// a hash made by either app verifies in the other.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    // the salt comes from a v4 UUID's 16 random bytes rather than pulling in
    // a second RNG: both end up on the platform CSPRNG through getrandom
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes())?;
    Ok(Argon2::default().hash_password(password.as_bytes(), &salt)?.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// Burns roughly the time a real verification costs, so a missing username
/// cannot be told from a wrong password by how fast the answer comes back.
/// The V login does the same with a throwaway generate-then-compare.
pub fn waste_time(password: &str) {
    if let Ok(h) = hash_password(password) {
        let _ = verify_password(password, &h);
    }
}
