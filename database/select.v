module database

import db.sqlite
import log
import models
import time
import app.util

@[table: 'cookie']
pub struct CookieView {
pub:
	cookie_id    int
	image        ?string
	grade        models.Grade
	release_date time.Time

	lang                   string
	name                   string
	en_name                string // English name, for cross-language list filters
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

// warn_missing_id logs a corrupt row: ids on rows fetched with select are
// always present (serial keys are assigned at insert and never cleared), so a
// missing one can only happen on an unsaved/created row — or data corruption.
// Log it instead of silently dropping the row, which would hide the damage as
// a picker/list entry that's just gone.
fn warn_missing_id(kind string) {
	log.warn('${kind} row missing ${kind}_id — corrupt row?')
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
		cid := cookie.cookie_id or {
			warn_missing_id('cookie')
			continue
		}
		ids << cid
	}
	// an empty list would generate `owner_id IN ()` (a SQLite syntax error);
	// corrupt rows are all skipped below, same as the old full-table path
	if ids.len == 0 {
		return []
	}

	// One query fetches both the page's user-lang rows and their en fallbacks
	// (narrowed to the 30 ids on this page instead of the whole table); the en
	// names power the cross-language list filter: a th page still matches
	// English queries against the en names on each card.
	user_lang := lang
	translations := sql conn {
		select from models.CookieTranslation where owner_id in ids && (lang == user_lang || lang == 'en')
	}!

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
	mut en_name_map := map[int]string{}
	for tr in translations {
		if tr.lang == 'en' && tr.name != '' {
			en_name_map[tr.owner_id] = tr.name
		}
	}

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
				en_name:                en_name_map[cookie.cookie_id or { 0 }]
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
	en_name     string // English name, for cross-language list filters
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

