//! `Config.toml`: host/port, external SurrealDB, Turnstile, rate limiting,
//! and outgoing email API or authenticated SMTP settings.

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

/// Outgoing app email through a JSON API or authenticated SMTP submission.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Email {
    /// Transport: `api` by default, or `smtp`.
    pub transport: String,
    /// API bearer key, used only by the `api` transport.
    pub api_key: String,
    /// The sender mailbox, e.g. `Cookie Run Wiki <no-reply@example.com>`.
    pub from: String,
    /// JSON API endpoint, used only by the `api` transport.
    pub endpoint: String,
    /// SMTP relay host, e.g. `mail.lebenoa.com`.
    pub smtp_host: String,
    /// Submission port, normally 587 (STARTTLS) or 465 (implicit TLS).
    pub smtp_port: u16,
    /// `starttls` for port 587 or `tls` for port 465. Plaintext is unsupported.
    pub smtp_tls: String,
    /// SMTP AUTH username.
    pub smtp_username: String,
    /// SMTP AUTH password.
    pub smtp_password: String,
    /// Absolute public site URL used to build reset links.
    pub site_url: String,
}

impl Default for Email {
    fn default() -> Self {
        Self {
            transport: "api".into(),
            api_key: String::new(),
            from: String::new(),
            endpoint: "https://api.resend.com/emails".into(),
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_tls: "starttls".into(),
            smtp_username: String::new(),
            smtp_password: String::new(),
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
        if let Ok(v) = std::env::var("EMAIL_TRANSPORT") {
            if !v.is_empty() {
                cfg.email.transport = v;
            }
        }
        if let Ok(v) = std::env::var("EMAIL_API_KEY") {
            cfg.email.api_key = v;
        }
        if let Ok(v) = std::env::var("EMAIL_SMTP_HOST") {
            if !v.is_empty() {
                cfg.email.smtp_host = v;
            }
        }
        if let Ok(v) = std::env::var("EMAIL_SMTP_PORT") {
            if let Ok(port) = v.parse() {
                cfg.email.smtp_port = port;
            }
        }
        if let Ok(v) = std::env::var("EMAIL_SMTP_TLS") {
            if !v.is_empty() {
                cfg.email.smtp_tls = v;
            }
        }
        if let Ok(v) = std::env::var("EMAIL_SMTP_USERNAME") {
            cfg.email.smtp_username = v;
        }
        if let Ok(v) = std::env::var("EMAIL_SMTP_PASSWORD") {
            cfg.email.smtp_password = v;
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
