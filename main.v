module main

import veb
import config
import app
import database

fn main() {
	loaded_config := config.load() or { 
		println("Failed to load `Config.toml`; using default config; ${err}")
		config.Config{}
	}
	db := database.initialize(loaded_config.db_file)!
	mut wapp := app.initialize(db)!
	veb.run_at[app.App, app.Context](mut wapp, 
		family: .ip
		host: loaded_config.host
		port: loaded_config.port
	)!
}
