module database

import db.sqlite
import models
import time

@[table: 'cookie']
pub struct CookieView {
pub:
	cookie_id    int
	image        ?string
	grade        models.Grade
	release_date time.Time

	lang                   string
	name                   string
	abilities              string
	description            string
	power_plus             string
	power_plus_requirement string
	unlock_goal            string
}

struct BackupTranslationParam {
	owner_id ?int
	limit    int = -1
}

fn backup_translations[T](conn sqlite.DB, user_lang string, params ?BackupTranslationParam) !map[int]T {
	abs_params := params or { BackupTranslationParam{} }

	translations := if ow_id := abs_params.owner_id {
		sql conn {
			select from T where (lang == user_lang || lang == 'en') && owner_id == ow_id limit abs_params.limit
		}!
	} else {
		sql conn {
			select from T where lang == user_lang || lang == 'en' limit abs_params.limit
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
		select from models.Cookie order by cookie_id desc limit limit offset offset
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
				cookie_id:              cookie.cookie_id or { 0 }
				image:                  cookie.image
				grade:                  cookie.grade
				release_date:           cookie.release_date
				lang:                   tr.lang
				name:                   tr.name
				abilities:              tr.abilities
				description:            tr.description
				power_plus:             tr.power_plus
				power_plus_requirement: tr.power_plus_requirement
				unlock_goal:            tr.unlock_goal
			}
		}
	}

	return result
}

@[table: 'pet']
pub struct PetView {
pub:
	pet_id       int
	image        ?string
	grade        models.Grade
	release_date time.Time

	lang        string
	name        string
	abilities   string
	description string
}

pub fn select_pets(conn sqlite.DB, lang string, limit int, offset int) ![]PetView {
	pets := sql conn {
		select from models.Pet order by pet_id desc limit limit offset offset
	}!

	if pets.len == 0 {
		return []
	}

	user_lang := lang
	translations := sql conn {
		select from models.PetTranslation where lang == user_lang || lang == 'en'
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
				pet_id:       pet.pet_id or { 0 }
				image:        pet.image
				grade:        pet.grade
				release_date: pet.release_date
				lang:         tr.lang
				name:         tr.name
				abilities:    tr.abilities
				description:  tr.description
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
		select from models.Pet where pet_id == id
	}!

	if pets.len == 0 {
		return error('pet (${id}) not found')
	}
	pet := pets.first()

	user_lang := lang
	translations := sql conn {
		select from models.PetTranslation where pet_id == id && (lang == user_lang || lang == 'en')
	}!

	mut tr := if translations.len > 0 {
		translations.first()
	} else {
		return error('pet (${id}) has no translation')
	}
	for t in translations {
		if t.lang == user_lang {
			tr = t
			break
		}
	}

	return PetView{
		pet_id:       pet.pet_id or { 0 }
		image:        pet.image
		grade:        pet.grade
		release_date: pet.release_date
		lang:         tr.lang
		name:         tr.name
		abilities:    tr.abilities
		description:  tr.description
	}
}

@[table: 'treasure']
pub struct TreasureView {
pub:
	treasure_id      int
	image            ?string
	grade            ?models.Grade
	base_treasure_id ?int
	unlock_cookie_id ?int
	unlock_pet_id    ?int
	is_evolved       bool
	release_date     time.Time

	lang        string
	name        string
	description string
}

// treasure_view maps a stored row + translation to the view handed to templates
fn treasure_view(t models.Treasure, tr models.TreasureTranslation) TreasureView {
	return TreasureView{
		treasure_id:      t.treasure_id or { 0 }
		image:            t.image
		grade:            treasure_grade(t.grade)
		base_treasure_id: t.base_treasure_id
		unlock_cookie_id: t.unlock_cookie_id
		unlock_pet_id:    t.unlock_pet_id
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
		select from models.TreasureTranslation where treasure_id == tid
		&& (lang == user_lang || lang == 'en')
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
		select from models.Treasure where treasure_id == id
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
		select from models.Treasure where treasure_id == base_id
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
		select from models.Treasure where base_treasure_id == id && is_evolved == true
	}!
	if evos.len == 0 {
		return error('treasure (${id}) has no evolved variant')
	}
	e := evos.first()
	return treasure_view(e, best_treasure_translation(conn, lang, e.treasure_id or { 0 })!)
}

