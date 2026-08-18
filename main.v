module main

import os
import log
import veb
import config
import app
import database

$if sqlite_fts5 ? {
	#flag -DSQLITE_ENABLE_FTS5
}

fn main() {
	// Install a thread-safe logger writing to the terminal AND a file, so
	// warnings (e.g. corrupt rows in database/select.v) show up in the watch
	// console and in a persistent log.
	setup_logging()

	// Test session (non-production builds only): fresh throwaway db +
	// integration suite.
	$if !prod {
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

// setup_logging writes log lines to both the terminal and a file (default
// `logs/cookierun.log`, override with CR_LOG_FILE). The log module's default
// instance is thread safe, which the multi-threaded veb server needs.
fn setup_logging() {
	mut log_file := os.getenv('CR_LOG_FILE')
	if log_file == '' {
		log_file = 'logs/cookierun.log'
	}
	os.mkdir_all(os.dir(log_file)) or {}
	mut l := log.new_thread_safe_log()
	l.set_level(.debug)
	l.set_local_time(true)
	l.set_time_format(.tf_ss_milli)
	// flush after every line — the default only flushes on process exit, so a
	// long-running server's log file would otherwise look frozen while running
	l.set_always_flush(true)
	l.set_full_logpath(log_file)
	l.log_to_console_too()
	log.set_logger(l)
	log.info('logging to ${log_file} (terminal + file)')
}
