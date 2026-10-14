//! `Config.toml`: host/port, the `SurrealDB` server this app talks to (never an
//! embedded engine — always `ws://` or `http://` over the wire), Turnstile
//! credentials, the rate-limit tuning, and the sender the password-reset mail
//! goes out through.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub surreal: SurrealConfig,
    pub turnstile: Turnstile,
    pub ratelimit: RateLimit,
    pub email: Email,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6785,
            surreal: SurrealConfig::default(),
            turnstile: Turnstile::default(),
            ratelimit: RateLimit::default(),
            email: Email::default(),
        }
    }
}

/// Where the data lives: an external `SurrealDB` server. The app has no local
/// storage of its own.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SurrealConfig {
    /// `ws://host:8000` or `http://host:8000`
    pub url: String,
    pub namespace: String,
    pub database: String,
    pub username: String,
    pub password: String,
}

impl SurrealConfig {
    pub const fn is_configured(&self) -> bool {
        !self.url.is_empty() && !self.namespace.is_empty() && !self.database.is_empty()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Turnstile {
    pub secret: String,
    pub hostnames: String,
}

/// Outgoing mail. The password-reset link is the only message the app sends,
/// and it goes out as a JSON POST to `endpoint` — Resend's
/// `POST https://api.resend.com/emails` by default — with the message text
/// rendered by the app's own templates, so no copy lives in a provider
/// dashboard.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Email {
    /// Provider API key (`re_...` for Resend), sent as `Authorization: Bearer`.
    /// Empty sends nothing at all: a debug build logs the reset link so the
    /// flow stays walkable without an account, and a release build logs an
    /// error. The visitor sees the same neutral page either way, so a
    /// misconfigured deployment cannot leak which addresses exist.
    pub api_key: String,
    /// The `From` mailbox: `Cookie Run Wiki <no-reply@example.com>`. Resend
    /// rejects a sender the account has not verified.
    pub from: String,
    /// Where sends are posted. Only a test or a different provider changes it.
    pub endpoint: String,
    /// Absolute site URL the reset link is built from, e.g.
    /// `https://cookierun.example`. Empty falls back to the request's own
    /// host, and a forged `Host` header would then be mailed to the visitor as
    /// the place to type a new password — set it in production.
    pub site_url: String,
}

impl Default for Email {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            from: String::new(),
            endpoint: "https://api.resend.com/emails".into(),
            site_url: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RateLimit {
    pub capacity: f64,
    pub refill: f64,
    pub idle_ttl: i64,
    pub sweep_above: i64,
    /// Proxy IPs whose forwarded client headers may stand in for the visitor
    /// address when keying buckets; every other peer is keyed by its own TCP
    /// address, because forwarded headers from anyone else are spoofable.
    pub trusted_proxies: Vec<String>,
}

impl Default for RateLimit {
    fn default() -> Self {
        Self {
            capacity: 60.0,
            refill: 20.0,
            idle_ttl: 300,
            sweep_above: 2048,
            trusted_proxies: Vec::new(),
        }
    }
}

impl Config {
    /// Loads Config.toml, falling back to the defaults when it is missing or
    /// unparseable — the V version prints and carries on the same way. The
    /// fallback is loud: a half-read config silently disables rate limiting
    /// and Turnstile, so the reason must reach the log.
    pub fn load(path: &str) -> Self {
        let mut cfg: Self = std::fs::read_to_string(path).map_or_else(
            |_| {
                tracing::warn!("config: {path} not found; running on defaults");
                Self::default()
            },
            |s| match toml::from_str(&s) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("config: {path} does not parse ({e}); running on defaults");
                    Self::default()
                }
            },
        );

        // CR_HOST / CR_PORT override the bind address without editing the
        // shared file.
        if let Ok(v) = std::env::var("CR_HOST") {
            if !v.is_empty() {
                cfg.host = v;
            }
        }
        if let Ok(v) = std::env::var("CR_PORT") {
            if let Ok(port) = v.parse::<u16>() {
                cfg.port = port;
            }
        }

        // env overrides for the database server
        if let Ok(v) = std::env::var("SURREAL_URL") {
            if !v.is_empty() {
                cfg.surreal.url = v;
            }
        }
        if let Ok(v) = std::env::var("SURREAL_NS") {
            if !v.is_empty() {
                cfg.surreal.namespace = v;
            }
        }
        if let Ok(v) = std::env::var("SURREAL_DB") {
            if !v.is_empty() {
                cfg.surreal.database = v;
            }
        }
        if let Ok(v) = std::env::var("SURREAL_USER") {
            if !v.is_empty() {
                cfg.surreal.username = v;
            }
        }
        if let Ok(v) = std::env::var("SURREAL_PASS") {
            if !v.is_empty() {
                cfg.surreal.password = v;
            }
        }

        // env overrides, as in config.v
        if let Ok(v) = std::env::var("TURNSTILE_SECRET") {
            cfg.turnstile.secret = v;
        }
        if let Ok(v) = std::env::var("TURNSTILE_HOSTNAMES") {
            cfg.turnstile.hostnames = v;
        }

        // the reset mail's sender. A key set here rather than in a file keeps
        // the secret out of the deployment bundle; the endpoint override is
        // what lets a test point the sender at a stub.
        if let Ok(v) = std::env::var("EMAIL_API_KEY") {
            cfg.email.api_key = v;
        }
        if let Ok(v) = std::env::var("EMAIL_FROM") {
            cfg.email.from = v;
        }
        if let Ok(v) = std::env::var("EMAIL_ENDPOINT") {
            if !v.is_empty() {
                cfg.email.endpoint = v;
            }
        }
        if let Ok(v) = std::env::var("EMAIL_SITE_URL") {
            cfg.email.site_url = v;
        }

        let d = RateLimit::default();
        if cfg.ratelimit.capacity <= 0.0 {
            cfg.ratelimit.capacity = d.capacity;
        }
        if cfg.ratelimit.refill <= 0.0 {
            cfg.ratelimit.refill = d.refill;
        }
        if cfg.ratelimit.idle_ttl <= 0 {
            cfg.ratelimit.idle_ttl = d.idle_ttl;
        }
        if cfg.ratelimit.sweep_above <= 0 {
            cfg.ratelimit.sweep_above = d.sweep_above;
        }
        cfg
    }
}