	// narrow the translation fetch to the pets on this page instead of the
	// whole table (see select_cookies for the map-building pattern)
	user_lang := lang
	mut ids := []int{}
	for pet in pets {
		pid := pet.pet_id or {
			warn_missing_id('pet')
			continue
		}
		ids << pid
	}
	// an empty list would generate `pet_id IN ()` (a SQLite syntax error)
	if ids.len == 0 {
		return []
	}
	translations := sql conn {
		select from models.PetTranslation where pet_id in ids && (lang == user_lang || lang == 'en')
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

	// English names power the cross-language list filter (the en rows are
	// already in `translations`).
	mut en_name_map := map[int]string{}
	for tr in translations {
		if tr.lang == 'en' && tr.name != '' {
			en_name_map[tr.pet_id] = tr.name
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
				en_name:      en_name_map[pet.pet_id or { 0 }]
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
	is_power_plus    bool
	release_date     time.Time

	lang        string
	name        string
	en_name     string // English name, for cross-language list filters
	description string
}

// treasure_view maps a stored row + translation to the view handed to templates
fn treasure_view(t models.Treasure, tr models.TreasureTranslation, en_name string) TreasureView {
	return TreasureView{
		treasure_id:      t.treasure_id or { 0 }
		image:            t.image
		grade:            treasure_grade(t.grade)
		base_treasure_id: t.base_treasure_id
		unlock_cookie_id: t.unlock_cookie_id
		unlock_pet_id:    t.unlock_pet_id
		is_evolved:       t.is_evolved
		is_power_plus:    t.is_power_plus
		release_date:     t.release_date
		lang:             tr.lang
		name:             tr.name
		en_name:          en_name
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

// grade_rank maps a Grade to its sort position from grade_values (lowest to
// highest); treasures without a wiki grade rank below all graded ones.
fn grade_rank(g ?models.Grade) int {
	if v := g {
		for i, name in models.grade_values {
			if name == v.str() {
				return i
			}
		}
	}
	return -1
}

// compare_treasures orders the list by grade (highest first, ungraded last)
// and then by release date descending, with the name as a final tie-break.
fn compare_treasures(a &TreasureView, b &TreasureView) int {
	return compare_grade_date(a.grade, a.release_date, a.name, b.grade, b.release_date, b.name)
}

// compare_treasure_options applies the same grade-then-date ordering to the
// picker modal's IdNameOption list.
fn compare_treasure_options(a &IdNameOption, b &IdNameOption) int {
	return compare_grade_date(a.grade, a.release_date, a.name, b.grade, b.release_date, b.name)
}

fn compare_grade_date(ag ?models.Grade, ad time.Time, an string, bg ?models.Grade, bd time.Time, bn string) int {
	ar := grade_rank(ag)
	br := grade_rank(bg)
	if ar != br {
		if ar > br {
			return -1
		}
		return 1
	}
	if ad.unix() != bd.unix() {
		if ad.unix() > bd.unix() {
			return -1
		}
		return 1
	}
	if an < bn {
		return -1
	}
	if an > bn {
		return 1
	}
	return 0
}

// treasure_grade maps the stored int enum value back to the Grade enum;
// none when the treasure has no wiki grade (renders without a badge).
fn treasure_grade(g ?int) ?models.Grade {
	if v := g {
		return models.Grade.from(v) or { none }
	}
	return none
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
	return treasure_view(treasure, best_treasure_translation(conn, lang, id)!, '')
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
	return treasure_view(b, best_treasure_translation(conn, lang, base_id)!, '')
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
	return treasure_view(e, best_treasure_translation(conn, lang, e.treasure_id or { 0 })!, '')
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
	} else if kind == 'treasure' {
		rows := sql conn {
			select from models.Treasure where treasure_id == id
		} or { return none }
		if rows.len > 0 {
			img = rows.first().image
		}
	}
	return img
}

// entity_name_by_id returns the localized name of a cookie, pet, or treasure
// (user lang with en fallback); '' when the id doesn't exist or no
// translation covers it. The id-based rich-text [[kind:id]] renderer uses it
// so authored markup stays language-independent — the link text is resolved
// per language at render time instead of embedding a localized name.
pub fn entity_name_by_id(conn sqlite.DB, kind string, id int, lang string) string {
	return resolve_entity_name(conn, kind, id, lang)
}

fn resolve_entity_name(conn sqlite.DB, kind string, id int, lang string) string {
	user_lang := lang
	if kind == 'pet' {
		trs := sql conn {
			select from models.PetTranslation where pet_id == id && (lang == user_lang || lang == 'en')
		} or { return '' }
		if trs.len == 0 {
			return ''
		}
		mut tr := trs.first()
		for x in trs {
			if x.lang == user_lang {
				tr = x
				break
			}
		}
		return tr.name
	}
	if kind == 'treasure' {
		trs := sql conn {
			select from models.TreasureTranslation where treasure_id == id && (lang == user_lang || lang == 'en')
		} or { return '' }
		if trs.len == 0 {
			return ''
		}
		mut tr := trs.first()
		for x in trs {
			if x.lang == user_lang {
				tr = x
				break
			}
		}
		return tr.name
	}
	trs := sql conn {
		select from models.CookieTranslation where owner_id == id && (lang == user_lang || lang == 'en')
	} or { return '' }
	if trs.len == 0 {
		return ''
	}
	mut tr := trs.first()
	for x in trs {
		if x.lang == user_lang {
			tr = x
			break
		}
	}
	return tr.name
}

// combi_effect_text returns the localized name of a combo bonus's linked
// effect (user lang, en fallback, '' when the combo has no effect).
fn combi_effect_text(conn sqlite.DB, effect_id ?int, lang string) string {
	eid := effect_id or { return '' }
	user_lang := lang
	trs := sql conn {
		select from models.EffectTranslation where effect_id == eid && (lang == user_lang || lang == 'en')
	} or { return '' }
	for tr in trs {
		if tr.lang == user_lang {
			return tr.name
		}
	}
	if trs.len > 0 {
		return trs.first().name
	}
	return ''
}

// combi_bonus_id_for returns the combi_bonus row id for a cookie+pet pair,
// or 0 when the pair has no combo bonus. Called on build insert/update so the
// snapshot column stays in sync with the pair's current combo.
pub fn combi_bonus_id_for(conn sqlite.DB, cid int, pid int) !int {
	rows := sql conn {
		select from models.CombiBonus where cookie_id == cid && pet_id == pid
	}!
	if rows.len == 0 {
		return 0
	}
	if id := rows[0].id {
		return id
	}
	warn_missing_id('combi_bonus')
	return 0
}

// CombiBonusView is one combo-bonus row rendered on a cookie/pet detail page:
// the partner entity (the other half of the pair) plus the localized bonus
// effect text.
pub struct CombiBonusView {
pub:
	partner_kind  string // 'cookie' | 'pet'
	partner_id    int
	partner_name  string
	partner_image ?string
	effect        string
	is_hidden     bool
}

// get_combi_bonus returns the combo bonuses involving `kind` (cookie or pet)
// with id `id`, resolving each partner's name and the effect text in the
// user's language (en fallback). Hidden rows are listed too — this is a wiki,
// readers see everything. Empty when the entity has no combos.
// combo_lookups pre-fetches everything get_combi_bonus / combi_edit_rows need
// per row — partner name + sprite and the effect text — in 4 batched queries
// instead of 3 per combo row.
fn combo_lookups(conn sqlite.DB, lang string, partner_kind string, partner_ids []int, effect_ids []int) (map[int]string, map[int]?string, map[int]string) {
	mut name_map := map[int]string{}
	mut img_map := map[int]?string{}
	user_lang := lang

	if partner_kind == 'pet' {
		trs := sql conn {
			select from models.PetTranslation where pet_id in partner_ids
			&& (lang == user_lang || lang == 'en')
		} or { [] }
		for tr in trs {
			if tr.lang == user_lang {
				name_map[tr.pet_id] = tr.name
			}
		}
		for tr in trs {
			if tr.pet_id !in name_map {
				name_map[tr.pet_id] = tr.name
			}
		}
		id_csv := partner_ids.map(it.str()).join(',')
		rows := conn.exec('SELECT pet_id, image FROM pet WHERE pet_id IN (${id_csv})') or { [] }
		for row in rows {
			// get_string maps SQL NULL to ''; map that to none so the
			// placeholder URL fires (same pattern as images_by_ids).
			img := row.get_string('image')
			img_map[row.get_int('pet_id')] = if img == '' { none } else { img }
		}
	} else {
		trs := sql conn {
			select from models.CookieTranslation where owner_id in partner_ids
			&& (lang == user_lang || lang == 'en')
		} or { [] }
		for tr in trs {
			if tr.lang == user_lang {
				name_map[tr.owner_id] = tr.name
			}
		}
		for tr in trs {
			if tr.owner_id !in name_map {
				name_map[tr.owner_id] = tr.name
			}
		}
		id_csv := partner_ids.map(it.str()).join(',')
		rows := conn.exec('SELECT cookie_id, image FROM cookie WHERE cookie_id IN (${id_csv})') or { [] }
		for row in rows {
			// get_string maps SQL NULL to ''; map that to none so the
			// placeholder URL fires (same pattern as images_by_ids).
			img := row.get_string('image')
			img_map[row.get_int('cookie_id')] = if img == '' { none } else { img }
		}
	}

	mut effect_map := map[int]string{}
	if effect_ids.len > 0 {
		trs := sql conn {
			select from models.EffectTranslation where effect_id in effect_ids
			&& (lang == user_lang || lang == 'en')
		} or { [] }
		for tr in trs {
			if tr.lang == user_lang {
				effect_map[tr.effect_id] = tr.name
			}
		}
		for tr in trs {
			if tr.effect_id !in effect_map {
				effect_map[tr.effect_id] = tr.name
			}
		}
	}
	return name_map, img_map, effect_map
}

pub fn get_combi_bonus(conn sqlite.DB, lang string, kind string, id int) ![]CombiBonusView {
	mut rows := []models.CombiBonus{}
	if kind == 'cookie' {
		rows = sql conn {
			select from models.CombiBonus where cookie_id == id
		}!
	} else if kind == 'pet' {
		rows = sql conn {
			select from models.CombiBonus where pet_id == id
		}!
	} else {
		return error('invalid combi kind')
	}
	if rows.len == 0 {
		return []
	}

	partner_kind := if kind == 'cookie' { 'pet' } else { 'cookie' }
	mut partner_ids := []int{}
	mut effect_ids := []int{}
	mut seen := map[int]bool{}
	mut seen_e := map[int]bool{}
	for row in rows {
		pid := if kind == 'cookie' { row.pet_id } else { row.cookie_id }
		if pid !in seen {
			partner_ids << pid
			seen[pid] = true
		}
		if eid := row.effect_id {
			if eid !in seen_e {
				effect_ids << eid
				seen_e[eid] = true
			}
		}
	}
	name_map, img_map, effect_map := combo_lookups(conn, lang, partner_kind, partner_ids, effect_ids)

	mut result := []CombiBonusView{}
	for row in rows {
		pid := if kind == 'cookie' { row.pet_id } else { row.cookie_id }
		name := name_map[pid] or { '' }
		if name == '' {
			continue
		}
		result << CombiBonusView{
			partner_kind:  partner_kind
			partner_id:    pid
			partner_name:  name
			partner_image: img_map[pid] or { none }
			effect:        effect_map[row.effect_id or { 0 }] or { '' }
			is_hidden:     row.is_hidden
		}
	}
	return result
}

// CombiEditRow is one combo bonus of a cookie/pet rendered on the cookie/pet
// admin form: the partner entity (the other half of the pair) plus the
// editable effect name (resolved for the form's language) and hidden flag.
pub struct CombiEditRow {
pub:
	id            int
	partner_id    int
	partner_name  string
	partner_image ?string
	effect        string
	is_hidden     bool
}

// combi_edit_rows returns the combo bonuses of `kind` (cookie or pet) with id
// `id` for the admin edit form, resolving the partner name/sprite and the
// effect text in the given language (en fallback).
pub fn combi_edit_rows(conn sqlite.DB, lang string, kind string, id int) ![]CombiEditRow {
	mut rows := []models.CombiBonus{}
	if kind == 'cookie' {
		rows = sql conn {
			select from models.CombiBonus where cookie_id == id
		}!
	} else if kind == 'pet' {
		rows = sql conn {
			select from models.CombiBonus where pet_id == id
		}!
	} else {
		return error('invalid combi kind')
	}
	if rows.len == 0 {
		return []
	}

	partner_kind := if kind == 'cookie' { 'pet' } else { 'cookie' }
	mut partner_ids := []int{}
	mut effect_ids := []int{}
	mut seen := map[int]bool{}
	mut seen_e := map[int]bool{}
	for row in rows {
		pid := if kind == 'cookie' { row.pet_id } else { row.cookie_id }
		if pid !in seen {
			partner_ids << pid
			seen[pid] = true
		}
		if eid := row.effect_id {
			if eid !in seen_e {
				effect_ids << eid
				seen_e[eid] = true
			}
		}
	}
	name_map, img_map, effect_map := combo_lookups(conn, lang, partner_kind, partner_ids, effect_ids)

	mut result := []CombiEditRow{}
	for row in rows {
		pid := if kind == 'cookie' { row.pet_id } else { row.cookie_id }
		rid := row.id or { continue }
		result << CombiEditRow{
			id:            rid
			partner_id:    pid
			partner_name:  name_map[pid] or { '' }
			partner_image: img_map[pid] or { none }
			effect:        effect_map[row.effect_id or { 0 }] or { '' }
			is_hidden:     row.is_hidden
		}
	}
	return result
}

// EffectOption is one treasure effect line for the build planner picker:
// the display text ("gives extra points for Jellies") plus its compact value
// ("2-4", "12%", ""). Both come from split_effect_value/compact_effect_value
// so the picker text matches the treasure detail page.
pub struct EffectOption {
pub:
	text  string
	value string
}

// IdNameOption is a lightweight (id, name) pair for admin-form dropdowns;
// image is populated for treasure options so the combobox can show sprites,
// and the effect fields carry the treasure's full normal + blessed effect
// lists (in wiki order, deduped) for the build planner picker. Empty for
// cookies/pets. has_blessed_toggle is true when the blessed set differs from
// the normal set, so the picker can offer a normal/blessed toggle.
pub struct IdNameOption {
pub:
	id                 int
	name               string
	en_name            string // English name, for cross-language picker search
	image              ?string
	effects            []EffectOption
	effects_blessed    []EffectOption
	has_blessed_toggle bool
	is_evolved         bool // treasures only: evolved rows have an evo base
	grade              ?models.Grade // treasures only: sort rank for the picker
	release_date       time.Time // treasures only: sort rank for the picker
}

// BuildCard is one community-submitted build on the /builds list: the EP
// tier, the five entities, the tag, and the submitter info. Anonymous builds
// carry the remaining lifetime so the list can render an expiry badge.
pub struct BuildCard {
pub:
	build_id     int
	ep           int
	ep_special   int
	tags         []string
	boosts       []string
	boost        string // purchased pre-run boost key ('' = none)
	power_effects []string // owned Power+ effect keys marked used
	score        u64
	coin         u64
	time         u64
	boxes        u64
	description  string
	youtube_url  string
	author       string
	user_id      int // owning user id; 0 for anonymous builds
	is_anon      bool
	expires_in_h int // remaining hours for anonymous builds, 0 for permanent
	created_at   time.Time
	cookie       IdNameOption
	cookie2      ?IdNameOption // relay cookie; none when the build has no relay
	pet             IdNameOption
	treasures       []IdNameOption
	treasure_blessed []bool // per-slot blessed state, aligned with treasures
	combi_bonus_id   int    // combi_bonus row id for the cookie+pet pair; 0 = no combo
}

// select_builds returns non-expired community builds, filtered by
// cookie/pet/treasure/EP tier and sorted by score/coin/time/latest
// (score/coin DESC, time ASC — fastest run first, latest by created_at),
// paginated. Expired anonymous builds are excluded. Raw SQL because
// the expiry filter combines `IS NULL` with a unix-timestamp comparison;
// time.Time is stored as a unix int by the ORM.
pub fn select_builds(conn sqlite.DB, lang string, f_cookie int, f_pet int, f_treasure int, f_ep int, f_ep_special int, sort string, limit int, offset int) ![]BuildCard {
	mut where := '1=1'
	if f_cookie > 0 {
		where += ' AND cookie_id = ${f_cookie}'
	}
	if f_pet > 0 {
		where += ' AND pet_id = ${f_pet}'
	}
	if f_treasure > 0 {
		where += ' AND (treasure1_id = ${f_treasure} OR treasure2_id = ${f_treasure} OR treasure3_id = ${f_treasure})'
	}
	if f_ep > 0 {
		where += ' AND ep = ${f_ep} AND ep_special = 0'
	}
	if f_ep_special > 0 {
		where += ' AND ep_special = ${f_ep_special}'
	}
	where += ' AND (expires_at IS NULL OR expires_at > ${time.now().unix()})'
	order := match sort {
		'score' { 'score DESC, build_id DESC' }
		'coin' { 'coin DESC, build_id DESC' }
		'time' { 'time ASC, build_id DESC' } // fastest run first (duration ms)
		else { 'created_at DESC, build_id DESC' } // latest
	}

	rows := conn.exec('SELECT build_id, cookie_id, cookie2_id, pet_id, combi_bonus_id, treasure1_id, treasure2_id, treasure3_id, treasure1_blessed, treasure2_blessed, treasure3_blessed, ep, ep_special, tag, boosts, boost, power_effects, score, coin, time, boxes, description, youtube_url, author, user_id, expires_at, created_at FROM build WHERE ${where} ORDER BY ${order} LIMIT ${limit} OFFSET ${offset}')!
	if rows.len == 0 {
		return []
	}

	mut result := []BuildCard{}
	lookups := build_card_lookups(conn, lang, rows)
	for row in rows {
		result << build_card_from_row(row, lookups)
	}
	return result
}
// BuildCardLookups holds the localized names and sprites every card on a
// build list/detail page needs, resolved once for the whole page instead of
// two queries per entity per card (30 cards × 6 entities = ~360 queries).
pub struct BuildCardLookups {
pub:
	cookie_names    map[int]string
	cookie_images   map[int]?string
	pet_names       map[int]string
	pet_images      map[int]?string
	treasure_names  map[int]string
	treasure_images map[int]?string
}

// build_card_lookups pre-fetches the localized names and sprites for every
// cookie/pet/treasure id the given build rows reference — 6 batched queries
// total (name + image per kind), independent of the row count.
fn build_card_lookups(conn sqlite.DB, lang string, rows []sqlite.Row) BuildCardLookups {
	mut cookie_ids := []int{}
	mut pet_ids := []int{}
	mut treasure_ids := []int{}
	mut seen_c := map[int]bool{}
	mut seen_p := map[int]bool{}
	mut seen_t := map[int]bool{}
	for row in rows {
		cid := row.get_int('cookie_id')
		if cid > 0 && cid !in seen_c {
			cookie_ids << cid
			seen_c[cid] = true
		}
		cid2 := row.get_int('cookie2_id')
		if cid2 > 0 && cid2 !in seen_c {
			cookie_ids << cid2
			seen_c[cid2] = true
		}
		pid := row.get_int('pet_id')
		if pid > 0 && pid !in seen_p {
			pet_ids << pid
			seen_p[pid] = true
		}
		for tid in [row.get_int('treasure1_id'), row.get_int('treasure2_id'), row.get_int('treasure3_id')] {
			if tid > 0 && tid !in seen_t {
				treasure_ids << tid
				seen_t[tid] = true
			}
		}
	}

	user_lang := lang
	mut cookie_names := map[int]string{}
	if cookie_ids.len > 0 {
		trs := sql conn {
			select from models.CookieTranslation where owner_id in cookie_ids
			&& (lang == user_lang || lang == 'en')
		} or { [] }
		for tr in trs {
			if tr.lang == user_lang {
				cookie_names[tr.owner_id] = tr.name
			}
		}
		for tr in trs {
			if tr.owner_id !in cookie_names {
				cookie_names[tr.owner_id] = tr.name
			}
		}
	}
	mut pet_names := map[int]string{}
	if pet_ids.len > 0 {
		trs := sql conn {
			select from models.PetTranslation where pet_id in pet_ids
			&& (lang == user_lang || lang == 'en')
		} or { [] }
		for tr in trs {
			if tr.lang == user_lang {
				pet_names[tr.pet_id] = tr.name
			}
		}
		for tr in trs {
			if tr.pet_id !in pet_names {
				pet_names[tr.pet_id] = tr.name
			}
		}
	}
	mut treasure_names := map[int]string{}
	if treasure_ids.len > 0 {
		trs := sql conn {
			select from models.TreasureTranslation where treasure_id in treasure_ids
			&& (lang == user_lang || lang == 'en')
		} or { [] }
		for tr in trs {
			if tr.lang == user_lang {
				treasure_names[tr.treasure_id] = tr.name
			}
		}
		for tr in trs {
			if tr.treasure_id !in treasure_names {
				treasure_names[tr.treasure_id] = tr.name
			}
		}
	}

	return BuildCardLookups{
		cookie_names:    cookie_names
		cookie_images:   images_by_ids(conn, 'cookie', 'cookie_id', cookie_ids)
		pet_names:       pet_names
		pet_images:      images_by_ids(conn, 'pet', 'pet_id', pet_ids)
		treasure_names:  treasure_names
		treasure_images: images_by_ids(conn, 'treasure', 'treasure_id', treasure_ids)
	}
}

// images_by_ids fetches the sprite column for the given ids in one query;
// ids without a row (or a NULL image) map to none for the placeholder URL.
fn images_by_ids(conn sqlite.DB, table string, id_col string, ids []int) map[int]?string {
	mut img_map := map[int]?string{}
	if ids.len == 0 {
		return img_map
	}
	id_csv := ids.map(it.str()).join(',')
	rows := conn.exec('SELECT ${id_col}, image FROM ${table} WHERE ${id_col} IN (${id_csv})') or { return img_map }
	for row in rows {
		// get_string maps SQL NULL to ''; map that to none so the template's
		// placeholder URL fires (matching the ?string rows the ORM returns).
		img := row.get_string('image')
		img_map[row.get_int(id_col)] = if img == '' { none } else { img }
	}
	return img_map
}

// build_card_from_row maps one build row (the exact column set select_builds
// and select_build select) to the card/detail view, resolving localized
// entity names from the page-level lookups (see build_card_lookups).
fn build_card_from_row(row sqlite.Row, lookups BuildCardLookups) BuildCard {
	// user_id is NULL for anonymous submissions; ids start at 1.
	is_anon := row.get_int('user_id') == 0
	exp_unix := row.get_int('expires_at')
	expires_in_h := if is_anon && exp_unix > 0 {
		int((exp_unix - time.now().unix() + 3599) / 3600)
	} else {
		0
	}
	cid := row.get_int('cookie_id')
	cid2 := row.get_int('cookie2_id')
	pid := row.get_int('pet_id')
	tids := [row.get_int('treasure1_id'), row.get_int('treasure2_id'), row.get_int('treasure3_id')]
	mut treasures := []IdNameOption{}
	for tid in tids {
		treasures << IdNameOption{
			id:    tid
			name:  lookups.treasure_names[tid] or { '' }
			image: lookups.treasure_images[tid] or { none }
		}
	}
	treasure_blessed := [row.get_int('treasure1_blessed') != 0, row.get_int('treasure2_blessed') != 0,
		row.get_int('treasure3_blessed') != 0]
	return BuildCard{
		build_id:     row.get_int('build_id')
		ep:           row.get_int('ep')
		ep_special:   row.get_int('ep_special')
		tags:         row.get_string('tag').split(',').filter(it != '')
		boosts:       row.get_string('boosts').split(',').filter(it != '')
		boost:        row.get_string('boost')
		power_effects: row.get_string('power_effects').split(',').filter(it != '')
		score:        u64(row.get_int('score'))
		coin:         u64(row.get_int('coin'))
		time:         u64(row.get_int('time'))
		boxes:        u64(row.get_int('boxes'))
		description:  row.get_string('description')
		youtube_url:  row.get_string('youtube_url')
		author:       row.get_string('author')
		user_id:      row.get_int('user_id')
		is_anon:      is_anon
		expires_in_h: expires_in_h
		created_at:   time.unix(i64(row.get_int('created_at')))
		cookie:       IdNameOption{
			id:    cid
			name:  lookups.cookie_names[cid] or { '' }
			image: lookups.cookie_images[cid] or { none }
		}
		cookie2: if cid2 > 0 {
			IdNameOption{
				id:    cid2
				name:  lookups.cookie_names[cid2] or { '' }
				image: lookups.cookie_images[cid2] or { none }
			}
		} else {
			none
		}
		pet: IdNameOption{
			id:    pid
			name:  lookups.pet_names[pid] or { '' }
			image: lookups.pet_images[pid] or { none }
		}
		treasures:       treasures
		treasure_blessed: treasure_blessed
		combi_bonus_id:  row.get_int('combi_bonus_id')
	}
}

// select_build returns one community build by id (including expired ones — a
// direct link should still render), or an error when it doesn't exist. Used
// by the /builds/:id detail page the list cards link to.
pub fn select_build(conn sqlite.DB, lang string, id int) !BuildCard {
	rows := conn.exec('SELECT build_id, cookie_id, cookie2_id, pet_id, combi_bonus_id, treasure1_id, treasure2_id, treasure3_id, treasure1_blessed, treasure2_blessed, treasure3_blessed, ep, ep_special, tag, boosts, boost, power_effects, score, coin, time, boxes, description, youtube_url, author, user_id, expires_at, created_at FROM build WHERE build_id = ${id}')!
	if rows.len == 0 {
		return error('build (${id}) not found')
	}
	lookups := build_card_lookups(conn, lang, rows)
	return build_card_from_row(rows.first(), lookups)
}

// build_review_counts returns the verified and issue totals for one build,
// for the detail page's verdict summary. The UNIQUE(build_id, user_id)
// auto-index serves the by-build lookup (leftmost column).
pub fn build_review_counts(conn sqlite.DB, build_id int) (int, int) {
	rows := sql conn {
		select from models.BuildReview where build_id == build_id
	} or { return 0, 0 }
	mut verified := 0
	mut issues := 0
	for r in rows {
		if r.verified {
			verified++
		} else {
			issues++
		}
	}
	return verified, issues
}

// get_build_review returns the current user's verdict on a build, or none
// when they haven't reviewed it yet. Used to render the detail page's
// "your verdict" state and pre-fill the change form.
pub fn get_build_review(conn sqlite.DB, build_id int, user_id int) ?models.BuildReview {
	rows := sql conn {
		select from models.BuildReview where build_id == build_id && user_id == user_id limit 1
	} or { return none }
	if rows.len == 0 {
		return none
	}
	return rows.first()
}

// BuildIssueView is one reported issue on a build detail page: the reason
// text plus the reporter's username ('' when the user row is gone).
pub struct BuildIssueView {
pub:
	reason string
	author string
}

// build_review_issues returns the reported issues on a build (reason +
// reporter username), newest first. The reasons are public — a visitor needs
// to see why a build is flagged, not just that it is — so the detail page
// lists them under the issue count.
pub fn build_review_issues(conn sqlite.DB, build_id int) []BuildIssueView {
	rows := conn.exec('SELECT r.reason, u.username FROM build_review r LEFT JOIN user u ON u.user_id = r.user_id WHERE r.build_id = ${build_id} AND r.verified = 0 ORDER BY r.updated_at DESC, r.build_review_id DESC') or {
		return []
	}
	mut out := []BuildIssueView{}
	for row in rows {
		reason := row.get_string('reason').trim_space()
		if reason == '' {
			continue
		}
		out << BuildIssueView{
			reason: reason
			author: row.get_string('username')
		}
	}
	return out
}

// treasure_options lists every treasure's id, localized name and first
// normal effect text (the picker shows effects in the modal and on slots) for
// the build planner and the cookie/pet admin forms' "unlocks treasure"
// selector. Effects resolve in two batched queries, mirroring the detail
// page's value-splitting so the picker text matches the wiki card.
pub fn treasure_options(conn sqlite.DB, lang string, equippable bool) ![]IdNameOption {
	// Power+ treasures are friendly-run bonus items that cannot be equipped:
	// the build pickers exclude them at the query level, while the admin
	// unlock combobox (equippable=false) keeps the full list. Explicit
	// branches keep the WHERE clause non-empty — a `dynamic select` guard
	// that's false at runtime emits `WHERE ;` (a syntax error).
	treasures := if equippable {
		sql conn {
			select from models.Treasure where is_power_plus == false
		}!
	} else {
		sql conn {
			select from models.Treasure
		}!
	}
	// fetch only the translations/links for the treasure set about to be
	// rendered, not the whole catalog: both tables carry a plain `treasure_id
	// int` (unlike models.Treasure's optional id), so the ORM `in` works —
	// same pattern as treasures_by_ids.
	user_lang := lang
	mut tids := []int{}
	for t in treasures {
		if tid := t.treasure_id {
			tids << tid
		} else {
			warn_missing_id('treasure')
		}
	}
	translations := sql conn {
		select from models.TreasureTranslation where treasure_id in tids
		&& (lang == user_lang || lang == 'en')
	}!
	mut tmap := map[int]models.TreasureTranslation{}
	for tr in translations {
		if tr.lang == user_lang {
			tmap[tr.treasure_id] = tr
		}
	}
	for tr in translations {
		if tr.treasure_id !in tmap {
			tmap[tr.treasure_id] = tr
		}
	}

	// English names power cross-language search in the picker modal (the en
	// rows are already in `translations`).
	mut en_name_map := map[int]string{}
	for tr in translations {
		if tr.lang == 'en' && tr.name != '' {
			en_name_map[tr.treasure_id] = tr.name
		}
	}

	// every effect per treasure per state (normal + blessed), in wiki order
	// with duplicate links dropped; the picker shows them all and toggles
	// between the states for evolved treasures.
	mut effect_map := map[int][]EffectOption{}
	mut blessed_map := map[int][]EffectOption{}
	links := sql conn {
		select from models.TreasureEffect where treasure_id in tids order by treasure_effect_id
	} or { [] }
	if links.len > 0 {
		mut eids := []int{}
		mut seen := map[int]bool{}
		for l in links {
			if l.effect_id !in seen {
				eids << l.effect_id
				seen[l.effect_id] = true
			}
		}
		trs := sql conn {
			select from models.EffectTranslation where effect_id in eids
			&& (lang == user_lang || lang == 'en')
		} or { [] }
		mut etmap := map[int]models.EffectTranslation{}
		for tr in trs {
			if tr.lang == user_lang {
				etmap[tr.effect_id] = tr
			}
		}
		for tr in trs {
			if tr.effect_id !in etmap {
				etmap[tr.effect_id] = tr
			}
		}
		mut seen_tid := map[int]map[int]bool{} // treasure_id -> effect_id dedupe
		mut seen_tid_b := map[int]map[int]bool{}
		for l in links {
			if tr := etmap[l.effect_id] {
				v0, v9, text := util.split_effect_value(l, tr.name)
				opt := EffectOption{
					text:  text
					value: util.compact_effect_value(v0, v9)
				}
				if l.state == models.EffectState.blessed {
					if l.effect_id in (seen_tid_b[l.treasure_id] or { map[int]bool{} }) {
						continue
					}
					if l.treasure_id !in seen_tid_b {
						seen_tid_b[l.treasure_id] = map[int]bool{}
					}
					seen_tid_b[l.treasure_id][l.effect_id] = true
					blessed_map[l.treasure_id] << opt
				} else {
					if l.effect_id in (seen_tid[l.treasure_id] or { map[int]bool{} }) {
						continue
					}
					if l.treasure_id !in seen_tid {
						seen_tid[l.treasure_id] = map[int]bool{}
					}
					seen_tid[l.treasure_id][l.effect_id] = true
					effect_map[l.treasure_id] << opt
				}
			}
		}
	}

	mut out := []IdNameOption{}
	for t in treasures {
		tid := t.treasure_id or { continue }
		if tr := tmap[tid] {
			normal := effect_map[tid] or { []EffectOption{} }
			blessed := blessed_map[tid] or { []EffectOption{} }
			out << IdNameOption{
				id:                 tid
				name:               tr.name
				en_name:            en_name_map[tid]
				image:              t.image
				effects:            normal
				effects_blessed:    blessed
				has_blessed_toggle: blessed.len > 0 && normal.len > 0 && effect_lists_differ(normal, blessed)
				is_evolved:         t.is_evolved
				grade:              treasure_grade(t.grade)
				release_date:       t.release_date
			}
		}
	}
	// match the /treasures list order: grade (highest first) then newest
	// release date, so the picker modal presents the same sequence
	out.sort_with_compare(compare_treasure_options)
	return out
}

// normal_treasure_options lists non-evolved treasures for the admin form's
// "base treasure" selector: evolved rows link back to their normal form, and
// the base must itself be a normal treasure (never evolved, never Power+ —
// Power+ treasures are friendly-run bonuses that cannot be equipped).
pub fn normal_treasure_options(conn sqlite.DB, lang string) ![]IdNameOption {
	mut out := []IdNameOption{}
	for opt in treasure_options(conn, lang, true)! {
		if !opt.is_evolved {
			out << opt
		}
	}
	return out
}

// cookie_options lists every cookie's id and localized name for the treasure
// admin form's "unlocked by cookie" selector.
pub fn cookie_options(conn sqlite.DB, lang string) ![]IdNameOption {
	cookies := sql conn {
		select from models.Cookie
	}!
	user_lang := lang
	translations := sql conn {
		select from models.CookieTranslation where lang == user_lang || lang == 'en'
	}!
	mut tmap := map[int]models.CookieTranslation{}
	for tr in translations {
		if tr.lang == user_lang {
			tmap[tr.owner_id] = tr
		}
	}
	for tr in translations {
		if tr.owner_id !in tmap {
			tmap[tr.owner_id] = tr
		}
	}

	// English names power cross-language search in the picker modal (the en
	// rows are already in `translations`).
	mut en_name_map := map[int]string{}
	for tr in translations {
		if tr.lang == 'en' && tr.name != '' {
			en_name_map[tr.owner_id] = tr.name
		}
	}

	mut out := []IdNameOption{}
	for c in cookies {
		cid := c.cookie_id or { continue }
		if tr := tmap[cid] {
			out << IdNameOption{
				id:      cid
				name:    tr.name
				en_name: en_name_map[cid]
				image:   c.image
			}
		}
	}
	// match the /cookies list order (newest id first) so the picker modal
	// presents the same sequence as the catalog page
	out.sort_with_compare(fn (a &IdNameOption, b &IdNameOption) int {
		if a.id > b.id {
			return -1
		}
		if a.id < b.id {
			return 1
		}
		return 0
	})
	return out
}

// pet_options lists every pet's id and localized name for the treasure admin
// form's "unlocked by pet" selector.
pub fn pet_options(conn sqlite.DB, lang string) ![]IdNameOption {
	pets := sql conn {
		select from models.Pet
	}!
	user_lang := lang
	translations := sql conn {
		select from models.PetTranslation where lang == user_lang || lang == 'en'
	}!
	mut tmap := map[int]models.PetTranslation{}
	for tr in translations {
		if tr.lang == user_lang {
			tmap[tr.pet_id] = tr
		}
	}
	for tr in translations {
		if tr.pet_id !in tmap {
			tmap[tr.pet_id] = tr
		}
	}

	// English names power cross-language search in the picker modal (the en
	// rows are already in `translations`).
	mut en_name_map := map[int]string{}
	for tr in translations {
		if tr.lang == 'en' && tr.name != '' {
			en_name_map[tr.pet_id] = tr.name
		}
	}

	mut out := []IdNameOption{}
	for p in pets {
		pid := p.pet_id or { continue }
		if tr := tmap[pid] {
			out << IdNameOption{
				id:      pid
				name:    tr.name
				en_name: en_name_map[pid]
				image: p.image
			}
		}
	}
	// match the /pets list order (newest id first) so the picker modal
	// presents the same sequence as the catalog page
	out.sort_with_compare(fn (a &IdNameOption, b &IdNameOption) int {
		if a.id > b.id {
			return -1
		}
		if a.id < b.id {
			return 1
		}
		return 0
	})
	return out
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
	return treasure_view(t, best_treasure_translation(conn, lang, t.treasure_id or { 0 })!, '')
}

// effects_from_links resolves effect translations and formats each link into
// an EffectView, preserving link order and dropping duplicate effects
fn effects_from_links(conn sqlite.DB, lang string, links []models.TreasureEffect) ![]util.EffectView {
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
	mut result := []util.EffectView{}
	for link in links {
		if link.effect_id in emitted {
			continue
		}
		emitted[link.effect_id] = true
		if tr := translation_map[link.effect_id] {
			v0, v9, text := util.split_effect_value(link, tr.name)
			result << util.EffectView{
				effect_id: link.effect_id
				name:      text
				value0:    v0
				value9:    v9
			}
		}
	}
	return result
}

// effects_by_state returns the treasure's effects for one state (normal or
// blessed), in the order the wiki listed them (treasure_effect_id order).
fn effects_by_state(conn sqlite.DB, lang string, id int, st models.EffectState) ![]util.EffectView {
	links := sql conn {
		select from models.TreasureEffect where treasure_id == id && state == st order by treasure_effect_id
	}!
	return effects_from_links(conn, lang, links)
}

// get_treasure_effects returns one row per distinct normal effect of the
// treasure, in the order the wiki listed them.
pub fn get_treasure_effects(conn sqlite.DB, lang string, id int) ![]util.EffectView {
	return effects_by_state(conn, lang, id, models.EffectState.normal)
}

// get_treasure_blessed_effects returns the blessed-state effects of an
// evolved treasure, in the order the wiki listed them; empty when the
// treasure has no blessed form
pub fn get_treasure_blessed_effects(conn sqlite.DB, lang string, id int) ![]util.EffectView {
	return effects_by_state(conn, lang, id, models.EffectState.blessed)
}

// effect_lists_differ reports whether two effect lists differ in any text or
// value at the same position, or in length — the build planner toggle only
// shows when the blessed set actually changes something visible.
fn effect_lists_differ(a []EffectOption, b []EffectOption) bool {
	if a.len != b.len {
		return true
	}
	for i in 0 .. a.len {
		if a[i].text != b[i].text || a[i].value != b[i].value {
			return true
		}
	}
	return false
}

// select_treasures lists treasures grade-first then newest; `tab` filters to
// normal/evolved rows at the query level ('all' returns both).
pub fn select_treasures(conn sqlite.DB, lang string, limit int, offset int, tab string) ![]TreasureView {
	// the tab filter is pushed into the query so the server never loads the
	// rows it would only discard (613 rows on every /treasures request)
	treasures := if tab == 'normal' {
		sql conn {
			select from models.Treasure where is_evolved == false
		}!
	} else if tab == 'evo' {
		sql conn {
			select from models.Treasure where is_evolved == true
		}!
	} else {
		sql conn {
			select from models.Treasure
		}!
	}

	if treasures.len == 0 {
		return []
	}

	// narrow the translation fetch to the tab-filtered set (the ORM `in`
	// works here — TreasureTranslation carries a plain `treasure_id int`).
	user_lang := lang
	mut tids := []int{}
	for t in treasures {
		if tid := t.treasure_id {
			tids << tid
		} else {
			warn_missing_id('treasure')
		}
	}
	translations := sql conn {
		select from models.TreasureTranslation where treasure_id in tids
		&& (lang == user_lang || lang == 'en')
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

	// English names power the cross-language list filter (the en rows are
	// already in `translations`).
	mut en_name_map := map[int]string{}
	for tr in translations {
		if tr.lang == 'en' && tr.name != '' {
			en_name_map[tr.treasure_id] = tr.name
		}
	}

	mut result := []TreasureView{}

	for treasure in treasures {
		if tr := translation_map[treasure.treasure_id or { continue }] {
			result << treasure_view(treasure, tr, en_name_map[treasure.treasure_id or { 0 }])
		}
	}

	result.sort_with_compare(compare_treasures)

	// paginate after the in-memory sort (grade first, newest release first)
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
		} else {
			warn_missing_id('cookie')
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
		} else {
			warn_missing_id('pet')
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
		} else {
			warn_missing_id('treasure')
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
				result << treasure_view(treasure, tr, '')
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
	name      string
	value     ?f64
	value_min ?f64
	value_max ?f64
	unit      models.EffectUnit
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
			name:      name
			value:     link.value
			value_min: link.value_min
			value_max: link.value_max
			unit:      link.unit
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

// collect_translation_opts appends every (id, name) translation row of one
// kind for a single language.
fn collect_translation_opts(conn sqlite.DB, kind string, rows_lang string, mut opts []IdNameOption) {
	if kind == 'pet' {
		rows := sql conn {
			select from models.PetTranslation where lang == rows_lang
		} or { return }
		for r in rows {
			n := r.name.trim_space()
			if n != '' {
				opts << IdNameOption{id: r.pet_id, name: n}
			}
		}
	} else if kind == 'treasure' {
		rows := sql conn {
			select from models.TreasureTranslation where lang == rows_lang
		} or { return }
		for r in rows {
			n := r.name.trim_space()
			if n != '' {
				opts << IdNameOption{id: r.treasure_id, name: n}
			}
		}
	} else {
		rows := sql conn {
			select from models.CookieTranslation where lang == rows_lang
		} or { return }
		for r in rows {
			n := r.name.trim_space()
			if n != '' {
				opts << IdNameOption{id: r.owner_id, name: n}
			}
		}
	}
}

// richtext_names lists the id + localized name of every linkable entity of one
// kind that the [[kind:id]] renderer can resolve for `lang`: that language's
// rows first, then the en fallback (the same match set entity_name_by_id uses
// — a th form can link en-only entities and vice versa). It feeds the
// autocomplete (which inserts ids) and the live rich-text preview (which
// looks ids up for the localized name). Page-language rows come first so the
// preview's id lookup yields that language's name; both languages stay in the
// list so authors can search in either. kind defaults to 'cookie'.
pub fn richtext_names(conn sqlite.DB, lang string, kind string) []IdNameOption {
	plang := lang
	mut opts := []IdNameOption{}
	collect_translation_opts(conn, kind, plang, mut opts)
	if plang != 'en' {
		collect_translation_opts(conn, kind, 'en', mut opts)
	}
	return opts
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
