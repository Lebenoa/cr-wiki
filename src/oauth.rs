//! Google sign-in: half provider client, half state store.
//!
//! The flow is the authorization-code one. `/auth/google` mints a single-use
//! `state`, mirrors it into a cookie and sends the visitor to Google's consent
//! screen; `/auth/google/callback` proves the callback belongs to *this*
//! browser (cookie == state) and that the state was unspent and young, then
//! exchanges the code for an access token and reads the profile off Google's
//! userinfo endpoint.
//!
//! Two deliberate omissions. There is no PKCE: the client is confidential (it
//! holds a secret) and the code is redeemed server-side over TLS, which is the
//! case PKCE exists to cover clients that cannot keep a secret. And there is
//! no id-token signature check: the profile comes from Google's userinfo
//! endpoint authenticated by the access token we just received directly from
//! Google's token endpoint over TLS, so there is no third party to verify
//! against — verifying a JWT we fetched ourselves would need a JWT dependency
//! for no added assurance.
//!
//! Everything fails closed: an outage, a mismatched state and a tampered code
//! all end as the same generic sign-in page with no session.

use std::collections::HashMap;

use parking_lot::Mutex;
use serde::Deserialize;

use crate::config::Google as GoogleConfig;
use crate::routes::auth::MIN_USERNAME_LEN;
use crate::time::now_unix;

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const USERINFO_ENDPOINT: &str = "https://openidconnect.googleapis.com/v1/userinfo";

/// `openid` is what makes `sub` stable; `email` and `profile` carry the two
/// values the account is built from.
const SCOPE: &str = "openid email profile";

/// The cookie holding the state a browser is waiting on. Named apart from the
/// session cookie so a shared or cleared session cannot leave it behind.
pub const STATE_COOKIE: &str = "CROAUTH";

/// The path the state cookie is scoped to — the flow's own two URLs, so no
/// other request carries it.
pub const STATE_COOKIE_PATH: &str = "/auth/google";

/// A state token is good for ten minutes: enough to pick an account and click
/// through consent, short enough that a URL left in tab history cannot be
/// replayed.
pub const STATE_TTL_SECS: i64 = 10 * 60;

/// Per-request ceiling, matching the Turnstile check's.
const HTTP_TIMEOUT_SECS: u64 = 10;

/// The longest OAuth code worth forwarding: Google's are far shorter, and a
/// body of arbitrary length has no business being posted on a stranger's say.
const MAX_CODE_LEN: usize = 2048;

/// A display name may claim the base itself, then `-2` .. `-5`; past that the
/// suffix is random, so a genuinely contested name still resolves instead of
/// failing the sign-in.
pub const MAX_USERNAME_ATTEMPTS: u32 = 5;

/// Display names can be far longer than a name anyone types into the register
/// form; the label stays readable at this width.
const MAX_USERNAME_LEN: usize = 32;

/// Random characters appended when a profile yields no usable name.
const RANDOM_SUFFIX_LEN: usize = 6;

/// The single-use `state` values outstanding right now, keyed by token and
/// stamped with the second they were minted. In memory like the sessions:
/// restarting drops the flows in flight, and the visitor retries.
#[derive(Debug, Default)]
pub struct States {
    inner: Mutex<HashMap<String, i64>>,
}

impl States {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mints a state and sweeps the expired ones on the way in, so a stream of
    /// abandoned consent screens cannot grow the map without bound.
    pub fn issue(&self) -> String {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let now = now_unix();
        let mut map = self.inner.lock();
        map.retain(|_, issued| now.saturating_sub(*issued) <= STATE_TTL_SECS);
        map.insert(token.clone(), now);
        token
    }

    /// Spends a state. `false` covers unknown, expired and replayed alike —
    /// the caller needs one verdict, and telling them apart would only give
    /// an attacker a probe.
    pub fn consume(&self, token: &str) -> bool {
        if token.is_empty() {
            return false;
        }
        let now = now_unix();
        let mut map = self.inner.lock();
        match map.remove(token) {
            Some(issued) => now.saturating_sub(issued) <= STATE_TTL_SECS,
            None => false,
        }
    }
}

/// Percent-encodes one query value. Everything outside the unreserved set is
/// escaped, so no value can close the query, start a fragment, or put a
/// control byte into the `Location` header the redirect carries.
fn encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(*byte));
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    out
}

