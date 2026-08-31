//! Cloudflare Turnstile, checked on the forms that accept public input.
//!
//! Ported from app/turnstile.v, including its refusals: a submission is
//! rejected when the secret is unset or the hostname allowlist is empty,
//! rather than waved through. A misconfigured deployment should stop taking
//! submissions, not silently stop checking them.

#[cfg(not(debug_assertions))]
use serde::Deserialize;

use crate::config::Config;

/// The subset of Cloudflare's siteverify response the check enforces on.
/// Compiled only with the release verifier that reads it.
#[cfg(not(debug_assertions))]
#[derive(Debug, Deserialize)]
struct SiteVerify {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    action: String,
    #[serde(default)]
    hostname: String,
}

/// The public Cloudflare Turnstile site key. It ships in the frontend by
/// design; only the secret stays server-side.
pub const SITEKEY: &str = "0x4AAAAAAERZ9YLl2pq5_2hu";

/// Verifies one submission. `expected_action` is the data-action the widget
/// was rendered with, so a token minted for the login form cannot be replayed
/// against the build form.
///
/// Non-release builds skip the check, matching the V `$if !prod` gate: the
/// HTTP suite and local development have no widget to solve. This is `#[cfg]`,
/// not `cfg!`, so neither build compiles the other path.
#[cfg(debug_assertions)]
#[allow(clippy::unused_async, clippy::needless_pass_by_ref_mut)] // signature parity with the release path
pub async fn verify(_cfg: &Config, _token: Option<&str>, _expected_action: &str) -> bool {
    true
}

#[cfg(not(debug_assertions))]
pub async fn verify(cfg: &Config, token: Option<&str>, expected_action: &str) -> bool {
    if cfg.turnstile.secret.is_empty() {
        tracing::warn!("turnstile: no secret configured, rejecting submission");
        return false;
    }
    let hostnames: Vec<&str> = cfg
        .turnstile
        .hostnames
        .split(',')
        .map(|h| h.trim())
        .filter(|h| !h.is_empty())
        .collect();
    if hostnames.is_empty() {
        tracing::warn!("turnstile: no hostname allowlist configured, rejecting submission");
        return false;
    }
    let Some(token) = token.filter(|t| !t.is_empty() && t.len() <= 2048) else {
        tracing::warn!("turnstile: missing or oversized cf-turnstile-response");
        return false;
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("turnstile: client build failed: {e}");
            return false;
        }
    };
    let res = client
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .form(&[("secret", cfg.turnstile.secret.as_str()), ("response", token)])
        .send()
        .await;

    let res = match res {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!("turnstile: siteverify returned HTTP {}", r.status());
            return false;
        }
        Err(e) => {
            tracing::warn!("turnstile: siteverify request failed: {e}");
            return false;
        }
    };
    let parsed: SiteVerify = match res.json().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("turnstile: unparsable siteverify body: {e}");
            return false;
        }
    };
    if !parsed.success
        || parsed.action != expected_action
        || !hostnames.contains(&parsed.hostname.as_str())
    {
        tracing::warn!(
            "turnstile: rejected (success={} action={} hostname={})",
            parsed.success,
            parsed.action,
            parsed.hostname
        );
        return false;
    }
    true
}
