module database

import db.sqlite
import models
import time

@[table: 'cookie']
pub struct CookieView {
	pub:
		cookie_id int
		image ?string
		grade models.Grade
		release_date time.Time

		lang string
		name string
		abilities string
		description string
		power_plus string
		power_plus_requirement string
		unlock_goal string
}

struct BackupTranslationParam {
	owner_id ?int
	limit int = -1
}

fn backup_translations[T](conn sqlite.DB, user_lang string, params ?BackupTranslationParam) !map[int]T {
	abs_params := params or { BackupTranslationParam{} }

	translations := if ow_id := abs_params.owner_id {
		sql conn {
			select from T
			where (lang == user_lang || lang == 'en') && owner_id == ow_id
			limit abs_params.limit
		}!
	} else {
		sql conn {
			select from T
			where lang == user_lang || lang == 'en'
			limit abs_params.limit
		}!
	}

	mut translation_map := map[int]T{}

	// Requested language first
	for tr in translations {
		if tr.lang == user_lang {
			translation_map[tr.owner_id] = tr
		}
	}

	// English fallback
	for tr in translations {
		if tr.owner_id !in translation_map {
			translation_map[tr.owner_id] = tr
		}
	}

	return translation_map
}

pub fn select_cookies(conn sqlite.DB, lang string, limit int, offset int) ![]CookieView {
	cookies := sql conn {
		select from models.Cookie
		order by cookie_id desc
		limit limit
		offset offset
	}!

	if cookies.len == 0 {
		return []
	}

	mut ids := []int{}
	for cookie in cookies {
		ids << cookie.cookie_id or { continue }
	}

	translation_map := backup_translations[models.CookieTranslation](conn, lang)!

	mut result := []CookieView{}

	for cookie in cookies {
		if tr := translation_map[cookie.cookie_id or { continue }] {
			result << CookieView{
				cookie_id: cookie.cookie_id or { 0 }
				image: cookie.image
				grade: cookie.grade
				release_date: cookie.release_date
				lang: tr.lang
				name: tr.name
				abilities: tr.abilities
				description: tr.description
				power_plus: tr.power_plus
				power_plus_requirement: tr.power_plus_requirement
				unlock_goal: tr.unlock_goal
			}
		}
	}

	return result
}

@[table: 'pet']
pub struct PetView {
	pub:
		pet_id int
		image ?string
		grade models.Grade
		release_date time.Time

		lang string
		name string
		abilities string
		description string
}

pub fn select_pets(conn sqlite.DB, lang string, limit int, offset int) ![]PetView {
	pets := sql conn {
		select from models.Pet
		order by pet_id desc
		limit limit
		offset offset
	}!

	if pets.len == 0 {
		return []
	}

	user_lang := lang
	translations := sql conn {
		select from models.PetTranslation
		where lang == user_lang || lang == 'en'
	}!

	mut translation_map := map[int]models.PetTranslation{}
	for tr in translations {
		if tr.lang == lang {
			translation_map[tr.pet_id] = tr
		}
	}
	for tr in translations {
		if tr.pet_id !in translation_map {
			translation_map[tr.pet_id] = tr
		}
	}

	mut result := []PetView{}

	for pet in pets {
		if tr := translation_map[pet.pet_id or { continue }] {
			result << PetView{
				pet_id: pet.pet_id or { 0 }
				image: pet.image
				grade: pet.grade
				release_date: pet.release_date
				lang: tr.lang
				name: tr.name
				abilities: tr.abilities
				description: tr.description
			}
		}
	}

	return result
}

