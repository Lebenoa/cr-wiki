//! Mirrors config/config.v: the same Config.toml, the same defaults, and the
//! same clamping of non-positive rate-limit values back to their defaults.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub db_file: String,
    pub turnstile: Turnstile,
    pub ratelimit: RateLimit,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6785,
            db_file: "sqlite.db".into(),
            turnstile: Turnstile::default(),
            ratelimit: RateLimit::default(),
        }
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
}

impl Default for RateLimit {
    fn default() -> Self {
        Self { capacity: 60.0, refill: 20.0, idle_ttl: 300, sweep_above: 2048 }
    }
}

impl Config {
    /// Loads Config.toml, falling back to the defaults when it is missing or
    /// unparseable — the V version prints and carries on the same way.
    pub fn load(path: &str) -> Self {
        let mut cfg: Config = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();

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
