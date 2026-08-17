module database

import log
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
		create table models.Build
		create table models.BuildReview
		// cookierundb catalog tables (episode first: relic/quest/stage rows
		// reference it; treasure/pet/cookie already exist above).
		create table models.Episode
		create table models.EpisodeTranslation
		create table models.EpisodeStage
		create table models.Relic
		create table models.RelicTranslation
		create table models.EpisodeRelic
		create table models.EpisodeDrawReward
		create table models.EpisodeBoxOdds
		create table models.Quest
		create table models.Ingredient
		create table models.IngredientTranslation
		create table models.IngredientRecipe
		create table models.Jelly
		create table models.JellyTranslation
		create table models.JellyMaker
		create table models.Skin
		create table models.SkinTranslation
		create table models.TreasureLevel
		create table models.GachaPool
		create table models.GachaPoolEntry
	}!

	migrate(conn)!
	create_indexes(conn)!

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
	treasure_level       []models.TreasureLevel
	combi_bonus          []models.CombiBonus
	relic                []models.Relic
	relic_translation    []models.RelicTranslation
	episode              []models.Episode
	episode_translation  []models.EpisodeTranslation
	episode_stage        []models.EpisodeStage
	episode_relic        []models.EpisodeRelic
	episode_draw_reward  []models.EpisodeDrawReward
	episode_box_odds     []models.EpisodeBoxOdds
	quest                []models.Quest
	ingredient           []models.Ingredient
	ingredient_translation []models.IngredientTranslation
	jelly                []models.Jelly
	jelly_translation    []models.JellyTranslation
	jelly_maker          []models.JellyMaker
	skin                 []models.Skin
	skin_translation     []models.SkinTranslation
	gacha_pool           []models.GachaPool
	gacha_pool_entry     []models.GachaPoolEntry
	build                []models.Build
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
	// One transaction around the whole fixture: with per-statement autocommit
	// the ~5800 ORM inserts plus their FTS triggers turn a fresh seed into a
	// multi-minute slog (and make the test session unusable). A single commit
	// brings it back to a couple of seconds.
	conn.exec('BEGIN')!
	insert_fixture(conn, fixture) or {
		conn.exec('ROLLBACK')!
		return err
	}
	conn.exec('COMMIT')!
}

