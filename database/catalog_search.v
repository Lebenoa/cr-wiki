module database

import db.sqlite
import database.models

// CatalogHit is one catalog search result (relic, episode, ingredient or
// quest) with its localized name and the episode it belongs to ('' when the
// row has no episode, e.g. event relics and the general-quest fallback).
pub struct CatalogHit {
pub:
	id          int
	kind        string // relic | episode | ingredient | quest — drives links and icons
	name        string
	description string
	image       ?string
	episode_id  ?int
	episode     string // owning episode's localized name; '' when none
}

// catalog_translations resolves (id -> [lang]name) rows for one translation
// table, in the user's language with an English fallback.
fn catalog_translations(conn sqlite.DB, translation_table string, id_col string, lang string) map[int]map[string]string {
	plang := lang
	mut out := map[int]map[string]string{}
	rows := conn.exec('SELECT ${id_col} AS eid, lang, name FROM ${translation_table} WHERE lang = \'${plang}\' OR lang = \'en\'') or {
		return out
	}
	for r in rows {
		eid := r.get_int('eid')
		if eid !in out {
			out[eid] = {}
		}
		out[eid][r.get_string('lang')] = r.get_string('name')
	}
	return out
}

// episodes_by_ids returns the episodes for the given ids (ordered by `ids`),
// with localized names and images.
pub fn episodes_by_ids(conn sqlite.DB, lang string, ids []int) []CatalogHit {
	if ids.len == 0 {
		return []CatalogHit{}
	}
	names := catalog_translations(conn, 'episode_translation', 'episode_id', lang)
	rows := sql conn {
		select from models.Episode
	} or { return [] }
	mut by_id := map[int]models.Episode{}
	for e in rows {
		by_id[e.episode_id or { 0 }] = e
	}
	mut out := []CatalogHit{}
	for id in ids {
		if e := by_id[id] {
			out << CatalogHit{
				id:          id
				kind:        'episode'
				name:        lang_name(names, id, lang)
				image:       e.image
				description: e.entry_cost
			}
		}
	}
	return out
}

// relics_by_ids returns the relics for the given ids (ordered by `ids`) with
// localized names, images and owning episode names.
pub fn relics_by_ids(conn sqlite.DB, lang string, ids []int) []CatalogHit {
	if ids.len == 0 {
		return []CatalogHit{}
	}
	names := catalog_translations(conn, 'relic_translation', 'relic_id', lang)
	rows := sql conn {
		select from models.Relic
	} or { return [] }
	mut by_id := map[int]models.Relic{}
	for r in rows {
		by_id[r.relic_id or { 0 }] = r
	}
	ep_names := episode_translation_map(conn, lang)
	mut out := []CatalogHit{}
	for id in ids {
		if r := by_id[id] {
			out << CatalogHit{
				id:         id
				kind:       'relic'
				name:       lang_name(names, id, lang)
				image:      r.image
				episode_id: r.episode_id
				episode:    lang_name(ep_names, r.episode_id or { 0 }, lang)
			}
		}
	}
	return out
}

// ingredients_by_ids returns the ingredients for the given ids (ordered by
// `ids`) with localized names and images.
pub fn ingredients_by_ids(conn sqlite.DB, lang string, ids []int) []CatalogHit {
	if ids.len == 0 {
		return []CatalogHit{}
	}
	names := catalog_translations(conn, 'ingredient_translation', 'ingredient_id', lang)
	rows := sql conn {
		select from models.Ingredient
	} or { return [] }
	mut by_id := map[int]models.Ingredient{}
	for i in rows {
		by_id[i.ingredient_id or { 0 }] = i
	}
	mut out := []CatalogHit{}
	for id in ids {
		if i := by_id[id] {
			out << CatalogHit{
				id:          id
				kind:        'ingredient'
				name:        lang_name(names, id, lang)
				image:       i.image
				episode_id:  i.drop_episode_id
				description: i.obtained_from
			}
		}
	}
	return out
}

// quest_owner_ids returns quest ids whose rows match `q` via the quest FTS
// index (rowid == quest_id, so no translation hop). Thai queries bypass FTS
// for a LIKE scan of the quest columns (the tokenizer can't segment Thai).
fn quest_owner_ids(conn sqlite.DB, q string, limit int) ![]int {
	if has_thai(q) {
		ids := like_owner_ids(conn, 'quest', 'quest_id', ['name', 'requirement', 'reward',
			'group'], q, limit)!
		return ids
	}
	match_expr := fts_match_query(q)
	if match_expr == '' {
		return []
	}
	query := "SELECT rowid FROM quest_fts WHERE quest_fts MATCH '${match_expr.replace("'", "''")}' ORDER BY ${fts_rank_clause('quest_fts')} LIMIT ${limit}"
	rows := conn.exec(query) or { return [] }
	mut ids := []int{}
	for r in rows {
		ids << r.get_int('rowid')
	}
	return ids
}

// quests_by_ids returns the quests for the given ids (ordered by `ids`) with
// their names, episode names and rewards.
pub fn quests_by_ids(conn sqlite.DB, ids []int) []CatalogHit {
	if ids.len == 0 {
		return []CatalogHit{}
	}
	rows := sql conn {
		select from models.Quest
	} or { return [] }
	mut by_id := map[int]models.Quest{}
	for q in rows {
		by_id[q.quest_id or { 0 }] = q
	}
	ep_names := episode_translation_map(conn, 'en')
	mut out := []CatalogHit{}
	for id in ids {
		if q := by_id[id] {
			out << CatalogHit{
				id:          id
				kind:        'quest'
				name:        q.name
				description: q.reward
				episode_id:  q.episode_id
				episode:     lang_name(ep_names, q.episode_id, 'en')
			}
		}
	}
	return out
}
