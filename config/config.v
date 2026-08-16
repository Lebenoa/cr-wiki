module config

import toml

pub struct Config {
pub:
	host string = "127.0.0.1"
	port int = 6785
	db_file string = "sqlite.db"
	// Cloudflare Turnstile: the secret used for server-side siteverify and
	// the comma-separated frontend-hostname allowlist. The TURNSTILE_SECRET /
	// TURNSTILE_HOSTNAMES env vars override these fields when set.
	turnstile_secret string
	turnstile_hostnames string
pub mut:
	// Per-IP token bucket for the public read endpoints (the `[ratelimit]`
	// table in Config.toml). pub mut only so load() can clamp non-positive
	// values back to the defaults; after boot it is read-only.
	ratelimit RateLimitConfig
}

// RateLimitConfig tunes the per-IP token bucket. capacity is the burst
// tokens granted to a fresh IP, refill the tokens restored per second,
// idle_ttl the seconds a bucket may sit unused before the sweep prunes it
// (only once the map grows past sweep_above entries).
pub struct RateLimitConfig {
pub mut:
	capacity    f64 = 60.0
	refill      f64 = 20.0
	idle_ttl    int = 300
	sweep_above int = 2048
}

pub fn load() !Config {
	doc := toml.parse_file('Config.toml')!
	mut cfg := doc.decode[Config]()!
	// clamp non-positive values to the defaults — a zero capacity/refill
	// would 429 every request, and a zero TTL/sweep would churn the sweep
	clamp_rate_limit(mut cfg.ratelimit)
	return cfg
}

// clamp_rate_limit replaces non-positive fields with their defaults.
fn clamp_rate_limit(mut rl RateLimitConfig) {
	d := RateLimitConfig{}
	if rl.capacity < 1.0 {
		rl.capacity = d.capacity
	}
	if rl.refill < 0.1 {
		rl.refill = d.refill
	}
	if rl.idle_ttl < 1 {
		rl.idle_ttl = d.idle_ttl
	}
	if rl.sweep_above < 1 {
		rl.sweep_above = d.sweep_above
	}
}