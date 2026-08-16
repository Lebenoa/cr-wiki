module app

import log
import time

// Per-IP token bucket for the public read endpoints (search plus the cookie,
// pet, treasure and build list pages). Each IP gets `rate_capacity` burst
// tokens, refilled at `rate_refill` per second, and a 429 is returned while
// the bucket is empty, so heavy read traffic cannot starve the server. One
// bucket per IP covers all five endpoints together.
const rate_capacity = 60.0 // burst tokens
const rate_refill = 20.0 // tokens refilled per second
const rate_idle_ttl = 300 // seconds without a request before a bucket is pruned
const rate_sweep_above = 2048 // prune only once the bucket map grows past this

// RateBucket is one client's token state. last_fill is the unix-second
// timestamp of the last refill; tokens is the current balance.
struct RateBucket {
mut:
	tokens    f64
	last_fill i64
}

__global (
	rate_buckets shared map[string]RateBucket
)

// client_ip resolves the requester's IP through veb's own detection: the
// trusted proxy headers (CF-Connecting-IP, X-Forwarded-For, X-Real-Ip) first,
// then the TCP peer address. Requests without any address share the
// 'unknown' bucket.
fn client_ip(ctx &Context) string {
	ip := ctx.ip()
	return if ip == '' { 'unknown' } else { ip }
}

// rate_limit_ok applies the per-IP token bucket to the current request.
// Returns true when the bucket had a token (the request proceeds); false
// with a 429 status when the bucket is empty. The debug (test-session)
// build skips limiting so the HTTP suite isn't throttled.
pub fn rate_limit_ok(mut ctx Context) bool {
	$if debug ? {
		return true
	}
	now := time.now().unix()
	ip := client_ip(ctx)
	// the whole read-modify-write runs under the shared lock so concurrent
	// requests actually deplete the bucket — per-op auto-locking alone would
	// let parallel requests read the same balance and consume one token per
	// wave
	mut allowed := false
	lock rate_buckets {
		// prune buckets idle past the TTL once the map grows; under normal
		// traffic the map stays small and no sweep ever runs
		if rate_buckets.len > rate_sweep_above {
			for k in rate_buckets.keys() {
				b := rate_buckets[k]
				if now - b.last_fill > rate_idle_ttl {
					rate_buckets.delete(k)
				}
			}
		}
		mut b := RateBucket{}
		had_key := ip in rate_buckets
		if had_key {
			b = rate_buckets[ip]
		} else {
			b = RateBucket{
				tokens:    rate_capacity
				last_fill: now
			}
		}
		// refill by wall-clock elapsed time, capped at the burst capacity
		elapsed := now - b.last_fill
		if elapsed > 0 {
			b.tokens += f64(elapsed) * rate_refill
			if b.tokens > rate_capacity {
				b.tokens = rate_capacity
			}
			b.last_fill = now
		}
		if b.tokens >= 1 {
			b.tokens -= 1
			rate_buckets[ip] = b
			allowed = true
		} else {
			rate_buckets[ip] = b
		}
	}
	if !allowed {
		log.warn('rate limit hit for ${ip}')
		ctx.res.set_status(.too_many_requests)
		return false
	}
	return true
}