// TreasureUnlock describes the cookie or pet whose max-level upgrade unlocks
// the treasure, for the detail page's unlock panel.
pub struct TreasureUnlock {
pub:
	kind  string // 'cookie' | 'pet'
	id    int
	name  string
	image ?string
}

// get_treasure_unlock resolves the treasure's unlock entity (cookie or pet)
// in the user's language; error when the treasure isn't unlocked by an
// upgrade, so callers can treat it as "no unlock panel".
pub fn get_treasure_unlock(conn sqlite.DB, lang string, t TreasureView) !TreasureUnlock {
	user_lang := lang
	if cid := t.unlock_cookie_id {
		trs := sql conn {
			select from models.CookieTranslation where owner_id == cid
			&& (lang == user_lang || lang == 'en')
		}!
		if trs.len == 0 {
			return error('unlock cookie (${cid}) has no translation')
		}
		mut tr := trs.first()
		for x in trs {
			if x.lang == user_lang {
				tr = x
				break
			}
		}
		img := unlock_entity_image(conn, 'cookie', cid)
		return TreasureUnlock{
			kind:  'cookie'
			id:    cid
			name:  tr.name
			image: img
		}
	}
	if pid := t.unlock_pet_id {
		trs := sql conn {
			select from models.PetTranslation where pet_id == pid && (lang == user_lang || lang == 'en')
		}!
		if trs.len == 0 {
			return error('unlock pet (${pid}) has no translation')
		}
		mut tr := trs.first()
		for x in trs {
			if x.lang == user_lang {
				tr = x
				break
			}
		}
		img := unlock_entity_image(conn, 'pet', pid)
		return TreasureUnlock{
			kind:  'pet'
			id:    pid
			name:  tr.name
			image: img
		}
	}
	return error('treasure has no cookie/pet unlock')
}

// unlock_entity_image returns the cookie/pet sprite filename; none when the
// row is missing (the template falls back to a placeholder URL).
fn unlock_entity_image(conn sqlite.DB, kind string, id int) ?string {
	mut img := ?string(none)
	if kind == 'cookie' {
		rows := sql conn {
			select from models.Cookie where cookie_id == id
		} or { return none }
		if rows.len > 0 {
			img = rows.first().image
		}
	} else if kind == 'pet' {
		rows := sql conn {
			select from models.Pet where pet_id == id
		} or { return none }
		if rows.len > 0 {
			img = rows.first().image
		}
	}
	return img
}

// CombiBonusView is one combo-bonus row rendered on a cookie/pet detail page:
// the partner entity (the other half of the pair) plus the bonus effect text.
pub struct CombiBonusView {
pub:
	partner_kind  string // 'cookie' | 'pet'
	partner_id    int
	partner_name  string
	partner_image ?string
	effect        string
}

// get_combi_bonus returns the combo bonuses involving `kind` (cookie or pet)
// with id `id`, resolving each partner's name (user lang, en fallback) and
// sprite; hidden rows are skipped. Empty when the entity has no combos.
pub fn get_combi_bonus(conn sqlite.DB, lang string, kind string, id int) ![]CombiBonusView {
	mut rows := []models.CombiBonus{}
	if kind == 'cookie' {
		rows = sql conn {
			select from models.CombiBonus where cookie_id == id && is_hidden == false
		}!
	} else if kind == 'pet' {
		rows = sql conn {
			select from models.CombiBonus where pet_id == id && is_hidden == false
		}!
	} else {
		return error('invalid combi kind')
	}
	if rows.len == 0 {
		return []
	}

	user_lang := lang
	mut result := []CombiBonusView{}
	for row in rows {
		partner_kind := if kind == 'cookie' { 'pet' } else { 'cookie' }
		pid := if kind == 'cookie' { row.pet_id } else { row.cookie_id }
		mut name := ''
		if partner_kind == 'pet' {
			trs := sql conn {
				select from models.PetTranslation where pet_id == pid && (lang == user_lang || lang == 'en')
			}!
			if trs.len == 0 {
				continue
			}
			mut tr := trs.first()
			for x in trs {
				if x.lang == user_lang {
					tr = x
					break
				}
			}
			name = tr.name
		} else {
			trs := sql conn {
				select from models.CookieTranslation where owner_id == pid
				&& (lang == user_lang || lang == 'en')
			}!
			if trs.len == 0 {
				continue
			}
			mut tr := trs.first()
			for x in trs {
				if x.lang == user_lang {
					tr = x
					break
				}
			}
			name = tr.name
		}
		result << CombiBonusView{
			partner_kind:  partner_kind
			partner_id:    pid
			partner_name:  name
			partner_image: unlock_entity_image(conn, partner_kind, pid)
			effect:        row.effect
		}
	}
	return result
}

