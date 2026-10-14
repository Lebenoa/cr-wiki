//! Outgoing app mail through authenticated SMTP. Message text is rendered
//! from app templates and translations; failures do not disclose whether an
//! account exists.

use std::time::Duration;

use crate::config::Config;

/// How long SMTP submission may take before the send is given up on.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
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

/// Sends one message through authenticated SMTP. Returns true only when the
/// SMTP server accepts the message. Delivery failures do not reveal account
/// existence to visitors.
pub async fn send(cfg: &Config, to: &str, mail: &Mail) -> bool {
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

    let builder = match cfg.email.smtp_tls {
        crate::config::SmtpTls::Starttls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.email.smtp_host)
        }
        crate::config::SmtpTls::Tls => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.email.smtp_host)
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
    fn smtp_defaults_to_authenticated_starttls() {
        let cfg = Config::default();
        assert_eq!(cfg.email.smtp_host, "");
        assert_eq!(cfg.email.smtp_port, 587);
        assert_eq!(cfg.email.smtp_tls, crate::config::SmtpTls::Starttls);
    }

    #[tokio::test]
    async fn smtp_requires_credentials_before_connecting() {
        let cfg = Config::default();
        assert!(!send(&cfg, "ginger@cookie.run", &mail()).await);
    }

    #[tokio::test]
    async fn smtp_requires_configured_tls_mode() {
        let mut cfg = Config::default();
        cfg.email.smtp_host = "mail.lebenoa.com".into();
        cfg.email.smtp_password = "secret".into();
        cfg.email.smtp_tls = crate::config::SmtpTls::Tls;
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
        cfg.email.smtp_host = "localhost".into();
        cfg.email.smtp_port = address.port();
        cfg.email.smtp_tls = crate::config::SmtpTls::Starttls;
        cfg.email.smtp_username = "account".into();
        cfg.email.smtp_password = "secret".into();
        cfg.email.from = "Wiki <no-reply@lebenoa.com>".into();
        assert!(!send(&cfg, "ginger@cookie.run", &mail()).await);
        server.await.unwrap();
    }

    /// The mask keeps the address out of the log while still naming it.
    #[test]
    fn mask_keeps_the_address_out_of_the_log() {
        assert_eq!(mask("ginger@cookie.run"), "g***@cookie.run");
        assert_eq!(mask("a@b.co"), "***@b.co");
        assert_eq!(mask("not-an-address"), "***");
        assert!(!mask("ginger@cookie.run").contains("inger"));
    }
}
