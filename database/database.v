module database

import db.sqlite
import strings
import models

pub fn initialize(path string) !sqlite.DB {
	conn := sqlite.connect(path)!

	sql conn {
		create table models.User
		create table models.Pet
		create table models.PetTranslation
		create table models.Cookie
		create table models.CookieTranslation
		create table models.CombiBonus
		create table models.Treasure
		create table models.TreasureTranslation
		create table models.Effect
		create table models.EffectTranslation
		create table models.TreasureEffect
	}!

	$if sqlite_fts5 ? {
		conn.exec("PRAGMA journal_mode = WAL; PRAGMA synchoronous = FULL;")!
		create_fts(conn)
	}
	return conn
}
struct FtsTable {
	table   string
	columns []string
}

const fts_tables = [
	FtsTable{
		table: 'cookie_translation'
		columns: ['name', 'abilities', 'description', 'lang']
	},
	FtsTable{
		table: 'pet_translation'
		columns: ['name', 'description', 'lang']
	},
	FtsTable{
		table: 'treasure_translation'
		columns: ['name', 'description', 'lang']
	},
	FtsTable{
		table: 'effect_translation'
		columns: ['name', 'description', 'lang']
	},
]

fn create_fts(conn sqlite.DB) {
	for table in fts_tables {
		create_fts_table(conn, table) or { panic(err) }
	}
}

fn create_fts_table(conn sqlite.DB, table FtsTable) ! {
	cols := table.columns.join(', ')

	mut insert_cols := strings.new_builder(256)
	mut new_cols := strings.new_builder(256)
	mut old_cols := strings.new_builder(256)

	for i, col in table.columns {
		if i > 0 {
			insert_cols.write_string(', ')
			new_cols.write_string(', ')
			old_cols.write_string(', ')
		}

		insert_cols.write_string(col)
		new_cols.write_string('new.${col}')
		old_cols.write_string('old.${col}')
	}

	insert_cols_str := insert_cols.str()
	new_cols_str := new_cols.str()
	old_cols_str := old_cols.str()

	exec(conn, "
		CREATE VIRTUAL TABLE IF NOT EXISTS ${table.table}_fts
		USING fts5(
			${cols},
			content='${table.table}',
			content_rowid='${table.table}_id'
		);
	")!

	exec(conn, "
		CREATE TRIGGER IF NOT EXISTS ${table.table}_ai
		AFTER INSERT ON ${table.table}
		BEGIN
			INSERT INTO ${table.table}_fts(
				rowid,
				${insert_cols_str}
			)
			VALUES (
				new.${table.table}_id,
				${new_cols_str}
			);
		END;
	")!

	exec(conn, "
		CREATE TRIGGER IF NOT EXISTS ${table.table}_ad
		AFTER DELETE ON ${table.table}
		BEGIN
			INSERT INTO ${table.table}_fts(
				${table.table}_fts,
				rowid,
				${insert_cols_str}
			)
			VALUES (
				'delete',
				old.${table.table}_id,
				${old_cols_str}
			);
		END;
	")!

	exec(conn, "
		CREATE TRIGGER IF NOT EXISTS ${table.table}_au
		AFTER UPDATE ON ${table.table}
		BEGIN
			INSERT INTO ${table.table}_fts(
				${table.table}_fts,
				rowid,
				${insert_cols_str}
			)
			VALUES (
				'delete',
				old.${table.table}_id,
				${old_cols_str}
			);

			INSERT INTO ${table.table}_fts(
				rowid,
				${insert_cols_str}
			)
			VALUES (
				new.${table.table}_id,
				${new_cols_str}
			);
		END;
	")!

	exec(conn, "
		INSERT INTO ${table.table}_fts(
			rowid,
			${insert_cols_str}
		)
		SELECT
			${table.table}_id,
			${insert_cols_str}
		FROM ${table.table}
		WHERE NOT EXISTS (
			SELECT 1
			FROM ${table.table}_fts
			LIMIT 1
		);
	")!
}

@[inline]
fn exec(conn sqlite.DB, nq string) ! {
	query := nq.trim_space()
	println("Executing: `${query}`")
	result := conn.exec_none(query)
	if !sqlite_success(result) {
		return conn.error_message(result, query)
	}
}

@[inline]
fn sqlite_success(code int) bool {
	return code == sqlite.sqlite_ok
		|| code == sqlite.sqlite_done
}
