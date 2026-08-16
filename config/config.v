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
}

pub fn load() !Config {
	doc := toml.parse_file('Config.toml')!
	return doc.decode[Config]()
}