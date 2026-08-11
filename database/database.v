module database

import db.sqlite
import strings
import os
import json2
import models

$if sqlite_fts5 ? {
	#flag -DSQLITE_ENABLE_FTS5
}

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

	migrate(conn)!

	$if sqlite_fts5 ? {
		conn.exec("PRAGMA journal_mode = WAL; PRAGMA synchoronous = FULL;")!
		create_fts(conn)
	}

	// Fresh checkouts have no data (sqlite.db is gitignored); bootstrap the
	// roster from the committed fixture so the site works out of the box.
	seed_if_empty(conn)!
	return conn
}

// SeedFixture mirrors the committed scripts/seed_data.json dump: the roster
// tables only. Users are never seeded (auth data stays local).
pub struct SeedFixture {
	cookie               []models.Cookie
	cookie_translation   []models.CookieTranslation
	pet                  []models.Pet
	pet_translation      []models.PetTranslation
	treasure             []models.Treasure
	treasure_translation []models.TreasureTranslation
	effect               []models.Effect
	effect_translation   []models.EffectTranslation
	treasure_effect      []models.TreasureEffect
}

// seed_if_empty loads scripts/seed_data.json into a database that has no
// cookies yet. It is a no-op for existing databases and for checkouts without
// the fixture. Inserts go through the ORM, so FTS triggers populate the index.
fn seed_if_empty(conn sqlite.DB) ! {
	rows := conn.exec('SELECT COUNT(*) AS n FROM cookie') or { return }
	if rows.len > 0 && rows[0].get_int('n') > 0 {
		return
	}
	path := 'scripts/seed_data.json'
	if !os.exists(path) {
		return
	}
	fixture := json2.decode[SeedFixture](os.read_file(path)!)!
	for c in fixture.cookie {
		sql conn {
			insert c into models.Cookie
		}!
	}
	for tr in fixture.cookie_translation {
		sql conn {
			insert tr into models.CookieTranslation
		}!
	}
	for p in fixture.pet {
		sql conn {
			insert p into models.Pet
		}!
	}
	for tr in fixture.pet_translation {
		sql conn {
			insert tr into models.PetTranslation
		}!
	}
	// treasure/effect rows in FK order; explicit ids preserved so the
	// treasure_effect links stay valid
	for t in fixture.treasure {
		sql conn {
			insert t into models.Treasure
		}!
	}
	for tr in fixture.treasure_translation {
		sql conn {
			insert tr into models.TreasureTranslation
		}!
	}
	for e in fixture.effect {
		sql conn {
			insert e into models.Effect
		}!
	}
	for tr in fixture.effect_translation {
		sql conn {
			insert tr into models.EffectTranslation
		}!
	}
	for te in fixture.treasure_effect {
		sql conn {
			insert te into models.TreasureEffect
		}!
	}
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

// migrate applies schema changes that `create table` cannot handle on existing databases.
fn migrate(conn sqlite.DB) ! {
	// cookie_translation columns added after the table first shipped; ensure they
	// exist for databases created before their introduction.
	translation_cols := conn.columns('cookie_translation') or { return }
	for col in ['power_plus', 'power_plus_requirement', 'unlock_goal'] {
		if col !in translation_cols {
			query := 'ALTER TABLE cookie_translation ADD COLUMN ${col} TEXT NOT NULL DEFAULT ""'
			result := conn.exec_none(query)
			if !sqlite_success(result) {
				return conn.error_message(result, query)
			}
		}
	}

	// treasure_effect.sort_order existed in an early model revision and was
	// later removed; the NOT NULL column blocks inserts from the current model.
	treasure_effect_cols := conn.columns('treasure_effect') or { return }
	if 'sort_order' in treasure_effect_cols {
		query := 'ALTER TABLE treasure_effect DROP COLUMN sort_order'
		result := conn.exec_none(query)
		if !sqlite_success(result) {
			return conn.error_message(result, query)
		}
	}

	// treasure.grade added after the table shipped; nullable so special
	// treasures without a wiki grade stay badge-less
	treasure_cols := conn.columns('treasure') or { return }
	if 'grade' !in treasure_cols {
		query := 'ALTER TABLE treasure ADD COLUMN grade INTEGER'
		result := conn.exec_none(query)
		if !sqlite_success(result) {
			return conn.error_message(result, query)
		}
	}
	// treasure.base_treasure_id added so evolved rows link back to their
	// normal base treasure; nullable because normal treasures have none
	if 'base_treasure_id' !in treasure_cols {
		query := 'ALTER TABLE treasure ADD COLUMN base_treasure_id INTEGER'
		result := conn.exec_none(query)
		if !sqlite_success(result) {
			return conn.error_message(result, query)
		}
	}
	// treasure.unlock_cookie_id / unlock_pet_id added so treasures unlocked by
	// upgrading a cookie or pet to max level can link back to it; nullable
	// because most treasures come from chests or events instead
	if 'unlock_cookie_id' !in treasure_cols {
		query := 'ALTER TABLE treasure ADD COLUMN unlock_cookie_id INTEGER'
		result := conn.exec_none(query)
		if !sqlite_success(result) {
			return conn.error_message(result, query)
		}
	}
	if 'unlock_pet_id' !in treasure_cols {
		query := 'ALTER TABLE treasure ADD COLUMN unlock_pet_id INTEGER'
		result := conn.exec_none(query)
		if !sqlite_success(result) {
			return conn.error_message(result, query)
		}
	}

	// merge blessed states into their evolved row: move the blessed effects to
	// treasure_blessed_effect (created by the ORM) keyed by the sibling
	// normal-state evolved row, drop the blessed treasure rows, then remove the
	// is_blessed column. Each step is idempotent so a partially-applied
	// migration recovers on reboot.
	current_cols := conn.columns('treasure') or { return }
	if 'is_blessed' in current_cols {
		// this step targets treasure_blessed_effect, which the ORM used to
		// create from its model; the model is gone (the tables were collapsed
		// below), so create the transient table here for this migration only
		mut result := conn.exec_none("
			CREATE TABLE IF NOT EXISTS treasure_blessed_effect (
				treasure_blessed_effect_id INTEGER PRIMARY KEY AUTOINCREMENT,
				treasure_id INTEGER NOT NULL,
				effect_id INTEGER NOT NULL,
				value REAL,
				unit INTEGER NOT NULL,
				UNIQUE(treasure_id, effect_id)
			)
		")
		if !sqlite_success(result) {
			return conn.error_message(result, 'create transient treasure_blessed_effect')
		}
		result = conn.exec_none("
			INSERT OR IGNORE INTO treasure_blessed_effect (treasure_id, effect_id, value, unit)
			SELECT e2.treasure_id, te.effect_id, te.value, te.unit
			FROM treasure t1
			JOIN treasure_effect te ON te.treasure_id = t1.treasure_id
			JOIN treasure e2 ON e2.is_evolved = 1 AND e2.is_blessed = 0
				AND e2.base_treasure_id = t1.base_treasure_id
			WHERE t1.is_blessed = 1
		")
		if !sqlite_success(result) {
			return conn.error_message(result, 'merge treasure blessed effects')
		}
		// translations reference treasure, so drop the orphaned ones first
		result = conn.exec_none('DELETE FROM treasure_translation WHERE treasure_id IN (SELECT treasure_id FROM treasure WHERE is_blessed = 1)')
		if !sqlite_success(result) {
			return conn.error_message(result, 'delete blessed treasure translations')
		}
		result = conn.exec_none('DELETE FROM treasure WHERE is_blessed = 1')
		if !sqlite_success(result) {
			return conn.error_message(result, 'delete blessed treasure rows')
		}
		result = conn.exec_none('ALTER TABLE treasure DROP COLUMN is_blessed')
		if !sqlite_success(result) {
			return conn.error_message(result, 'drop treasure is_blessed column')
		}
	}

	// treasure effects were split across treasure_effect (normal state) and
	// treasure_blessed_effect (blessed state); collapse to one table with a
	// state column. The unique key gains `state` (a treasure can list the
	// same effect in both states), so the table is rebuilt rather than
	// altered. All steps run in one transaction, so a partial failure rolls
	// back and the migration retries cleanly on the next boot.
	if 'state' !in treasure_effect_cols {
		conn.begin()!
		mut result := conn.exec_none('ALTER TABLE treasure_effect RENAME TO treasure_effect_old')
		if !sqlite_success(result) {
			return conn.error_message(result, 'rename treasure_effect')
		}
		// the ORM creates the fresh table from the current model (state
		// column + the three-column unique key)
		sql conn {
			create table models.TreasureEffect
		}!
		// only keep effects whose treasure still exists: the blessed-state
		// merge deleted the blessed treasure rows, orphaning their original
		// effect rows here (the same effects live on in the blessed state)
		// ORDER BY preserves the wiki listing order: the new serial ids must
		// follow the old ones so display stays in the order effects were
		// scraped (INSERT..SELECT without ORDER BY is not guaranteed ordered)
		result = conn.exec_none("
			INSERT INTO treasure_effect (treasure_id, effect_id, value, unit, state)
			SELECT treasure_id, effect_id, value, unit, 0
			FROM treasure_effect_old
			WHERE treasure_id IN (SELECT treasure_id FROM treasure)
			ORDER BY treasure_effect_id
		")
		if !sqlite_success(result) {
			return conn.error_message(result, 'copy normal treasure effects')
		}
		if _ := conn.columns('treasure_blessed_effect') {
			result = conn.exec_none("
				INSERT INTO treasure_effect (treasure_id, effect_id, value, unit, state)
				SELECT treasure_id, effect_id, value, unit, 1
				FROM treasure_blessed_effect
				ORDER BY treasure_blessed_effect_id
			")
			if !sqlite_success(result) {
				return conn.error_message(result, 'copy blessed treasure effects')
			}
			result = conn.exec_none('DROP TABLE treasure_blessed_effect')
			if !sqlite_success(result) {
				return conn.error_message(result, 'drop treasure_blessed_effect')
			}
		}
		result = conn.exec_none('DROP TABLE treasure_effect_old')
		if !sqlite_success(result) {
			return conn.error_message(result, 'drop treasure_effect_old')
		}
		conn.commit()!
	}
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
