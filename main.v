module main

import os
import veb
import config
import app
import database

$if sqlite_fts5 ? {
	#flag -DSQLITE_ENABLE_FTS5
}

fn main() {
	// Test session (debug builds only): fresh throwaway db + integration suite.
	$if debug ? {
		if os.getenv('CR_TEST') != '' {
			app.run_test_session()
			return
		}
	}
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
