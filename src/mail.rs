//! Outgoing app mail, posted to the configured provider endpoint — Resend's
//! `POST https://api.resend.com/emails` by default. Bodies use the provider's
//! `from`/`to`/`subject`/`text`/`html` JSON shape with bearer authentication.
//! Message text is rendered from app templates and translations.
//!
//! With no API key nothing is sent: a debug build logs the message body so
//! flows can be tested locally; a release build logs an error. Callers decide
//! whether delivery failure affects their operation.

use std::time::Duration;

use crate::config::Config;

/// How long the provider gets to accept a message before the send is given up
/// on: a visitor waiting on a reset page should not hang on a slow mail API.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// How much of a refused response's body reaches the log — enough to read the
/// provider's reason, short enough that a large error page cannot flood it.
const ERROR_BODY_CAP: usize = 400;

/// One message, already rendered in the visitor's language.
pub struct Mail {
    pub subject: String,
    pub text: String,
    pub html: String,
}

/// `g***@example.com`: the log names the account without recording the whole
/// address, which belongs to the visitor and not to the log.
fn mask(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return "***".to_string();
    };
    let mut chars = local.chars();
    match chars.next() {
        // one character of local part reveals less than the domain already does
        Some(first) if chars.next().is_some() => format!("{first}***@{domain}"),
        _ => format!("***@{domain}"),
    }
}

/// The provider's JSON body, apart from the send so a test can read it without
/// a network.
fn payload(from: &str, to: &str, mail: &Mail) -> serde_json::Value {
    serde_json::json!({
        "from": from,
        "to": [to],
        "subject": mail.subject,
        "text": mail.text,
        "html": mail.html,
    })
}

/// The first `cap` characters of a provider body, cut on a character boundary
/// so a multi-byte message cannot be logged as broken UTF-8.
fn capped(body: &str, cap: usize) -> String {
    if body.chars().count() <= cap {
        return body.to_string();
    }
    let mut out: String = body.chars().take(cap).collect();
    out.push('…');
    out
}

/// Sends one message. `true` only when the provider accepted it (2xx).
///
/// `to` is never request input: the caller passes the address stored on the
/// account the reset was asked for, so a forged form cannot aim the mail
/// anywhere. A refusal is logged with its status and reason and reported
/// `false` — there is nothing useful to tell the visitor, and saying more
/// would confirm whether the address exists.
pub async fn send(cfg: &Config, to: &str, mail: &Mail) -> bool {
    match cfg.email.transport.as_str() {
        "api" => send_api(cfg, to, mail).await,
        "smtp" => send_smtp(cfg, to, mail).await,
        other => {
            tracing::error!(transport = other, "email: unsupported transport");
            false
        }
    }
}

async fn send_api(cfg: &Config, to: &str, mail: &Mail) -> bool {
    if cfg.email.api_key.is_empty() {
        if cfg!(debug_assertions) {
            tracing::info!(
                to = %mask(to),
                "email: no [email] api_key configured, so nothing was sent; the message body follows for local development:\n{}",
                mail.text
            );
        } else {
            tracing::error!(to = %mask(to), "email: no API key configured; message not sent");
        }
        return false;
    }
    if cfg.email.from.is_empty() {
        tracing::error!("email: [email] from is empty");
        return false;
    }

    let client = match reqwest::Client::builder().timeout(SEND_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("email: client build failed: {e}");
            return false;
        }
    };
    let res = client
        .post(&cfg.email.endpoint)
        .bearer_auth(&cfg.email.api_key)
        .json(&payload(&cfg.email.from, to, mail))
        .send()
        .await;

    match res {
        Ok(r) if r.status().is_success() => {
            tracing::info!(to = %mask(to), "email: message accepted by API provider");
            true
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            tracing::error!(
                to = %mask(to), http_status = status.as_u16(),
                reason = %capped(&body, ERROR_BODY_CAP),
                "email: API provider refused the send"
            );
            false
        }
        Err(e) => {
            tracing::error!(to = %mask(to), "email: API send failed: {e}");
            false
        }
    }
}

