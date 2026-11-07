//! `Config.toml`: host/port, external SurrealDB, Turnstile, rate limiting,
//! and authenticated SMTP email settings.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub site: Site,
    pub surreal: SurrealConfig,
    pub turnstile: Turnstile,
    pub ratelimit: RateLimit,
    pub email: Email,
    pub google: Google,
    pub images: ImageHost,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6785,
            site: Site::default(),
            surreal: SurrealConfig::default(),
            turnstile: Turnstile::default(),
            ratelimit: RateLimit::default(),
            email: Email::default(),
            google: Google::default(),
            images: ImageHost::default(),
        }
    }
}

/// The external image host (image.lebenoa.com): a `discord-images` service
/// that stores uploads as Discord attachments and serves them back through
/// its `/proxy/` route. Unconfigured — `base_url` empty — means the wiki
/// keeps storing images on its own disk, and every remote-upload caller
/// treats that as "feature off" rather than an error.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ImageHost {
    /// Absolute origin of the image host, e.g. `https://image.lebenoa.com`.
    /// No trailing slash: the client builds URLs by appending paths.
    pub base_url: String,
}

impl ImageHost {
    pub fn is_configured(&self) -> bool {
        !self.base_url.trim().is_empty()
    }

    /// The origin without a trailing slash, ready to append a path.
    pub fn origin(&self) -> &str {
        self.base_url.trim().trim_end_matches('/')
    }
}

/// Facts about the wiki itself rather than about a service it talks to.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Site {
    /// The site's own timezone, in minutes east of UTC — Asia/Bangkok is 420.
    ///
    /// News authoring interprets local date-time inputs in this offset;
    /// public date labels use the same zone. Unset (0) keeps the wiki on UTC.
    ///
    /// A fixed offset, not a zone name: a name would need the tz database in
    /// the binary for one form field, and this wiki's own zone (Asia/Bangkok)
    /// has never observed a daylight-saving shift. A zone that does would need
    /// the value moved twice a year.
    pub utc_offset_minutes: i64,
}

/// Google sign-in (OAuth 2.0 authorization-code flow). An unconfigured
/// section is not an error: the login form simply offers no Google button and
/// `/auth/google` answers 404, which is how a wiki that only wants local
/// accounts runs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Google {
    /// OAuth client id from the Google Cloud console.
    pub client_id: String,
    /// OAuth client secret. Server-side only — it never reaches a template.
    pub client_secret: String,
    /// The exact callback URL registered with Google. Empty derives it from
    /// `[email] site_url` (or the request host when nothing is configured),
    /// so the token exchange always echoes the URL the consent screen was
    /// started with.
    pub redirect_uri: String,
}

