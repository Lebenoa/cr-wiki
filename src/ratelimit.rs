//! Per-IP token bucket for the public read endpoints, ported from
//! `app/ratelimit.v`.
//!
//! Each IP gets `capacity` burst tokens, refilled at `refill` per second, and
//! gets a 429 while the bucket is empty, so heavy read traffic cannot starve
//! the server. One bucket per IP covers every endpoint together.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::RateLimit;

/// Hard ceiling on the bucket map: the idle sweep keeps ordinary traffic
/// small, but a flood of unique addresses must not turn the map into an
/// attacker-sized allocation.
#[cfg_attr(debug_assertions, allow(dead_code))]
const MAX_BUCKETS: usize = 65_536;

#[cfg_attr(debug_assertions, allow(dead_code))]
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_fill: i64,
}

// In debug builds the storage exists for the release path only: `check`
// short-circuits to Allow and never touches it.
#[cfg_attr(debug_assertions, allow(dead_code))]
#[derive(Debug)]
pub struct Limiter {
    cfg: RateLimit,
    // one lock around the whole read-modify-write: per-entry locking would let
    // parallel requests read the same balance and spend one token per wave
    buckets: Mutex<HashMap<String, Bucket>>,
}

#[cfg_attr(debug_assertions, allow(dead_code))]
pub enum Decision {
    Allow,
    /// seconds to advertise in Retry-After
    Deny(u64),
}

impl Limiter {
    pub fn new(cfg: RateLimit) -> Self {
        Self { cfg, buckets: Mutex::new(HashMap::new()) }
    }

    /// Non-release builds skip limiting entirely, matching the old V `$if
    /// !prod` gate: the HTTP suite must not be throttled and local development
    /// must not be rate limited. This is `#[cfg]`, not `cfg!`, so neither
    /// build compiles — or can accidentally re-enable — the other path.
    #[cfg(debug_assertions)]
    #[allow(clippy::unused_self, clippy::missing_const_for_fn)]
    pub fn check(&self, _ip: &str) -> Decision {
        Decision::Allow
    }

    /// The token bucket itself, compiled only into release binaries.
    #[cfg(not(debug_assertions))]
    pub fn check(&self, ip: &str) -> Decision {
        let now = now_unix();
        let mut buckets = match self.buckets.lock() {
            Ok(b) => b,
            // a poisoned lock must not take the site down; let the request run
            Err(e) => e.into_inner(),
        };

        // prune buckets idle past the TTL once the map grows; under normal
        // traffic it stays small and no sweep ever runs. A flood of unique
        // addresses must not grow the map without bound: past MAX_BUCKETS a
        // new key evicts the least-recently-refilled entry (one scan, no
        // allocation of key vectors).
        if buckets.len() as i64 > self.cfg.sweep_above {
            let ttl = self.cfg.idle_ttl;
            buckets.retain(|_, b| now.saturating_sub(b.last_fill) <= ttl);
        }
        if !buckets.contains_key(ip) && buckets.len() >= MAX_BUCKETS {
            let oldest = buckets
                .iter()
                .min_by_key(|(_, b)| b.last_fill)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                buckets.remove(&k);
            }
        }

        let bucket = buckets
            .entry(ip.to_string())
            .or_insert(Bucket { tokens: self.cfg.capacity, last_fill: now });

        // refill by wall-clock elapsed time, capped at the burst capacity
        let elapsed = now.saturating_sub(bucket.last_fill);
        if elapsed > 0 {
            bucket.tokens = (bucket.tokens
                + i64::try_from(elapsed).unwrap_or(0) as f64 * self.cfg.refill)
                .min(self.cfg.capacity);
            bucket.last_fill = now;
        }

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Decision::Allow
        } else {
            // the next token lands within one refill tick, so advertise that;
            // a large burst can leave the bucket deep in the red, hence the +1
            let wait = ((1.0 - bucket.tokens) / self.cfg.refill) as u64 + 1;
            Decision::Deny(wait)
        }
    }
}

#[cfg_attr(debug_assertions, allow(dead_code))]
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}