async fn send_smtp(cfg: &Config, to: &str, mail: &Mail) -> bool {
    use lettre::{
        message::{header::ContentType, Mailbox, MultiPart, SinglePart},
        transport::smtp::authentication::Credentials,
        AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    };

    if cfg.email.smtp_host.is_empty()
        || cfg.email.smtp_username.is_empty()
        || cfg.email.smtp_password.is_empty()
        || cfg.email.from.is_empty()
    {
        tracing::error!("email: SMTP host, credentials, and from are required");
        return false;
    }

    let from: Mailbox = match cfg.email.from.parse() {
        Ok(address) => address,
        Err(e) => {
            tracing::error!("email: invalid from mailbox: {e}");
            return false;
        }
    };
    let to: Mailbox = match to.parse() {
        Ok(address) => address,
        Err(e) => {
            tracing::error!("email: invalid recipient mailbox: {e}");
            return false;
        }
    };
    let masked_to = mask(&to.to_string());
    let message = match lettre::Message::builder()
        .from(from)
        .to(to)
        .subject(&mail.subject)
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(mail.text.clone()),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(mail.html.clone()),
                ),
        ) {
        Ok(message) => message,
        Err(e) => {
            tracing::error!("email: SMTP message build failed: {e}");
            return false;
        }
    };

    let builder = match cfg.email.smtp_tls.as_str() {
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.email.smtp_host),
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.email.smtp_host),
        mode => {
            tracing::error!(smtp_tls = mode, "email: unsupported SMTP TLS mode");
            return false;
        }
    };
    let mut builder = match builder {
        Ok(builder) => builder.port(cfg.email.smtp_port),
        Err(e) => {
            tracing::error!("email: invalid SMTP relay host: {e}");
            return false;
        }
    };
    builder = builder.credentials(Credentials::new(
        cfg.email.smtp_username.clone(),
        cfg.email.smtp_password.clone(),
    ));
    let transport = builder.build();
    match tokio::time::timeout(SEND_TIMEOUT, transport.send(message)).await {
        Ok(Ok(_)) => {
            tracing::info!(to = %masked_to, "email: message accepted by SMTP server");
            true
        }
        Ok(Err(e)) => {
            tracing::error!(to = %masked_to, "email: SMTP send failed: {e}");
            false
        }
        Err(_) => {
            tracing::error!(to = %masked_to, "email: SMTP send timed out");
            false
        }
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn mail() -> Mail {
        Mail {
            subject: "Reset your password".into(),
            text: "https://cookierun.example/reset-password?token=abc".into(),
            html: "<p>reset</p>".into(),
        }
    }

    #[test]
    fn smtp_transport_defaults_to_authenticated_starttls() {
        let cfg = Config::default();
        assert_eq!(cfg.email.transport, "api");
        assert_eq!(cfg.email.smtp_port, 587);
        assert_eq!(cfg.email.smtp_tls, "starttls");
    }

    #[tokio::test]
    async fn smtp_requires_credentials_before_connecting() {
        let mut cfg = Config::default();
        cfg.email.transport = "smtp".into();
        assert!(!send(&cfg, "ginger@cookie.run", &mail()).await);
    }

    #[tokio::test]
    async fn smtp_rejects_plaintext_tls_mode() {
        let mut cfg = Config::default();
        cfg.email.transport = "smtp".into();
        cfg.email.smtp_host = "mail.lebenoa.com".into();
        cfg.email.smtp_username = "account".into();
        cfg.email.smtp_password = "secret".into();
        cfg.email.from = "Wiki <no-reply@lebenoa.com>".into();
        cfg.email.smtp_tls = "none".into();
        assert!(!send(&cfg, "ginger@cookie.run", &mail()).await);
    }

    #[tokio::test]
    async fn smtp_starttls_requests_tls_before_authentication() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut read = BufReader::new(read);
            write.write_all(b"220 localhost ESMTP\r\n").await.unwrap();
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            assert!(line.starts_with("EHLO "), "{line}");
            write
                .write_all(b"250-localhost\r\n250-STARTTLS\r\n250 AUTH PLAIN LOGIN\r\n")
                .await
                .unwrap();
            line.clear();
            read.read_line(&mut line).await.unwrap();
            assert!(line.starts_with("STARTTLS"), "{line}");
            write.write_all(b"220 begin TLS\r\n").await.unwrap();
        });

        let mut cfg = Config::default();
        cfg.email.transport = "smtp".into();
        cfg.email.smtp_host = "localhost".into();
        cfg.email.smtp_port = address.port();
        cfg.email.smtp_tls = "starttls".into();
        cfg.email.smtp_username = "account".into();
        cfg.email.smtp_password = "secret".into();
        cfg.email.from = "Wiki <no-reply@lebenoa.com>".into();
        assert!(!send(&cfg, "ginger@cookie.run", &mail()).await);
        server.await.unwrap();
    }

    /// The body is what the provider reads, so its fields are pinned: Resend
    /// takes `to` as an array and needs at least one of `text`/`html`.
    #[test]
    fn payload_carries_the_fields_resend_expects() {
        let body = payload("Wiki <no-reply@example.com>", "ginger@cookie.run", &mail());
        assert_eq!(body["from"], "Wiki <no-reply@example.com>");
        assert_eq!(body["to"][0], "ginger@cookie.run");
        assert_eq!(body["to"].as_array().map(Vec::len), Some(1));
        assert_eq!(body["subject"], "Reset your password");
        assert!(body["text"]
            .as_str()
            .unwrap()
            .contains("/reset-password?token="));
        assert!(body["html"].as_str().unwrap().contains("<p>"));
    }

    /// An unconfigured sender sends nothing and reports `false` rather than a
    /// hopeful `true`: the caller's response is neutral either way, so the
    /// return value is the only honest account of what happened. No network is
    /// touched — the key check comes first.
    #[tokio::test]
    async fn missing_api_key_sends_nothing() {
        let cfg = Config::default();
        assert!(!send(&cfg, "ginger@cookie.run", &mail()).await);
    }

    /// The mask keeps the address out of the log while still naming it.
    #[test]
    fn mask_keeps_the_address_out_of_the_log() {
        assert_eq!(mask("ginger@cookie.run"), "g***@cookie.run");
        assert_eq!(mask("a@b.co"), "***@b.co");
        assert_eq!(mask("not-an-address"), "***");
        assert!(!mask("ginger@cookie.run").contains("inger"));
    }

    /// The cap counts characters, not bytes, so a multi-byte body is never cut
    /// mid-character.
    #[test]
    fn capped_body_cuts_on_character_boundaries() {
        let long = "ก".repeat(ERROR_BODY_CAP + 10);
        let out = capped(&long, ERROR_BODY_CAP);
        assert_eq!(out.chars().count(), ERROR_BODY_CAP + 1);
        assert!(out.ends_with('…'));
        assert_eq!(capped("short", ERROR_BODY_CAP), "short");
    }
}