impl Google {
    /// Google sign-in is offered only with both halves of the credential:
    /// either one alone cannot complete a token exchange, and half a
    /// credential is a misconfiguration rather than a feature toggle.
    pub fn is_configured(&self) -> bool {
        !self.client_id.trim().is_empty() && !self.client_secret.trim().is_empty()
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
    /// Whether the datastore is configured well enough to connect: the
    /// endpoint plus the namespace/database pair the app signs in to.
    /// `Config::validate` refuses to start on an incomplete setup, so `main`
    /// reads this to say *why* it will not serve rather than to decide it.
    pub fn is_configured(&self) -> bool {
        !self.url.trim().is_empty()
            && !self.namespace.trim().is_empty()
            && !self.database.trim().is_empty()
    }
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
        // ±14:00 is the real range of civil offsets (UTC-12:00 .. UTC+14:00);
        // anything wider would silently date documents in no zone at all
        if !(-14 * 60..=14 * 60).contains(&self.site.utc_offset_minutes) {
            errors.push("[site] utc_offset_minutes must be within ±14 hours of UTC".to_string());
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
        if !self.images.base_url.is_empty()
            && self
                .images
                .base_url
                .parse::<reqwest::Url>()
                .map_or(true, |url| {
                    !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
                })
        {
            errors
                .push("[images] base_url must be an absolute http:// or https:// URL".to_string());
        }
        let google = &self.google;
        let google_partial = !google.client_id.is_empty() || !google.client_secret.is_empty();
        if google_partial && !google.is_configured() {
            errors.push("[google] client_id and client_secret are required together".to_string());
        }
        if google.client_id.trim() != google.client_id
            || google.client_secret.trim() != google.client_secret
        {
            errors.push(
                "[google] client_id and client_secret must not carry surrounding whitespace"
                    .to_string(),
            );
        }
        if !google.redirect_uri.is_empty()
            && google
                .redirect_uri
                .parse::<reqwest::Url>()
                .map_or(true, |url| {
                    !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
                })
        {
            errors.push(
                "[google] redirect_uri must be an absolute http:// or https:// URL".to_string(),
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
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

    /// Half a Google credential is a misconfiguration, not a feature toggle:
    /// the section is either complete (the button appears, the flow runs) or
    /// empty (no button, `/auth/google` 404s).
    /// A datastore-complete config, so the Google assertions below cannot be
    /// satisfied by an unrelated startup error.
    fn valid_base() -> Config {
        let mut config = Config::default();
        config.surreal.url = "http://localhost:8000".into();
        config.surreal.namespace = "wiki".into();
        config.surreal.database = "wiki".into();
        config
    }

    #[test]
    fn google_credential_is_all_or_nothing() {
        let mut config = valid_base();
        assert!(!config.google.is_configured(), "an empty section is fine");
        config.validate().unwrap();

        config.google.client_id = "123.apps.googleusercontent.com".into();
        assert!(!config.google.is_configured());
        let errors = config.validate().unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("[google]")),
            "a half-credential stops startup: {errors:?}"
        );

        config.google.client_secret = "secret".into();
        assert!(config.google.is_configured());
        config.validate().unwrap();

        // whitespace-only values are not a credential either
        config.google.client_id = "   ".into();
        assert!(!config.google.is_configured());
        assert!(config
            .validate()
            .unwrap_err()
            .iter()
            .any(|e| e.contains("whitespace")));
    }

    /// The callback URL Google is told to return to must be absolute: a
    /// relative or schemeless value produces a consent screen Google refuses
    /// and a token exchange that cannot match.
    #[test]
    fn google_redirect_uri_must_be_absolute() {
        let mut config = valid_base();
        config.google.client_id = "id".into();
        config.google.client_secret = "secret".into();

        config.google.redirect_uri = "/auth/google/callback".into();
        assert!(config
            .validate()
            .unwrap_err()
            .iter()
            .any(|e| e.contains("[google] redirect_uri")));

        config.google.redirect_uri = "https://wiki.example/auth/google/callback".into();
        config.validate().unwrap();
    }

    /// The site's offset is what a publication date means, so a value no zone
    /// on earth uses is a startup error rather than a date nothing can read:
    /// ±14:00 bounds the real range (UTC-12:00 .. UTC+14:00).
    #[test]
    fn the_site_timezone_is_bounded() {
        let mut config = valid_base();
        for offset in [0, 420, -720, 840] {
            config.site.utc_offset_minutes = offset;
            config
                .validate()
                .unwrap_or_else(|e| panic!("{offset}: {e:?}"));
        }
        for offset in [900, -900, 1441, i64::MAX] {
            config.site.utc_offset_minutes = offset;
            assert!(
                config
                    .validate()
                    .unwrap_err()
                    .iter()
                    .any(|e| e.contains("utc_offset_minutes")),
                "{offset} must be refused"
            );
        }
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

        // the site's own timezone, which schedules publish and reads show
        if let Ok(v) = std::env::var("SITE_UTC_OFFSET_MINUTES") {
            if !v.is_empty() {
                cfg.site.utc_offset_minutes = v.parse::<i64>().map_err(|_| {
                    vec!["SITE_UTC_OFFSET_MINUTES must be whole minutes east of UTC".to_string()]
                })?;
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
            cfg.turnstile.hostnames = v
                .split(',')
                .map(str::trim)
                .filter(|hostname| !hostname.is_empty())
                .map(str::to_string)
                .collect();
        }

        if let Ok(v) = std::env::var("GOOGLE_CLIENT_ID") {
            cfg.google.client_id = v;
        }
        if let Ok(v) = std::env::var("GOOGLE_CLIENT_SECRET") {
            cfg.google.client_secret = v;
        }
        if let Ok(v) = std::env::var("GOOGLE_REDIRECT_URI") {
            cfg.google.redirect_uri = v;
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