/// The consent-screen URL for one flow. Always a URL this module built from
/// configured values and its own state, so the caller can put it in a
/// `Location` header without re-checking it.
pub fn authorize_url(cfg: &GoogleConfig, redirect_uri: &str, state: &str) -> String {
    format!(
        "{AUTH_ENDPOINT}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}\
         &prompt=select_account",
        encode(&cfg.client_id),
        encode(redirect_uri),
        encode(SCOPE),
        encode(state)
    )
}

/// One HTTP client per call: the SDK's pooled client is not shared here
/// because the OAuth client is used twice per sign-in at most, and building it
/// keeps this module free of state beyond `States`.
fn client() -> Option<reqwest::Client> {
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!("google: client build failed: {e}");
            None
        }
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
}

/// Redeems the code for an access token. `None` on any failure — a refused
/// code, a network error and an unparsable body are the same answer to the
/// visitor: no session.
pub async fn exchange_code(
    cfg: &GoogleConfig,
    redirect_uri: &str,
    code: &str,
) -> Option<String> {
    let client = client()?;
    let res = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("code", code),
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await;
    let res = match res {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            // the body carries Google's reason (invalid_grant, redirect_uri_mismatch…),
            // which is a short code rather than anything a visitor typed
            tracing::warn!("google: token exchange returned HTTP {}", r.status());
            return None;
        }
        Err(e) => {
            tracing::warn!("google: token exchange failed: {e}");
            return None;
        }
    };
    let parsed: TokenResponse = match res.json().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("google: unparsable token response: {e}");
            return None;
        }
    };
    if parsed.access_token.is_empty() {
        tracing::warn!("google: token response carried no access token");
        return None;
    }
    Some(parsed.access_token)
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    #[serde(default)]
    sub: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    email_verified: bool,
}

/// What Google says about the visitor: the immutable subject id, the display
/// name, and the address — kept only when Google reports it verified.
#[derive(Debug, Clone)]
pub struct Identity {
    pub sub: String,
    pub name: String,
    /// `None` unless Google verified the address. An unverified one must never
    /// be matched against an existing account: the register form lets anyone
    /// claim any address, so a match alone proves nothing, but Google's
    /// verified flag proves the visitor reads that mailbox.
    pub email: Option<String>,
}

/// Reads the profile behind an access token. `None` when Google answers with
/// something unusable; a token with no subject cannot key an account, and
/// inventing one would hand every such visitor the same account.
pub async fn userinfo(access_token: &str) -> Option<Identity> {
    let client = client()?;
    let res = client
        .get(USERINFO_ENDPOINT)
        .bearer_auth(access_token)
        .send()
        .await;
    let res = match res {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!("google: userinfo returned HTTP {}", r.status());
            return None;
        }
        Err(e) => {
            tracing::warn!("google: userinfo request failed: {e}");
            return None;
        }
    };
    let parsed: UserInfo = match res.json().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("google: unparsable userinfo body: {e}");
            return None;
        }
    };
    let identity = to_identity(parsed);
    if identity.is_none() {
        tracing::warn!("google: userinfo carried no usable subject");
    }
    identity
}

/// The profile rules, split out of the request so they are testable without a
/// network: a subject is mandatory, and the address is kept only when Google
/// reports it verified.
///
/// `None` for a body with no subject — every such visitor would otherwise
/// share the account keyed by the empty string.
fn to_identity(parsed: UserInfo) -> Option<Identity> {
    let sub = parsed.sub.trim();
    if sub.is_empty() || sub.len() > 128 {
        return None;
    }
    let email = parsed.email.trim();
    let email = if parsed.email_verified && !email.is_empty() {
        Some(email.to_string())
    } else {
        None
    };
    Some(Identity {
        sub: sub.to_string(),
        name: parsed.name,
        email,
    })
}

/// Guards the code before it is posted anywhere: non-empty and bounded. The
/// value itself is never logged and never reaches a template.
pub fn usable_code(code: Option<&str>) -> Option<&str> {
    code.filter(|c| !c.is_empty() && c.len() <= MAX_CODE_LEN)
}

