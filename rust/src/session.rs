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

#[derive(Debug, Default)]
pub struct Sessions {
    inner: Mutex<HashMap<String, SessionUser>>,
}

impl Sessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a session and returns its key, which becomes the cookie value.
    pub fn start(&self, user: SessionUser) -> String {
        let key = uuid::Uuid::new_v4().to_string();
        if let Ok(mut map) = self.inner.lock() {
            map.insert(key.clone(), user);
        }
        key
    }

    pub fn get(&self, key: &str) -> Option<SessionUser> {
        self.inner.lock().ok()?.get(key).cloned()
    }

    pub fn end(&self, key: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(key);
        }
    }
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