// get_unlocked_treasure returns the treasure unlocked by upgrading the given
// cookie or pet to max level; error when the entity unlocks no treasure.
pub fn get_unlocked_treasure(conn sqlite.DB, lang string, kind string, id int) !TreasureView {
	mut ts := []models.Treasure{}
	if kind == 'cookie' {
		ts = sql conn {
			select from models.Treasure where unlock_cookie_id == id
		}!
	} else if kind == 'pet' {
		ts = sql conn {
			select from models.Treasure where unlock_pet_id == id
		}!
	}
	if ts.len == 0 {
		return error('${kind} (${id}) unlocks no treasure')
	}
	t := ts.first()
	return treasure_view(t, best_treasure_translation(conn, lang, t.treasure_id or { 0 })!)
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
		select from models.EffectTranslation where effect_id in effect_ids
		&& (lang == user_lang || lang == 'en')
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
		select from models.TreasureEffect where treasure_id == id && state == st order by treasure_effect_id
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

// select_treasures lists treasures newest-first; tab filters to normal or
// evolved rows, 'all' returns both.
pub fn select_treasures(conn sqlite.DB, lang string, limit int, offset int, tab string) ![]TreasureView {
	treasures := sql conn {
		select from models.Treasure
	}!

	if treasures.len == 0 {
		return []
	}

	user_lang := lang
	translations := sql conn {
		select from models.TreasureTranslation where lang == user_lang || lang == 'en'
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
		if (tab == 'normal' && treasure.is_evolved) || (tab == 'evo' && !treasure.is_evolved) {
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
		result = result[offset..].clone()
	}
	if result.len > limit {
		result = result[..limit].clone()
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

// thai_search_cols lists the translation columns searched for Thai queries.
// It includes columns the FTS tables don't index (unlock_goal, power_plus,
// pet abilities) because that's where the Thai content lives.
fn thai_search_cols(table string) []string {
	return match table {
		'cookie_translation' {
			['name', 'abilities', 'description', 'power_plus', 'power_plus_requirement',
				'unlock_goal']
		}
		'pet_translation' {
			['name', 'abilities', 'description']
		}
		'treasure_translation' {
			['name', 'description']
		}
		else {
			[]
		}
	}
}

// has_thai reports whether the string contains any Thai-script character
// (U+0E00–U+0E7F).
fn has_thai(s string) bool {
	for r in s.runes() {
		if r >= 0x0e00 && r <= 0x0e7f {
			return true
		}
	}
	return false
}

// like_escape escapes SQL LIKE wildcards (and quotes, for inline
// interpolation) so user input matches literally.
fn like_escape(s string) string {
	return s.replace('\\', '\\\\').replace('%', '\\%').replace('_', '\\_').replace("'", "''")
}

// like_owner_ids finds entity ids via substring (LIKE) matching. Used when the
// query contains Thai: FTS5's unicode61 tokenizer can't segment Thai, so
// prefix matching misses substrings (and V's bundled build drops Thai tokens
// entirely). Whitespace-separated tokens are AND'd, each token matching any
// of the searched columns.
fn like_owner_ids(conn sqlite.DB, translation_table string, owner_col string, cols []string, q string, limit int) ![]int {
	mut conds := []string{}
	for raw in q.split(' ') {
		tok := raw.trim_space()
		if tok == '' {
			continue
		}
		escaped := like_escape(tok)
		mut ors := []string{}
		for col in cols {
			ors << '${col} LIKE \'%${escaped}%\' ESCAPE \'\\\''
		}
		conds << '(' + ors.join(' OR ') + ')'
	}
	if conds.len == 0 {
		return []
	}
	query := 'SELECT DISTINCT ${owner_col} AS owner_id FROM ${translation_table} WHERE ${conds.join(' AND ')} ORDER BY owner_id LIMIT ${limit}'
	rows := conn.exec(query) or { return [] }
	mut ids := []int{}
	for r in rows {
		ids << r.get_int('owner_id')
	}
	return ids
}

// fts_rank_clause returns the ORDER BY expression for `fts_table`, weighting
// columns so title matches beat body-text mentions regardless of description
// length (bm25's length normalization otherwise lets a "chocolate" mention in
// a short description outrank "Mint Choco" in a long one). The lang column is
// weighted 0 so language tags never affect relevance.
fn fts_rank_clause(fts_table string) string {
	return match fts_table {
		'cookie_translation_fts' { 'bm25(${fts_table}, 10.0, 2.0, 1.0, 0.0)' }
		'pet_translation_fts', 'treasure_translation_fts', 'effect_translation_fts' {
			'bm25(${fts_table}, 10.0, 1.0, 0.0)'
		}
		else { 'rank' }
	}
}

// fts_owner_ids returns the entity ids whose translations (in fts_table)
// match `q`, in FTS5 rank order. Rowids of the FTS table are translation ids,
// which are mapped back to entity ids via the translation table; the ranked
// rowid list is walked in order because a plain `WHERE id IN (...)` returns
// rows in table order, silently dropping the ranking. Thai queries bypass FTS
// (see like_owner_ids) since the tokenizer can't segment Thai.
fn fts_owner_ids(conn sqlite.DB, fts_table string, translation_table string, translation_id_col string, owner_col string, q string, limit int) ![]int {
	if has_thai(q) {
		cols := thai_search_cols(translation_table)
		if cols.len == 0 {
			return []
		}
		return like_owner_ids(conn, translation_table, owner_col, cols, q, limit)
	}
	match_expr := fts_match_query(q)
	if match_expr == '' {
		return []
	}
	query := "SELECT rowid FROM ${fts_table} WHERE ${fts_table} MATCH '${match_expr.replace("'", "''")}' ORDER BY ${fts_rank_clause(fts_table)} LIMIT ${limit}"
	rows := conn.exec(query) or { return [] }
	if rows.len == 0 {
		return []
	}
	mut in_list := []string{}
	for r in rows {
		in_list << r.get_int('rowid').str()
	}
	owner_rows := conn.exec('SELECT ${translation_id_col} AS tid, ${owner_col} AS owner_id FROM ${translation_table} WHERE ${translation_id_col} IN (${in_list.join(', ')})') or {
		return []
	}
	mut owner_by_tid := map[int]int{}
	for r in owner_rows {
		owner_by_tid[r.get_int('tid')] = r.get_int('owner_id')
	}
	mut ids := []int{}
	mut seen := map[int]bool{}
	for tid_s in in_list {
		id := owner_by_tid[tid_s.int()] or { continue }
		if id !in seen {
			ids << id
			seen[id] = true
		}
	}
	return ids
}

// cookies_by_ids returns the cookies for the given ids, ordered by `ids` so
// FTS5 rank order survives the batch fetch. Translations prefer `lang` with
// English fallback, matching get_cookie but in two queries total instead of
// two per cookie. Entities are fetched unfiltered (the ORM cannot use `in` on
// the optional serial id column) and filtered in memory; the tables are small.
fn cookies_by_ids(conn sqlite.DB, lang string, ids []int) ![]CookieView {
	if ids.len == 0 {
		return []
	}
	user_lang := lang
	cookies := sql conn {
		select from models.Cookie
	}!
	translations := sql conn {
		select from models.CookieTranslation where owner_id in ids
		&& (lang == user_lang || lang == 'en')
	}!

	mut cookie_map := map[int]models.Cookie{}
	for cookie in cookies {
		if id := cookie.cookie_id {
			cookie_map[id] = cookie
		}
	}
	mut translation_map := map[int]models.CookieTranslation{}
	for tr in translations {
		if tr.lang == user_lang {
			translation_map[tr.owner_id] = tr
		}
	}
	for tr in translations {
		if tr.owner_id !in translation_map {
			translation_map[tr.owner_id] = tr
		}
	}

	mut result := []CookieView{}
	for id in ids {
		if cookie := cookie_map[id] {
			if tr := translation_map[id] {
				result << CookieView{
					cookie_id:              cookie.cookie_id or { 0 }
					image:                  cookie.image
					grade:                  cookie.grade
					release_date:           cookie.release_date
					lang:                   tr.lang
					name:                   tr.name
					abilities:              tr.abilities
					description:            tr.description
					power_plus:             tr.power_plus
					power_plus_requirement: tr.power_plus_requirement
					unlock_goal:            tr.unlock_goal
				}
			}
		}
	}
	return result
}

// pets_by_ids is the batched counterpart of get_pet for search results.
fn pets_by_ids(conn sqlite.DB, lang string, ids []int) ![]PetView {
	if ids.len == 0 {
		return []
	}
	user_lang := lang
	pets := sql conn {
		select from models.Pet
	}!
	translations := sql conn {
		select from models.PetTranslation where pet_id in ids && (lang == user_lang || lang == 'en')
	}!

	mut pet_map := map[int]models.Pet{}
	for pet in pets {
		if id := pet.pet_id {
			pet_map[id] = pet
		}
	}
	mut translation_map := map[int]models.PetTranslation{}
	for tr in translations {
		if tr.lang == user_lang {
			translation_map[tr.pet_id] = tr
		}
	}
	for tr in translations {
		if tr.pet_id !in translation_map {
			translation_map[tr.pet_id] = tr
		}
	}

	mut result := []PetView{}
	for id in ids {
		if pet := pet_map[id] {
			if tr := translation_map[id] {
				result << PetView{
					pet_id:       pet.pet_id or { 0 }
					image:        pet.image
					grade:        pet.grade
					release_date: pet.release_date
					lang:         tr.lang
					name:         tr.name
					abilities:    tr.abilities
					description:  tr.description
				}
			}
		}
	}
	return result
}

// treasures_by_ids is the batched counterpart of get_treasure for search
// results (effects are not loaded for search hits).
fn treasures_by_ids(conn sqlite.DB, lang string, ids []int) ![]TreasureView {
	if ids.len == 0 {
		return []
	}
	user_lang := lang
	treasures := sql conn {
		select from models.Treasure
	}!
	translations := sql conn {
		select from models.TreasureTranslation where treasure_id in ids
		&& (lang == user_lang || lang == 'en')
	}!

	mut treasure_map := map[int]models.Treasure{}
	for treasure in treasures {
		if id := treasure.treasure_id {
			treasure_map[id] = treasure
		}
	}
	mut translation_map := map[int]models.TreasureTranslation{}
	for tr in translations {
		if tr.lang == user_lang {
			translation_map[tr.treasure_id] = tr
		}
	}
	for tr in translations {
		if tr.treasure_id !in translation_map {
			translation_map[tr.treasure_id] = tr
		}
	}

	mut result := []TreasureView{}
	for id in ids {
		if treasure := treasure_map[id] {
			if tr := translation_map[id] {
				result << treasure_view(treasure, tr)
			}
		}
	}
	return result
}

// search_all returns up to `limit` matches per entity type, ranked by FTS5.
// Each type resolves with two batched queries (entity ids from FTS, then
// entities + translations in two IN queries), instead of the previous two
// queries per match, so the total stays flat no matter the limit.
pub fn search_all(conn sqlite.DB, lang string, q string, limit int) !SearchResults {
	mut results := SearchResults{}
	cookie_ids := fts_owner_ids(conn, 'cookie_translation_fts', 'cookie_translation',
		'cookie_translation_id', 'owner_id', q, limit)!
	results.cookies = cookies_by_ids(conn, lang, cookie_ids)!
	pet_ids := fts_owner_ids(conn, 'pet_translation_fts', 'pet_translation', 'pet_translation_id',
		'pet_id', q, limit)!
	results.pets = pets_by_ids(conn, lang, pet_ids)!
	treasure_ids := fts_owner_ids(conn, 'treasure_translation_fts', 'treasure_translation',
		'treasure_translation_id', 'treasure_id', q, limit)!
	results.treasures = treasures_by_ids(conn, lang, treasure_ids)!
	return results
} // EffectRowData is one editable treasure effect: a display name plus the raw

// numeric value and unit, ready for the admin form's structured editor.
pub struct EffectRowData {
pub:
	name  string
	value ?f32
	unit  models.EffectUnit
}

// treasure_effect_rows returns the treasure's effects for one state as
// editable rows, in listing order. Names resolve with the user's language
// first, then English, then any translation, so an effect that lacks the
// current language still shows up and survives a save. Duplicate links (the
// same effect twice) emit once, matching the detail page.
pub fn treasure_effect_rows(conn sqlite.DB, lang string, id int, st models.EffectState) ![]EffectRowData {
	links := sql conn {
		select from models.TreasureEffect where treasure_id == id && state == st order by treasure_effect_id
	}!
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

	translations := sql conn {
		select from models.EffectTranslation where effect_id in effect_ids
	}!
	// resolve each effect's name once: user lang > en > any translation
	mut name_map := map[int]string{}
	mut rank_map := map[int]int{}
	for tr in translations {
		if tr.name == '' {
			continue
		}
		rank := if tr.lang == lang {
			3
		} else if tr.lang == 'en' {
			2
		} else {
			1
		}
		if rank > (rank_map[tr.effect_id] or { 0 }) {
			rank_map[tr.effect_id] = rank
			name_map[tr.effect_id] = tr.name
		}
	}

	mut emitted := map[int]bool{}
	mut rows := []EffectRowData{}
	for link in links {
		if link.effect_id in emitted {
			continue
		}
		emitted[link.effect_id] = true
		name := name_map[link.effect_id] or { continue }
		rows << EffectRowData{
			name:  name
			value: link.value
			unit:  link.unit
		}
	}
	return rows
}

// effect_names lists the effect names translated in `lang`, deduplicated and
// sorted, for the admin form's name suggestions.
pub fn effect_names(conn sqlite.DB, lang string) ![]string {
	plang := lang
	translations := sql conn {
		select from models.EffectTranslation where lang == plang order by name
	}!
	mut seen := map[string]bool{}
	mut names := []string{}
	for tr in translations {
		n := tr.name.trim_space()
		if n != '' && n !in seen {
			names << n
			seen[n] = true
		}
	}
	return names
}

pub fn get_cookie(conn sqlite.DB, lang string, id int) !CookieView {
	if id <= 0 {
		return error('invalid cookie id')
	}

	cookies := sql conn {
		select from models.Cookie where cookie_id == id
	}!

	if cookies.len == 0 {
		return error('cookie (${id}) not found')
	}
	cookie := cookies.first()

	translations := backup_translations[models.CookieTranslation](conn, lang, owner_id: id)!

	return CookieView{
		cookie_id:              cookie.cookie_id or { 0 }
		image:                  cookie.image
		grade:                  cookie.grade
		release_date:           cookie.release_date
		lang:                   translations[id].lang
		name:                   translations[id].name
		abilities:              translations[id].abilities
		description:            translations[id].description
		power_plus:             translations[id].power_plus
		power_plus_requirement: translations[id].power_plus_requirement
		unlock_goal:            translations[id].unlock_goal
	}
}
