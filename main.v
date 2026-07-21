module main

import veb
import config
import app

fn main() {
	loaded_config := config.load() or { 
		println("Failed to load `Config.toml`; using default config; ${err}")
		config.Config{}
	}
	mut wapp := app.initialize()!
	veb.run_at[app.App, app.Context](mut wapp, 
		family: .ip
		host: loaded_config.host or { '127.0.0.1' }
		port: loaded_config.port or { 6785 }
	)!
}
