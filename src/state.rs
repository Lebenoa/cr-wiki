use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;
use crate::ratelimit::Limiter;
use crate::session::Sessions;

#[derive(Clone)]
pub struct AppState {
    /// Shared across request clones so every handler uses the one authenticated
    /// ws session. A bare `Surreal` clone registers a fresh session server-side
    /// and replays signin+use per request (~120 ms measured); the Arc keeps the
    /// pool warm after the first request.
    pub db: Arc<Db>,
    /// read by the Turnstile check and the admin routes (see PORTING.md)
    #[allow(dead_code)]
    pub cfg: Arc<Config>,
    pub limiter: Arc<Limiter>,
    pub sessions: Arc<Sessions>,
}
