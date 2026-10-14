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
    if cfg.email.api_key.is_empty() {
        if cfg!(debug_assertions) {
            tracing::info!(
                to = %mask(to),
                "email: no [email] api_key configured, so nothing was sent; the message body follows for local development:\n{}",
                mail.text
            );
        } else {
            tracing::error!(
                to = %mask(to),
                "email: no [email] api_key configured; the message was not sent"
            );
        }
        return false;
    }
    if cfg.email.from.is_empty() {
        tracing::error!("email: [email] from is empty; the provider cannot accept this send");
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
            tracing::info!(to = %mask(to), "email: message accepted by the provider");
            true
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            tracing::error!(
                to = %mask(to),
                http_status = status.as_u16(),
                reason = %capped(&body, ERROR_BODY_CAP),
                "email: the provider refused the send"
            );
            false
        }
        Err(e) => {
            tracing::error!(to = %mask(to), "email: send failed: {e}");
            false
        }
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    fn mail() -> Mail {
        Mail {
            subject: "Reset your password".into(),
            text: "https://cookierun.example/reset-password?token=abc".into(),
            html: "<p>reset</p>".into(),
        }
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
