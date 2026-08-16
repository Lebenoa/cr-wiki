module app

import log
import time
import veb

// Per-IP token bucket for the public read endpoints (search, the cookie,
// pet, treasure and build list pages, their detail pages, and the API/planner
// AJAX endpoints). Each IP gets `rate_cfg.capacity` burst tokens, refilled at
// `rate_cfg.refill` per second, and a 429 is returned while the bucket is
// empty, so heavy read traffic cannot starve the server. One bucket per IP
// covers all the endpoints together. Tuning comes from the [ratelimit] table
// of Config.toml (see config/config.v for the defaults).

// RateBucket is one client's token state. last_fill is the unix-second
// timestamp of the last refill; tokens is the current balance.
struct RateBucket {
mut:
	tokens    f64
	last_fill i64
}

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
// with a 429 status and a Retry-After header when the bucket is empty. The
// debug (test-session) build skips limiting so the HTTP suite isn't
// throttled.
pub fn (mut wapp App) rate_limit_ok(mut ctx Context) bool {
	$if debug ? {
		return true
	}
	now := time.now().unix()
	ip := client_ip(ctx)
	// the RwMutex guards the whole read-modify-write so concurrent requests
	// actually deplete the bucket — per-op auto-locking alone would let
	// parallel requests read the same balance and consume one token per wave
	mut allowed := false
	mut retry_after := 1
	wapp.rate_mu.lock()
	{
		// prune buckets idle past the TTL once the map grows; under normal
		// traffic the map stays small and no sweep ever runs
		if wapp.rate_buckets.len > wapp.rate_cfg.sweep_above {
			for k in wapp.rate_buckets.keys() {
				b := wapp.rate_buckets[k]
				if now - b.last_fill > wapp.rate_cfg.idle_ttl {
					wapp.rate_buckets.delete(k)
				}
			}
		}
		mut b := RateBucket{}
		had_key := ip in wapp.rate_buckets
		if had_key {
			b = wapp.rate_buckets[ip]
		} else {
			b = RateBucket{
				tokens:    wapp.rate_cfg.capacity
				last_fill: now
			}
		}
		// refill by wall-clock elapsed time, capped at the burst capacity
		elapsed := now - b.last_fill
		if elapsed > 0 {
			b.tokens += f64(elapsed) * wapp.rate_cfg.refill
			if b.tokens > wapp.rate_cfg.capacity {
				b.tokens = wapp.rate_cfg.capacity
			}
			b.last_fill = now
		}
		if b.tokens >= 1 {
			b.tokens -= 1
			wapp.rate_buckets[ip] = b
			allowed = true
		} else {
			wapp.rate_buckets[ip] = b
			// the next token lands within one refill tick, so advertise that;
			// the bucket may still be deep in the red when the burst was
			// large, hence the +1 guard
			retry_after = int((1.0 - b.tokens) / wapp.rate_cfg.refill) + 1
		}
	}
	wapp.rate_mu.unlock()
	if !allowed {
		log.warn('rate limit hit for ${ip}')
		ctx.res.set_status(.too_many_requests)
		ctx.set_header(.retry_after, retry_after.str())
		return false
	}
	return true
}

// rate_limited_response returns the body for a denied request (status 429
// already set by rate_limit_ok). htmx fragment swaps get an empty body so
// the infinite-scroll sentinel is removed by its outerHTML swap and the
// search/filter results just stop quietly — the bucket refills within
// seconds and a retry is harmless. Full-page and hx-boosted navigations get
// the plain text so the visitor sees why the page is blank.
pub fn rate_limited_response(mut ctx Context) veb.Result {
	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return ctx.text('')
	}
	return ctx.text('too many requests')
}
