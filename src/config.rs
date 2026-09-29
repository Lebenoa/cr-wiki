//! `Config.toml`: host/port, the `SurrealDB` server this app talks to (never an
//! embedded engine — always `ws://` or `http://` over the wire), Turnstile
//! credentials, and the rate-limit tuning.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub surreal: SurrealConfig,
    pub turnstile: Turnstile,
    pub ratelimit: RateLimit,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6785,
            surreal: SurrealConfig::default(),
            turnstile: Turnstile::default(),
            ratelimit: RateLimit::default(),
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
