use std::sync::OnceLock;

use crate::config::Config;
use crate::db::Db;
use crate::oauth::States;
use crate::ratelimit::Limiter;
use crate::session::Sessions;

/// The shared server state, owned by [`STATE`]. Handlers and middleware read
/// it through [`state()`] instead of an axum `State` extractor, so serving a
/// request never clones the connection handle (a `Surreal` clone registers a
/// fresh server-side session and replays signin+use per request, ~120 ms
/// measured — the once-we-rebought perf regression this shape prevents).
///
/// No `Arc` wrappers: [`STATE`] is the single owner and nothing clones it;
/// `Sessions` and `Limiter` keep their own interior mutexes.
pub struct AppState {
    pub db: Db,
    /// read by the Turnstile check and the rate limiter's proxy whitelist
    pub cfg: Config,
    pub limiter: Limiter,
    pub sessions: Sessions,
    /// the Google sign-in flows waiting on a callback, in memory like the
    /// sessions: a restart drops the flows in flight and the visitor retries
    pub oauth_states: States,
}

static STATE: OnceLock<AppState> = OnceLock::new();

/// Installs the one shared instance. Called exactly once in `main` after the
/// DB connection is established (and by the route tests with an inert
/// handle). A second call is ignored — the first instance wins, which is what
/// parallel route tests rely on.
pub fn init(state: AppState) {
    let _ = STATE.set(state);
}

/// The shared instance for handlers and middleware.
///
/// `init` runs before the listener is bound, so a serving process always has
/// one; the expect is an unreachable startup-order assertion, not a recovery
/// path.
#[allow(clippy::expect_used)] // sole exception: process-global set in main
pub fn state() -> &'static AppState {
    STATE.get().expect("state initialized before serving")
}

/// Whether Google sign-in is configured, for the templates that decide whether
/// to offer it. Deliberately not [`state`]: a test that renders a page without
/// installing the shared state must get "no button" rather than a panic, and
/// the answer is a property of the config, not of the request.
pub fn google_login_enabled() -> bool {
    STATE.get().is_some_and(|s| s.cfg.google.is_configured())
}

/// The site's own timezone, in minutes east of UTC, for the surfaces that turn
/// a stored instant into the civil day a reader or an author sees. Same rule as
/// [`google_login_enabled`]: a test rendering a page without the shared state
/// gets UTC rather than a panic. `main` installs the state before the listener
/// binds, so a serving process always answers from config.
pub fn site_tz_offset() -> i64 {
    STATE.get().map_or(0, |s| s.cfg.site.utc_offset_minutes)
}