module config

import toml

pub struct Config {
pub:
	host string = "127.0.0.1"
	port u16 = 6785
	db_file string = "sqlite.db"
}

pub fn load() !Config {
	doc := toml.parse_file('Config.toml')!
	return doc.decode[Config]()
}