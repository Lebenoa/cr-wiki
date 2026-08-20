use std::sync::Arc;

use crate::config::Config;
use crate::db::Db;
use crate::ratelimit::Limiter;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// read by the Turnstile check and the admin routes (see PORTING.md)
    #[allow(dead_code)]
    pub cfg: Arc<Config>,
    pub limiter: Arc<Limiter>,
}
