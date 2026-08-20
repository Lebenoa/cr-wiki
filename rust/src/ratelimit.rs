//! Per-IP token bucket for the public read endpoints, ported from
//! app/ratelimit.v.
//!
//! Each IP gets `capacity` burst tokens, refilled at `refill` per second, and
//! gets a 429 while the bucket is empty, so heavy read traffic cannot starve
//! the server. One bucket per IP covers every endpoint together.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::RateLimit;

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_fill: i64,
}

#[derive(Debug)]
pub struct Limiter {
    cfg: RateLimit,
    // one lock around the whole read-modify-write: per-entry locking would let
    // parallel requests read the same balance and spend one token per wave
    buckets: Mutex<HashMap<String, Bucket>>,
}

pub enum Decision {
    Allow,
    /// seconds to advertise in Retry-After
    Deny(u64),
}

impl Limiter {
    pub fn new(cfg: RateLimit) -> Self {
        Self { cfg, buckets: Mutex::new(HashMap::new()) }
    }

    pub fn check(&self, ip: &str) -> Decision {
        // Non-release builds skip limiting, matching the V `$if !prod` gate:
        // the HTTP suite must not be throttled and local development must not
        // be rate limited.
        if cfg!(debug_assertions) {
            return Decision::Allow;
        }
        let now = now_unix();
        let mut buckets = match self.buckets.lock() {
            Ok(b) => b,
            // a poisoned lock must not take the site down; let the request run
            Err(e) => e.into_inner(),
        };

        // prune buckets idle past the TTL once the map grows; under normal
        // traffic it stays small and no sweep ever runs
        if buckets.len() as i64 > self.cfg.sweep_above {
            let ttl = self.cfg.idle_ttl;
            buckets.retain(|_, b| now - b.last_fill <= ttl);
        }

        let bucket = buckets
            .entry(ip.to_string())
            .or_insert(Bucket { tokens: self.cfg.capacity, last_fill: now });

        // refill by wall-clock elapsed time, capped at the burst capacity
        let elapsed = now - bucket.last_fill;
        if elapsed > 0 {
            bucket.tokens = (bucket.tokens + elapsed as f64 * self.cfg.refill).min(self.cfg.capacity);
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

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