pub fn get_pet(conn sqlite.DB, lang string, id int) !PetView {
	if id <= 0 {
		return error('invalid pet id')
	}

	pets := sql conn {
		select from models.Pet
		where pet_id == id
	}!

	if pets.len == 0 {
		return error('pet (${id}) not found')
	}
	pet := pets.first()

	user_lang := lang
	translations := sql conn {
		select from models.PetTranslation
		where pet_id == id && (lang == user_lang || lang == 'en')
	}!

	mut tr := if translations.len > 0 { translations.first() } else { return error('pet (${id}) has no translation') }
	for t in translations {
		if t.lang == user_lang {
			tr = t
			break
		}
	}

	return PetView{
		pet_id: pet.pet_id or { 0 }
		image: pet.image
		grade: pet.grade
		release_date: pet.release_date
		lang: tr.lang
		name: tr.name
		abilities: tr.abilities
		description: tr.description
	}
}

@[table: 'treasure']
pub struct TreasureView {
	pub:
		treasure_id      int
		image            ?string
		grade            ?models.Grade
		base_treasure_id ?int
		is_evolved       bool
		release_date     time.Time

		lang string
		name string
		description string
}

// treasure_view maps a stored row + translation to the view handed to templates
fn treasure_view(t models.Treasure, tr models.TreasureTranslation) TreasureView {
	return TreasureView{
		treasure_id:      t.treasure_id or { 0 }
		image:            t.image
		grade:            treasure_grade(t.grade)
		base_treasure_id: t.base_treasure_id
		is_evolved:       t.is_evolved
		release_date:     t.release_date
		lang:             tr.lang
		name:             tr.name
		description:      tr.description
	}
}

// best_treasure_translation picks the user's language when present, else en
fn best_treasure_translation(conn sqlite.DB, lang string, tid int) !models.TreasureTranslation {
	user_lang := lang
	translations := sql conn {
		select from models.TreasureTranslation
		where treasure_id == tid && (lang == user_lang || lang == 'en')
	}!
	if translations.len == 0 {
		return error('treasure (${tid}) has no translation')
	}
	mut tr := translations.first()
	for t in translations {
		if t.lang == user_lang {
			tr = t
			break
		}
	}
	return tr
}

fn compare_treasures(a &TreasureView, b &TreasureView) int {
	ad := a.release_date.unix()
	bd := b.release_date.unix()
	if ad != bd {
		if ad > bd {
			return -1
		}
		return 1
	}
	if a.name < b.name {
		return -1
	}
	if a.name > b.name {
		return 1
	}
	return 0
}

@[table: 'effect']
pub struct EffectView {
	pub:
		effect_id     int
		name          string
		value_display string
}

// treasure_grade maps the stored int enum value back to the Grade enum;
// none when the treasure has no wiki grade (renders without a badge).
fn treasure_grade(g ?int) ?models.Grade {
	if v := g {
		return models.Grade.from(v) or { none }
	}
	return none
}

// format_effect_value renders the extracted numeric value with its unit
// suffix ("12%", "3s", "5000"); empty when the text carried no single value
// (e.g. ranges like "5-6%").
fn format_effect_value(value ?f32, unit models.EffectUnit) string {
	val := value or { return '' }
	mut s := val.str()
	if s.contains('.') {
		s = s.trim_right('0').trim_right('.')
	}
	return match unit {
		.percent { '${s}%' }
		.second { '${s}s' }
		.flat { s }
	}
}

pub fn get_treasure(conn sqlite.DB, lang string, id int) !TreasureView {
	if id <= 0 {
		return error('invalid treasure id')
	}

	treasures := sql conn {
		select from models.Treasure
		where treasure_id == id
	}!

	if treasures.len == 0 {
		return error('treasure (${id}) not found')
	}
	treasure := treasures.first()
	return treasure_view(treasure, best_treasure_translation(conn, lang, id)!)
}

// get_treasure_base returns the normal treasure an evolved row evolved from
pub fn get_treasure_base(conn sqlite.DB, lang string, base_id int) !TreasureView {
	if base_id <= 0 {
		return error('no base treasure')
	}
	bases := sql conn {
		select from models.Treasure
		where treasure_id == base_id
	}!
	if bases.len == 0 {
		return error('base treasure (${base_id}) not found')
	}
	b := bases.first()
	return treasure_view(b, best_treasure_translation(conn, lang, base_id)!)
}

