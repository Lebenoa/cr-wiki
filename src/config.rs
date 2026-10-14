//! `Config.toml`: host/port, external SurrealDB, Turnstile, rate limiting,
//! and authenticated SMTP email settings.

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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Turnstile {
    pub secret: String,
    pub hostnames: Vec<String>,
}

impl Default for Turnstile {
    fn default() -> Self {
        Self {
            secret: String::new(),
            hostnames: vec!["localhost".into(), "127.0.0.1".into()],
        }
    }
}

/// SMTP encryption modes accepted by the mail client.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmtpTls {
    Starttls,
    Tls,
}

/// Outgoing app email through authenticated SMTP submission.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Email {
    /// The sender mailbox, e.g. `Cookie Run Wiki <no-reply@example.com>`.
    pub from: String,
    /// SMTP relay host, e.g. `mail.lebenoa.com`.
    pub smtp_host: String,
    /// Submission port, normally 587 (STARTTLS) or 465 (implicit TLS).
    pub smtp_port: u16,
    /// SMTP encryption mode: STARTTLS or implicit TLS.
    pub smtp_tls: SmtpTls,
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
            from: String::new(),
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_tls: SmtpTls::Starttls,
            smtp_username: String::new(),
            smtp_password: String::new(),
            site_url: String::new(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.host.trim().is_empty() {
            errors.push("host must not be empty".to_string());
        }
        if self.port == 0 {
            errors.push("port must be between 1 and 65535".to_string());
        }
        let surreal_url = self.surreal.url.trim();
        if !(surreal_url.starts_with("ws://")
            || surreal_url.starts_with("wss://")
            || surreal_url.starts_with("http://")
            || surreal_url.starts_with("https://"))
        {
            errors.push("[surreal] url must use ws://, wss://, http://, or https://".to_string());
        }
        if self.surreal.namespace.trim().is_empty() || self.surreal.database.trim().is_empty() {
            errors.push("[surreal] namespace and database are required".to_string());
        }
        if self
            .turnstile
            .hostnames
            .iter()
            .any(|hostname| hostname.trim() != *hostname || hostname.is_empty())
        {
            errors.push("[turnstile] hostnames must be non-empty trimmed hostnames".to_string());
        }
        if !self.ratelimit.capacity.is_finite() || self.ratelimit.capacity <= 0.0 {
            errors.push("[ratelimit] capacity must be finite and greater than zero".to_string());
        }
        if !self.ratelimit.refill.is_finite() || self.ratelimit.refill <= 0.0 {
            errors.push("[ratelimit] refill must be finite and greater than zero".to_string());
        }
        if self.ratelimit.idle_ttl <= 0 || self.ratelimit.sweep_above <= 0 {
            errors
                .push("[ratelimit] idle_ttl and sweep_above must be greater than zero".to_string());
        }
        let email = &self.email;
        let smtp_fields_set = !email.smtp_host.is_empty()
            || !email.smtp_username.is_empty()
            || !email.smtp_password.is_empty()
            || !email.from.is_empty();
        if smtp_fields_set {
            if email.smtp_host.is_empty() {
                errors.push("[email] smtp_host is required when configuring SMTP".to_string());
            }
            if email.smtp_port == 0 {
                errors.push("[email] smtp_port must be between 1 and 65535".to_string());
            }
            if email.smtp_username.is_empty() || email.smtp_password.is_empty() {
                errors.push(
                    "[email] smtp_username and smtp_password are required when configuring SMTP"
                        .to_string(),
                );
            }
            if email.from.parse::<lettre::message::Mailbox>().is_err() {
                errors.push("[email] from must be a valid SMTP mailbox".to_string());
            }
        }
        if !email.site_url.is_empty()
            && email.site_url.parse::<reqwest::Url>().map_or(true, |url| {
                !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
            })
        {
            errors.push("[email] site_url must be an absolute http:// or https:// URL".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, SmtpTls, Turnstile};

    #[test]
    fn typed_config_values_deserialize() {
        let config: Config = toml::from_str(
            "[turnstile]\nhostnames = [\"wiki.example\"]\n[email]\nsmtp_tls = \"tls\"",
        )
        .unwrap();
        assert_eq!(config.turnstile.hostnames, ["wiki.example"]);
        assert_eq!(config.email.smtp_tls, SmtpTls::Tls);
        assert_eq!(Turnstile::default().hostnames, ["localhost", "127.0.0.1"]);
    }

    #[test]
    fn validation_reports_missing_db_and_partial_smtp_fields() {
        let mut config = Config::default();
        config.email.from = "invalid".into();
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|error| error.contains("[surreal]")));
        assert!(errors.iter().any(|error| error.contains("smtp_host")));
        assert!(errors.iter().any(|error| error.contains("SMTP mailbox")));
    }

    #[test]
    fn validation_accepts_complete_valid_config() {
        let mut config = Config::default();
        config.surreal.url = "http://localhost:8000".into();
        config.surreal.namespace = "wiki".into();
        config.surreal.database = "wiki".into();
        config.validate().unwrap();
    }

    #[test]
    fn validation_accepts_complete_smtp_configuration() {
        let mut config = Config::default();
        config.surreal.url = "http://localhost:8000".into();
        config.surreal.namespace = "wiki".into();
        config.surreal.database = "wiki".into();
        config.email.from = "Wiki <no-reply@example.com>".into();
        config.email.smtp_host = "mail.example.com".into();
        config.email.smtp_username = "user".into();
        config.email.smtp_password = "secret".into();
        config.email.site_url = "https://wiki.example.com".into();
        config.validate().unwrap();
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
    /// Loads config and environment overrides, then validates all fields.
    /// Parse and validation failures prevent startup instead of defaulting
    /// silently.
    pub fn load(path: &str) -> Result<Self, Vec<String>> {
        let mut cfg: Self = match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).map_err(|e| vec![format!("{path}: {e}")])?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(vec![format!("cannot read {path}: {e}")]),
        };

        // CR_HOST / CR_PORT override the bind address without editing the
        // shared file.
        if let Ok(v) = std::env::var("CR_HOST") {
            if !v.is_empty() {
                cfg.host = v;
            }
        }
        if let Ok(v) = std::env::var("CR_PORT") {
            cfg.port = v
                .parse::<u16>()
                .map_err(|_| vec!["CR_PORT must be a valid port number".to_string()])?;
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
            cfg.turnstile.hostnames = v
                .split(',')
                .map(str::trim)
                .filter(|hostname| !hostname.is_empty())
                .map(str::to_string)
                .collect();
        }

        if let Ok(v) = std::env::var("EMAIL_SMTP_HOST") {
            if !v.is_empty() {
                cfg.email.smtp_host = v;
            }
        }
        if let Ok(v) = std::env::var("EMAIL_SMTP_PORT") {
            cfg.email.smtp_port = v
                .parse()
                .map_err(|_| vec!["EMAIL_SMTP_PORT must be a valid port number".to_string()])?;
        }
        if let Ok(v) = std::env::var("EMAIL_SMTP_TLS") {
            if !v.is_empty() {
                cfg.email.smtp_tls = match v.as_str() {
                    "starttls" => SmtpTls::Starttls,
                    "tls" => SmtpTls::Tls,
                    _ => return Err(vec!["EMAIL_SMTP_TLS must be starttls or tls".to_string()]),
                };
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
        if let Ok(v) = std::env::var("EMAIL_SITE_URL") {
            cfg.email.site_url = v;
        }

        cfg.validate()?;
        Ok(cfg)
    }
}
