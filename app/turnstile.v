module app

import config
import json2
import log
import net.http
import os
import time

// turnstile_sitekey is the public Cloudflare Turnstile site key for this
// site's widget. It ships in the frontend by design; only the secret stays
// server-side (TURNSTILE_SECRET env var or Config.toml's [turnstile] secret).
const turnstile_sitekey = '0x4AAAAAAERZ9YLl2pq5_2hu'

// turnstile_sitekey_for_template exposes the site key to the view templates
// so the widget divs can be rendered with the configured key.
pub fn (ctx &Context) turnstile_sitekey_for_template() string {
	return turnstile_sitekey
}

// SiteVerifyResponse is the subset of Cloudflare's siteverify response that
// the handler enforces on.
struct SiteVerifyResponse {
	success  bool
	action   string
	hostname string
}

// turnstile_secret returns the siteverify secret: the TURNSTILE_SECRET env
// var wins, falling back to the secret field of Config.toml's [turnstile]
// table (via the config module). Empty when neither is configured.
fn turnstile_secret() string {
	s := os.getenv('TURNSTILE_SECRET')
	if s != '' {
		return s
	}
	cfg := config.load() or { return '' }
	return cfg.turnstile.secret
}

// turnstile_hostnames_raw returns the raw comma-separated hostname allowlist:
// the TURNSTILE_HOSTNAMES env var wins, falling back to the hostnames field
// of Config.toml's [turnstile] table.
fn turnstile_hostnames_raw() string {
	s := os.getenv('TURNSTILE_HOSTNAMES')
	if s != '' {
		return s
	}
	cfg := config.load() or { return '' }
	return cfg.turnstile.hostnames
}

// turnstile_hostnames returns the frontend-hostname allowlist (comma-
// separated, from env or Config.toml). Local hosts are the default so
// development works without configuration; production must set the real
// deployment hostname or every protected submission is rejected (fail
// closed).
pub fn turnstile_hostnames() []string {
	mut hostnames := []string{}
	for raw in turnstile_hostnames_raw().split(',') {
		h := raw.trim_space()
		if h != '' {
			hostnames << h
		}
	}
	if hostnames.len == 0 {
		return ['localhost', '127.0.0.1']
	}
	return hostnames
}

// verify_turnstile validates the cf-turnstile-response token from the form
// against Cloudflare's siteverify endpoint: the response must succeed, the
// action must match the protected surface and the frontend hostname must be
// in the allowlist. It fails closed on missing config or any network/parse
// error. The debug (test-session) build skips verification — the suite posts
// directly to protected handlers and cannot mint real browser tokens.
pub fn verify_turnstile(ctx &Context, expected_action string) bool {
	$if debug ? {
		return true
	}
	secret := turnstile_secret()
	if secret == '' {
		log.warn('turnstile: no TURNSTILE_SECRET / turnstile.secret configured, rejecting submission')
		return false
	}
	token := ctx.form['cf-turnstile-response'] or { return false }
	if token.len == 0 || token.len > 2048 {
		log.warn('turnstile: missing or oversized cf-turnstile-response')
		return false
	}
	hostnames := turnstile_hostnames()
	if hostnames.len == 0 {
		return false
	}
	resp := http.fetch(
		method:       .post
		url:          'https://challenges.cloudflare.com/turnstile/v0/siteverify'
		header:       http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:         http.url_encode_form_data({
			'secret':   secret
			'response': token
		})
		read_timeout: 10 * time.second
	) or {
		log.warn('turnstile: siteverify request failed: ${err}')
		return false
	}
	if resp.status_code != 200 {
		log.warn('turnstile: siteverify returned HTTP ${resp.status_code}')
		return false
	}
	parsed := json2.decode[SiteVerifyResponse](resp.body) or {
		log.warn('turnstile: siteverify returned an unparsable body: ${err}')
		return false
	}
	if !parsed.success || parsed.action != expected_action || parsed.hostname !in hostnames {
		log.warn('turnstile: rejected (success=${parsed.success} action=${parsed.action} hostname=${parsed.hostname})')
		return false
	}
	return true
}