/// The letters, digits and separators of a profile string, trimmed of edge
/// punctuation and capped. Spaces and emoji are dropped rather than encoded
/// away: a username is typed back by a human, and this is the closest thing to
/// the name they go by.
fn sanitize(raw: &str) -> String {
    let kept: String = raw
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .take(MAX_USERNAME_LEN)
        .collect();
    kept.trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

/// The part of an address before the `@`.
fn local_part(email: &str) -> String {
    email.split('@').next().unwrap_or_default().to_string()
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// Appends random digits so a name too short for the register form's floor
/// still becomes a usable username. Random rather than a fixed word: two
/// profiles that sanitize to the same pair of letters must not collide on a
/// constant suffix they would then fight over.
fn pad_with_random(seed: &str) -> String {
    let hex = uuid::Uuid::new_v4().simple().to_string();
    let suffix = truncate(&hex, RANDOM_SUFFIX_LEN);
    let room = MAX_USERNAME_LEN.saturating_sub(suffix.chars().count());
    format!("{}{suffix}", truncate(seed, room))
}

/// The username a Google profile claims first: its display name, or the
/// address's local part when the name carries nothing usable or is too short.
/// Both empty — a profile with neither a name nor an address — falls back to
/// the random pad alone, which is what the account is: an anonymous visitor.
pub fn derive_username(name: &str, email: Option<&str>) -> String {
    let from_name = sanitize(name);
    let from_email = email.map(local_part).map(|local| sanitize(&local)).unwrap_or_default();
    for candidate in [&from_name, &from_email] {
        if candidate.chars().count() >= MIN_USERNAME_LEN {
            return candidate.clone();
        }
    }
    let seed = if from_name.is_empty() {
        &from_email
    } else {
        &from_name
    };
    pad_with_random(seed)
}

/// The `attempt`th username for a display name: the base itself, then `-2`,
/// `-3`, … and past the last numbered attempt a random suffix. Each candidate
/// is still offered to the unique index, so the numbering is a hint rather
/// than a reservation — a race lands on the next number instead of failing.
pub fn username_candidate(base: &str, attempt: u32) -> String {
    if attempt == 0 {
        return base.to_string();
    }
    if attempt < MAX_USERNAME_ATTEMPTS {
        let suffix = format!("-{}", attempt + 1);
        return format!(
            "{}{suffix}",
            truncate(base, MAX_USERNAME_LEN.saturating_sub(suffix.len()))
        );
    }
    pad_with_random(base)
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// A state is single use: the first consume wins, the replay does not, and
    /// an empty or never-issued value is refused outright.
    #[test]
    fn states_are_single_use() {
        let states = States::new();
        let token = states.issue();
        assert_eq!(token.len(), 32, "{token}");
        assert!(states.consume(&token));
        assert!(!states.consume(&token), "a replay must not verify");
        assert!(!states.consume(""), "an empty state is never valid");
        assert!(!states.consume("never-issued"));
        assert_ne!(token, states.issue(), "two flows never share a state");
    }

    /// A state that sat past its TTL is refused even though nothing swept it
    /// yet — the sweep is a memory bound, not the expiry check.
    #[test]
    fn states_expire() {
        let states = States::new();
        let stale = states.issue();
        states
            .inner
            .lock()
            .insert(stale.clone(), now_unix() - STATE_TTL_SECS - 1);
        assert!(!states.consume(&stale));

        // and issuing sweeps it away rather than carrying it forward
        let fresh = states.issue();
        assert!(!states.inner.lock().contains_key(&stale));
        assert!(states.consume(&fresh));
    }

    /// The consent URL carries every parameter Google requires, and a state
    /// with metacharacters in it cannot escape the query — which is what keeps
    /// the redirect header safe to build.
    #[test]
    fn authorize_url_encodes_every_value() {
        let cfg = GoogleConfig {
            client_id: "123-abc.apps.googleusercontent.com".into(),
            client_secret: "secret".into(),
            redirect_uri: String::new(),
        };
        let url = authorize_url(&cfg, "https://wiki.example/auth/google/callback", "a b&c=d#e");
        assert!(url.starts_with(AUTH_ENDPOINT), "{url}");
        assert!(
            url.contains("client_id=123-abc.apps.googleusercontent.com"),
            "{url}"
        );
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("prompt=select_account"), "{url}");
        assert!(
            url.contains("redirect_uri=https%3A%2F%2Fwiki.example%2Fauth%2Fgoogle%2Fcallback"),
            "{url}"
        );
        assert!(url.contains("state=a%20b%26c%3Dd%23e"), "{url}");
        assert!(
            url.is_ascii() && !url.chars().any(char::is_control),
            "a Location header value must be printable ASCII: {url}"
        );
    }

    /// A username is built from the profile, never guessed: the display name
    /// wins, the address stands in when the name is unusable, and anything
    /// under the register form's floor gets a random pad so the account can
    /// still be created.
    #[test]
    fn usernames_come_from_the_profile() {
        assert_eq!(derive_username("Ginger Brave", None), "GingerBrave");
        assert_eq!(derive_username("  .Ginger- ", None), "Ginger");
        assert_eq!(
            derive_username("", Some("ginger@cookie.run")),
            "ginger",
            "the local part stands in for a missing name"
        );
        assert_eq!(
            derive_username("Ginger", Some("other@cookie.run")),
            "Ginger",
            "the display name wins over the address, and its case is kept"
        );
        assert_eq!(derive_username("ようこそ", None), "ようこそ", "characters, not bytes");

        // a two-character handle is below the register floor, so it grows a
        // random tail instead of shipping a name the register form refuses
        let short = derive_username("Jo", None);
        assert!(short.starts_with("Jo"), "{short}");
        assert!(short.chars().count() >= MIN_USERNAME_LEN, "{short}");
        assert_ne!(short, derive_username("Jo", None));

        // emoji and spaces alone leave nothing to keep
        let anonymous = derive_username("😀 😀", None);
        assert_eq!(anonymous.chars().count(), RANDOM_SUFFIX_LEN, "{anonymous}");
        assert!(anonymous.chars().all(|c| c.is_ascii_hexdigit()), "{anonymous}");

        // a long name is capped, and the letters survive the cap
        let long = derive_username(&"a".repeat(200), None);
        assert_eq!(long.chars().count(), MAX_USERNAME_LEN);
    }

    /// Numbered candidates are stable and distinct, and past the last number
    /// the suffix goes random — so a name that is genuinely contested resolves
    /// instead of failing the sign-in.
    #[test]
    fn candidates_number_then_randomize() {
        assert_eq!(username_candidate("Ginger", 0), "Ginger");
        assert_eq!(username_candidate("Ginger", 1), "Ginger-2");
        assert_eq!(username_candidate("Ginger", 2), "Ginger-3");
        let last = username_candidate("Ginger", MAX_USERNAME_ATTEMPTS - 1);
        assert_eq!(last, "Ginger-5", "{last}");

        let random = username_candidate("Ginger", MAX_USERNAME_ATTEMPTS);
        assert!(random.starts_with("Ginger"), "{random}");
        assert_ne!(random, username_candidate("Ginger", MAX_USERNAME_ATTEMPTS));
        assert!(random.chars().count() <= MAX_USERNAME_LEN, "{random}");

        // the suffix never pushes a name past the cap
        let long = username_candidate(&"b".repeat(MAX_USERNAME_LEN), 1);
        assert_eq!(long.chars().count(), MAX_USERNAME_LEN, "{long}");
        assert!(long.ends_with("-2"), "{long}");
    }

    /// The profile rules: a subject is mandatory, and an address is trusted
    /// only when Google says it verified it. An unverified address is dropped
    /// rather than stored — the register form lets anyone claim any address, so
    /// a match on one proves nothing.
    #[test]
    fn identity_requires_a_subject_and_a_verified_address() {
        let verified = to_identity(UserInfo {
            sub: " 12345 ".into(),
            name: "Ginger Brave".into(),
            email: " ginger@cookie.run ".into(),
            email_verified: true,
        })
        .expect("subject present");
        assert_eq!(verified.sub, "12345", "the subject is trimmed, never case-folded");
        assert_eq!(verified.name, "Ginger Brave");
        assert_eq!(verified.email.as_deref(), Some("ginger@cookie.run"));

        let unverified = to_identity(UserInfo {
            sub: "12345".into(),
            name: String::new(),
            email: "ginger@cookie.run".into(),
            email_verified: false,
        })
        .expect("subject present");
        assert_eq!(unverified.email, None, "an unverified address is never used");

        let blank = to_identity(UserInfo {
            sub: "   ".into(),
            name: "Ginger".into(),
            email: "ginger@cookie.run".into(),
            email_verified: true,
        });
        assert!(blank.is_none(), "no subject means no account");
        assert!(to_identity(UserInfo {
            sub: "x".repeat(129),
            name: String::new(),
            email: String::new(),
            email_verified: true,
        })
        .is_none());
    }

    /// A code is only forwarded when it looks like one: empty or oversized
    /// values are refused before any request is made.
    #[test]
    fn codes_are_bounded() {
        assert_eq!(usable_code(Some("4/0Abc")), Some("4/0Abc"));
        assert_eq!(usable_code(None), None);
        assert_eq!(usable_code(Some("")), None);
        assert_eq!(usable_code(Some(&"x".repeat(MAX_CODE_LEN + 1))), None);
    }
}
