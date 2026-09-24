//! One clock for the whole app.

/// Unix seconds now; 0 before the epoch rather than a panic, so every caller
/// — session TTL, rate-limit refill, build expiry — reads the same number.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}