// get_treasure_evo returns the evolved variant of a base treasure; error
// when the treasure has no evolved form
pub fn get_treasure_evo(conn sqlite.DB, lang string, id int) !TreasureView {
	evos := sql conn {
		select from models.Treasure
		where base_treasure_id == id && is_evolved == true
	}!
	if evos.len == 0 {
		return error('treasure (${id}) has no evolved variant')
	}
	e := evos.first()
	return treasure_view(e, best_treasure_translation(conn, lang, e.treasure_id or { 0 })!)
}

// effects_from_links resolves effect translations and formats each link into
// an EffectView, preserving link order and dropping duplicate effects
fn effects_from_links(conn sqlite.DB, lang string, links []models.TreasureEffect) ![]EffectView {
	if links.len == 0 {
		return []
	}

	mut effect_ids := []int{}
	mut seen := map[int]bool{}
	for link in links {
		if link.effect_id !in seen {
			effect_ids << link.effect_id
			seen[link.effect_id] = true
		}
	}

	user_lang := lang
	translations := sql conn {
		select from models.EffectTranslation
		where effect_id in effect_ids && (lang == user_lang || lang == 'en')
	}!

	mut translation_map := map[int]models.EffectTranslation{}
	for tr in translations {
		if tr.lang == lang {
			translation_map[tr.effect_id] = tr
		}
	}
	for tr in translations {
		if tr.effect_id !in translation_map {
			translation_map[tr.effect_id] = tr
		}
	}

	mut emitted := map[int]bool{}
	mut result := []EffectView{}
	for link in links {
		if link.effect_id in emitted {
			continue
		}
		emitted[link.effect_id] = true
		if tr := translation_map[link.effect_id] {
			result << EffectView{
				effect_id:     link.effect_id
				name:          tr.name
				value_display: format_effect_value(link.value, link.unit)
			}
		}
	}
	return result
}

// effects_by_state returns the treasure's effects for one state (normal or
// blessed), in the order the wiki listed them (treasure_effect_id order).
fn effects_by_state(conn sqlite.DB, lang string, id int, st models.EffectState) ![]EffectView {
	links := sql conn {
		select from models.TreasureEffect
		where treasure_id == id && state == st
		order by treasure_effect_id
	}!
	return effects_from_links(conn, lang, links)
}

// get_treasure_effects returns one row per distinct normal effect of the
// treasure, in the order the wiki listed them.
pub fn get_treasure_effects(conn sqlite.DB, lang string, id int) ![]EffectView {
	return effects_by_state(conn, lang, id, models.EffectState.normal)
}

// get_treasure_blessed_effects returns the blessed-state effects of an
// evolved treasure, in the order the wiki listed them; empty when the
// treasure has no blessed form
pub fn get_treasure_blessed_effects(conn sqlite.DB, lang string, id int) ![]EffectView {
	return effects_by_state(conn, lang, id, models.EffectState.blessed)
}

// select_treasures lists treasures newest-first; evolved selects only evolved
// rows, false only normal rows.
pub fn select_treasures(conn sqlite.DB, lang string, limit int, offset int, evolved bool) ![]TreasureView {
	treasures := sql conn {
		select from models.Treasure
	}!

	if treasures.len == 0 {
		return []
	}

	user_lang := lang
	translations := sql conn {
		select from models.TreasureTranslation
		where lang == user_lang || lang == 'en'
	}!

	mut translation_map := map[int]models.TreasureTranslation{}
	for tr in translations {
		if tr.lang == lang {
			translation_map[tr.treasure_id] = tr
		}
	}
	for tr in translations {
		if tr.treasure_id !in translation_map {
			translation_map[tr.treasure_id] = tr
		}
	}

	mut result := []TreasureView{}

	for treasure in treasures {
		if treasure.is_evolved != evolved {
			continue
		}
		if tr := translation_map[treasure.treasure_id or { continue }] {
			result << treasure_view(treasure, tr)
		}
	}

	result.sort_with_compare(compare_treasures)

	// paginate after the in-memory sort (newest first, name tie-break)
	if offset >= result.len {
		return []
	}
	if offset > 0 {
		result = result[offset..]
	}
	if result.len > limit {
		result = result[..limit]
	}
	return result
}

