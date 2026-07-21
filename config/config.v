module config

import toml

pub struct Config {
pub:
	host ?string
	port ?u16
}

pub fn load() !Config {
	doc := toml.parse_file('Config.toml') or { return err }
	return doc.decode[Config]()
}