// insert_fixture inserts every roster row from the fixture in FK order;
// explicit ids are preserved so the treasure_effect links stay valid.
fn insert_fixture(conn sqlite.DB, fixture SeedFixture) ! {
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
	for tl in fixture.treasure_level {
		sql conn {
			insert tl into models.TreasureLevel
		}!
	}
	for cb in fixture.combi_bonus {
		sql conn {
			insert cb into models.CombiBonus
		}!
	}
	// Phase A catalog tables, in FK order (episode before the tables that
	// reference it; relic after episode and cookie; ingredient after episode).
	for e in fixture.episode {
		sql conn {
			insert e into models.Episode
		}!
	}
	for tr in fixture.episode_translation {
		sql conn {
			insert tr into models.EpisodeTranslation
		}!
	}
	for s in fixture.episode_stage {
		sql conn {
			insert s into models.EpisodeStage
		}!
	}
	for r in fixture.relic {
		sql conn {
			insert r into models.Relic
		}!
	}
	for tr in fixture.relic_translation {
		sql conn {
			insert tr into models.RelicTranslation
		}!
	}
	for er in fixture.episode_relic {
		sql conn {
			insert er into models.EpisodeRelic
		}!
	}
	for dr in fixture.episode_draw_reward {
		sql conn {
			insert dr into models.EpisodeDrawReward
		}!
	}
	for bo in fixture.episode_box_odds {
		sql conn {
			insert bo into models.EpisodeBoxOdds
		}!
	}
	for q in fixture.quest {
		sql conn {
			insert q into models.Quest
		}!
	}
	for i in fixture.ingredient {
		sql conn {
			insert i into models.Ingredient
		}!
	}
	for tr in fixture.ingredient_translation {
		sql conn {
			insert tr into models.IngredientTranslation
		}!
	}
	for j in fixture.jelly {
		sql conn {
			insert j into models.Jelly
		}!
	}
	for tr in fixture.jelly_translation {
		sql conn {
			insert tr into models.JellyTranslation
		}!
	}
	for jm in fixture.jelly_maker {
		sql conn {
			insert jm into models.JellyMaker
		}!
	}
	for s in fixture.skin {
		sql conn {
			insert s into models.Skin
		}!
	}
	for tr in fixture.skin_translation {
		sql conn {
			insert tr into models.SkinTranslation
		}!
	}
	for p in fixture.gacha_pool {
		sql conn {
			insert p into models.GachaPool
		}!
	}
	for e in fixture.gacha_pool_entry {
		sql conn {
			insert e into models.GachaPoolEntry
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
	FtsTable{
		table: 'relic_translation'
		columns: ['name', 'description', 'lang']
	},
	FtsTable{
		table: 'episode_translation'
		columns: ['name', 'description', 'lang']
	},
	FtsTable{
		table: 'ingredient_translation'
		columns: ['name', 'description', 'lang']
	},
	FtsTable{
		table: 'quest'
		// group is a reserved word in SQL (new.group is a syntax error); the
		// FTS column keeps the real name and the generated trigger/backfill
		// SQL quotes it. See create_fts_table.
		columns: ['name', 'requirement', 'reward', 'group']
	},
]

fn create_fts(conn sqlite.DB) {
	for table in fts_tables {
		create_fts_table(conn, table) or { panic(err) }
	}
}

fn create_fts_table(conn sqlite.DB, table FtsTable) ! {
	cols := table.columns.join(', ')

	// Reserved words (quest.group) are quoted with a safe identifier
	// ("group"); everything else stays plain. Quoting is applied in the
	// trigger bodies and the backfill SELECT, where the column is referenced
	// as new.col / old.col / bare col — the FTS CREATE keeps the bare names.
	mut insert_cols := strings.new_builder(256)
	mut new_cols := strings.new_builder(256)
	mut old_cols := strings.new_builder(256)

	for i, col in table.columns {
		if i > 0 {
			insert_cols.write_string(', ')
			new_cols.write_string(', ')
			old_cols.write_string(', ')
		}

		qcol := if col == 'group' { '"group"' } else { col }
		insert_cols.write_string(qcol)
		new_cols.write_string('new.' + qcol)
		old_cols.write_string('old.' + qcol)
	}

	insert_cols_str := insert_cols.str()
	new_cols_str := new_cols.str()
	old_cols_str := old_cols.str()
	// backfill SELECT mirrors insert_cols: quoted names survive reserved words
	backfill_cols_str := insert_cols_str

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
			${backfill_cols_str}
		FROM ${table.table};
	")!
}

// create_indexes materializes the secondary indexes the ORM's `create table`
// never creates: V's sqlite ORM emits only the UNIQUE constraint auto-indexes
// (one per table), so every WHERE on a non-PK column is a full table scan
// until these exist. IF NOT EXISTS keeps this idempotent for both fresh and
// pre-existing databases; run once at initialize.
fn create_indexes(conn sqlite.DB) ! {
	// The ORM's UNIQUE(owner_id, lang) auto-index already serves cookie/pet/
	// treasure translation lookups by owner and treasure_effect lookups by
	// treasure (leftmost column of UNIQUE(treasure_id, effect_id, state)), so
	// only tables with no covering constraint need explicit indexes.
	indexes := [
		// combi_bonus: cookie+pet pair lookup on build insert/update, and the
		// cookie/pet detail-page combo sections (cookie_id prefix).
		'CREATE INDEX IF NOT EXISTS idx_combi_bonus_cookie_pet ON combi_bonus (cookie_id, pet_id)',
		'CREATE INDEX IF NOT EXISTS idx_combi_bonus_pet ON combi_bonus (pet_id)',
		// treasure: evolved/base links and the cookie/pet max-level unlocks.
		'CREATE INDEX IF NOT EXISTS idx_treasure_base ON treasure (base_treasure_id)',
		'CREATE INDEX IF NOT EXISTS idx_treasure_unlock_cookie ON treasure (unlock_cookie_id)',
		'CREATE INDEX IF NOT EXISTS idx_treasure_unlock_pet ON treasure (unlock_pet_id)',
		// build: the /builds list filters (cookie/pet/EP tiers, and the
		// treasure OR filter) — the model's @[index] annotations were never
		// materialized by the ORM. The three treasure indexes turn the
		// treasure1/2/3 OR filter into a MULTI-INDEX OR instead of a scan.
		'CREATE INDEX IF NOT EXISTS idx_build_cookie ON build (cookie_id)',
		'CREATE INDEX IF NOT EXISTS idx_build_cookie2 ON build (cookie2_id)',
		'CREATE INDEX IF NOT EXISTS idx_build_pet ON build (pet_id)',
		'CREATE INDEX IF NOT EXISTS idx_build_ep ON build (ep)',
		'CREATE INDEX IF NOT EXISTS idx_build_ep_special ON build (ep_special)',
		'CREATE INDEX IF NOT EXISTS idx_build_treasure1_id ON build (treasure1_id)',
		'CREATE INDEX IF NOT EXISTS idx_build_treasure2_id ON build (treasure2_id)',
		'CREATE INDEX IF NOT EXISTS idx_build_treasure3_id ON build (treasure3_id)',
		// cookierundb catalog tables: every FK WHERE below (the ORM never
		// materializes @[index] annotations — only UNIQUE auto-indexes).
		'CREATE INDEX IF NOT EXISTS idx_relic_episode ON relic (episode_id)',
		'CREATE INDEX IF NOT EXISTS idx_relic_unlock_cookie ON relic (unlock_cookie_id)',
		'CREATE INDEX IF NOT EXISTS idx_ingredient_drop_episode ON ingredient (drop_episode_id)',
		'CREATE INDEX IF NOT EXISTS idx_ingredient_recipe_treasure ON ingredient_recipe (treasure_id)',
		'CREATE INDEX IF NOT EXISTS idx_episode_relic_relic ON episode_relic (relic_id)',
		'CREATE INDEX IF NOT EXISTS idx_episode_draw_reward ON episode_draw_reward (episode_id)',
		'CREATE INDEX IF NOT EXISTS idx_episode_box_odds ON episode_box_odds (episode_id)',
		'CREATE INDEX IF NOT EXISTS idx_quest_episode ON quest (episode_id)',
		'CREATE INDEX IF NOT EXISTS idx_jelly_maker_entity ON jelly_maker (entity_kind, entity_id)',
		'CREATE INDEX IF NOT EXISTS idx_skin_cookie ON skin (cookie_id)',
		'CREATE INDEX IF NOT EXISTS idx_gacha_pool_entry_pool ON gacha_pool_entry (pool_id)',
		'CREATE INDEX IF NOT EXISTS idx_gacha_pool_entry_treasure ON gacha_pool_entry (treasure_id)',
		'CREATE INDEX IF NOT EXISTS idx_gacha_pool_entry_pet ON gacha_pool_entry (pet_id)',
	]
	for stmt in indexes {
		result := conn.exec_none(stmt)
		if !sqlite_success(result) {
			return conn.error_message(result, 'create index')
		}
	}
}

// migrate applies schema changes that `create table` cannot handle on existing databases.
fn migrate(conn sqlite.DB) ! {
	// build.ep became a tiered combobox (EP 1-7, Special EP 1-3) and builds
	// gained a tag (#score/#coin/#autofarm); add the columns for databases
	// created before the change. Old rows keep their raw ep value and empty
	// tag until re-submitted.
	build_cols := conn.columns('build') or { return }
	if 'ep_special' !in build_cols {
		result := conn.exec_none('ALTER TABLE build ADD COLUMN ep_special INTEGER NOT NULL DEFAULT 0')
		if !sqlite_success(result) {
			return conn.error_message(result, 'add build ep_special column')
		}
	}
	if 'tag' !in build_cols {
		result := conn.exec_none("ALTER TABLE build ADD COLUMN tag TEXT NOT NULL DEFAULT ''")
		if !sqlite_success(result) {
			return conn.error_message(result, 'add build tag column')
		}
	}
	for col in ['description', 'youtube_url'] {
		if col !in build_cols {
			result := conn.exec_none("ALTER TABLE build ADD COLUMN ${col} TEXT NOT NULL DEFAULT ''")
			if !sqlite_success(result) {
				return conn.error_message(result, 'add build ${col} column')
			}
		}
	}
	// run-result stats (score/coin/time/boxes) added later; existing rows keep 0.
	for col in ['score', 'coin', 'time', 'boxes'] {
		if col !in build_cols {
			result := conn.exec_none('ALTER TABLE build ADD COLUMN ${col} INTEGER NOT NULL DEFAULT 0')
			if !sqlite_success(result) {
				return conn.error_message(result, 'add build ${col} column')
			}
		}
	}
	if 'cookie2_id' !in build_cols {
		result := conn.exec_none('ALTER TABLE build ADD COLUMN cookie2_id INTEGER NOT NULL DEFAULT 0')
		if !sqlite_success(result) {
			return conn.error_message(result, 'add build cookie2_id column')
		}
	}
	if 'boosts' !in build_cols {
		result := conn.exec_none("ALTER TABLE build ADD COLUMN boosts TEXT NOT NULL DEFAULT ''")
		if !sqlite_success(result) {
			return conn.error_message(result, 'add build boosts column')
		}
	}
	// the purchased pre-run boost (one key) and the owned Power+ effect keys;
	// old rows keep the empty defaults until re-submitted.
	for col in ['boost', 'power_effects'] {
		if col !in build_cols {
			result := conn.exec_none("ALTER TABLE build ADD COLUMN ${col} TEXT NOT NULL DEFAULT ''")
			if !sqlite_success(result) {
				return conn.error_message(result, 'add build ${col} column')
			}
		}
	}
	// per-slot treasure blessed state (1 = blessed), added later; old rows default to normal.
	for col in ['treasure1_blessed', 'treasure2_blessed', 'treasure3_blessed'] {
		if col !in build_cols {
			result := conn.exec_none('ALTER TABLE build ADD COLUMN ${col} INTEGER NOT NULL DEFAULT 0')
			if !sqlite_success(result) {
				return conn.error_message(result, 'add build ${col} column')
			}
		}
	}
	// per-slot treasure equipped level (0-9), added later; old rows default to max.
	for col in ['treasure1_level', 'treasure2_level', 'treasure3_level'] {
		if col !in build_cols {
			result := conn.exec_none('ALTER TABLE build ADD COLUMN ${col} INTEGER NOT NULL DEFAULT 9')
			if !sqlite_success(result) {
				return conn.error_message(result, 'add build ${col} column')
			}
		}
	}
	// build.combi_bonus_id snapshots the combi_bonus row for the cookie+pet
	// pair (none when the pair has no combo); added later, old rows stay null.
	if 'combi_bonus_id' !in build_cols {
		result := conn.exec_none('ALTER TABLE build ADD COLUMN combi_bonus_id INTEGER')
		if !sqlite_success(result) {
			return conn.error_message(result, 'add build combi_bonus_id column')
		}
	}

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
	// treasure.is_power_plus marks POWER+ treasures (friendly-run bonus items
	// that cannot be equipped); they are excluded from the build pickers.
	if 'is_power_plus' !in treasure_cols {
		query := 'ALTER TABLE treasure ADD COLUMN is_power_plus INTEGER NOT NULL DEFAULT 0'
		result := conn.exec_none(query)
		if !sqlite_success(result) {
			return conn.error_message(result, query)
		}
	}

	// combi_bonus.effect text moved to a reference into the shared effect
	// table (effect_id); the effect text was English, so find-or-create an
	// effect row by the English name and link it. Idempotent: once the
	// column is gone the guards skip. The effect_id column is added first
	// (the model now carries it) so the link UPDATE works on existing dbs.
	combi_cols := conn.columns('combi_bonus') or { return }
	if 'effect_id' !in combi_cols {
		result := conn.exec_none('ALTER TABLE combi_bonus ADD COLUMN effect_id INTEGER')
		if !sqlite_success(result) {
			return conn.error_message(result, 'add combi_bonus effect_id column')
		}
	}
	combi_cols2 := conn.columns('combi_bonus') or { return }
	if 'effect' in combi_cols2 {
		rows := conn.exec('SELECT id, effect FROM combi_bonus') or { [] }
		for r in rows {
			cb_id := r.get_int('id')
			eff := r.get_string('effect').trim_space()
			if eff == '' {
				continue
			}
			effect_id := find_or_create_effect(conn, 'en', eff)!
			result := conn.exec_none('UPDATE combi_bonus SET effect_id = ${effect_id} WHERE id = ${cb_id}')
			if !sqlite_success(result) {
				return conn.error_message(result, 'link combi bonus effect')
			}
		}
		result := conn.exec_none('ALTER TABLE combi_bonus DROP COLUMN effect')
		if !sqlite_success(result) {
			return conn.error_message(result, 'drop combi_bonus effect column')
		}
	}
}

@[inline]
// clamp_level bounds a treasure slot level to the valid 0-9 range. The form
// parse already clamps, but the persistence layer must not trust callers:
// no code path can write an out-of-range level into the build table.
fn clamp_level(l int) int {
	if l < 0 {
		return 0
	}
	if l > 9 {
		return 9
	}
	return l
}

// sanitize_blessed clears a treasure slot's blessed flag when the equipped
// treasure has no blessed effect rows, so a build can never persist a blessed
// state the detail/list pages cannot render values for (e.g. from a tampered
// or stale request). The flag survives only when the treasure is
// blessed-capable; an unselected slot (id 0) is never blessed.
fn sanitize_blessed(conn sqlite.DB, treasure_id int, blessed int) int {
	if treasure_id == 0 || blessed == 0 {
		return 0
	}
	rows := sql conn {
		select from models.TreasureEffect where treasure_id == treasure_id
			&& state == models.EffectState.blessed limit 1
	} or { [] }
	if rows.len == 0 {
		return 0
	}
	return 1
}

fn exec(conn sqlite.DB, nq string) ! {
	query := nq.trim_space()
	log.debug("Executing: `${query}`")
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