pub struct SearchResults {
	pub mut:
		cookies   []CookieView
		pets      []PetView
		treasures []TreasureView
}

// fts_match_query turns free-text input into an FTS5 MATCH expression: each
// whitespace-separated token becomes a double-quoted phrase with a prefix
// wildcard, so "sea fairy" matches "Sea Fairy" and partial words.
fn fts_match_query(q string) string {
	mut terms := []string{}
	for tok in q.split(' ') {
		t := tok.trim_space()
		if t != '' {
			terms << '"${t.replace('"', '""')}"*'
		}
	}
	return terms.join(' ')
}

// fts_owner_ids returns the entity ids whose translations (in fts_table)
// match `q`. Rowids of the FTS table are translation ids, which are mapped
// back to entity ids via the translation table.
fn fts_owner_ids(conn sqlite.DB, fts_table string, translation_table string, translation_id_col string, owner_col string, q string, limit int) ![]int {
	match_expr := fts_match_query(q)
	if match_expr == '' {
		return []
	}
	query := "SELECT rowid FROM ${fts_table} WHERE ${fts_table} MATCH '${match_expr.replace("'", "''")}' ORDER BY rank LIMIT ${limit}"
	rows := conn.exec(query) or { return [] }
	if rows.len == 0 {
		return []
	}
	mut in_list := []string{}
	for r in rows {
		in_list << r.get_int('rowid').str()
	}
	owner_rows := conn.exec('SELECT ${owner_col} AS owner_id FROM ${translation_table} WHERE ${translation_id_col} IN (${in_list.join(', ')})') or {
		return []
	}
	mut ids := []int{}
	mut seen := map[int]bool{}
	for r in owner_rows {
		id := r.get_int('owner_id')
		if id !in seen {
			ids << id
			seen[id] = true
		}
	}
	return ids
}

// search_all returns up to `limit` matches per entity type, ranked by FTS5.
pub fn search_all(conn sqlite.DB, lang string, q string, limit int) !SearchResults {
	mut results := SearchResults{}
	for id in fts_owner_ids(conn, 'cookie_translation_fts', 'cookie_translation', 'cookie_translation_id', 'owner_id', q, limit)! {
		results.cookies << get_cookie(conn, lang, id) or { continue }
	}
	for id in fts_owner_ids(conn, 'pet_translation_fts', 'pet_translation', 'pet_translation_id', 'pet_id', q, limit)! {
		results.pets << get_pet(conn, lang, id) or { continue }
	}
	for id in fts_owner_ids(conn, 'treasure_translation_fts', 'treasure_translation', 'treasure_translation_id', 'treasure_id', q, limit)! {
		results.treasures << get_treasure(conn, lang, id) or { continue }
	}
	return results
}

pub fn get_cookie(conn sqlite.DB, lang string, id int) !CookieView {
	if id <= 0 {
		return error('invalid cookie id')
	}

	cookies := sql conn {
		select from models.Cookie
		where cookie_id == id
	}!

	if cookies.len == 0 {
		return error('cookie (${id}) not found')
	}
	cookie := cookies.first()

	translations := backup_translations[models.CookieTranslation](conn, lang, owner_id: id)!

	return CookieView{
		cookie_id: cookie.cookie_id or { 0 }
		image: cookie.image
		grade: cookie.grade
		release_date: cookie.release_date
		lang: translations[id].lang
		name: translations[id].name
		abilities: translations[id].abilities
		description: translations[id].description
		power_plus: translations[id].power_plus
		power_plus_requirement: translations[id].power_plus_requirement
		unlock_goal: translations[id].unlock_goal
	}
}
