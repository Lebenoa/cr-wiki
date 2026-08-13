module database

import db.sqlite
import models
import strings
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
	effect_id int
	name      string // effect text (numbers stripped / {value} handled)
	value0    string // +0 column ("6%", "30"); empty when the effect carries no value
	value9    string // +9 column ("11%", "75")
	diff0     string // blessed tab only: delta vs the normal state ("+2%")
	diff9     string // blessed tab only: delta vs the normal state ("+10")
}

// treasure_grade maps the stored int enum value back to the Grade enum;
// none when the treasure has no wiki grade (renders without a badge).
fn treasure_grade(g ?int) ?models.Grade {
	if v := g {
		return models.Grade.from(v) or { none }
	}
	return none
}

// format_effect_value renders the numeric value with its unit suffix
// ("12%", "3s", "5000", "2-3%"); empty when the effect has no stored value
// (legacy names carry their own numbers).
pub fn format_effect_value(value ?int, value_min ?int, value_max ?int, unit models.EffectUnit) string {
	suffix := match unit {
		.percent { '%' }
		.second { 's' }
		.flat { '' }
	}
	if mn := value_min {
		if mx := value_max {
			return '${mn}-${mx}${suffix}'
		}
		return '${mn}${suffix}'
	}
	if v := value {
		return '${v}${suffix}'
	}
	return ''
}

// format_effect_bare_value renders the bare number/range without a unit
// suffix ("5", "2-3") for substitution into {value} placeholders — the unit
// word/symbol lives in each language's translation text.
pub fn format_effect_bare_value(value ?int, value_min ?int, value_max ?int) string {
	if mn := value_min {
		if mx := value_max {
			return '${mn}-${mx}'
		}
		return '${mn}'
	}
	if v := value {
		return '${v}'
	}
	return ''
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

// resolve_entity_name returns the localized name of a cookie or pet (user
// lang with en fallback); '' when no translation exists.
// cookie_id_by_name resolves a cookie by its localized name (exact match in
// the requested language, then en); 0 when no cookie has that name. Used by
// the rich-text [[Cookie Name]] link renderer.
pub fn cookie_id_by_name(conn sqlite.DB, user_lang string, cookie_name string) int {
	trs := sql conn {
		select from models.CookieTranslation where name == cookie_name && (lang == user_lang || lang == 'en')
	} or { return 0 }
	if trs.len == 0 {
		return 0
	}
	// prefer the requested language over en
	for tr in trs {
		if tr.lang == user_lang {
			return tr.owner_id
		}
	}
	return trs.first().owner_id
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

	mut result := []CombiBonusView{}
	for row in rows {
		partner_kind := if kind == 'cookie' { 'pet' } else { 'cookie' }
		pid := if kind == 'cookie' { row.pet_id } else { row.cookie_id }
		name := resolve_entity_name(conn, partner_kind, pid, lang)
		if name == '' {
			continue
		}
		result << CombiBonusView{
			partner_kind:  partner_kind
			partner_id:    pid
			partner_name:  name
			partner_image: unlock_entity_image(conn, partner_kind, pid)
			effect:        combi_effect_text(conn, row.effect_id, lang)
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

	mut result := []CombiEditRow{}
	for row in rows {
		partner_kind := if kind == 'cookie' { 'pet' } else { 'cookie' }
		pid := if kind == 'cookie' { row.pet_id } else { row.cookie_id }
		rid := row.id or { continue }
		result << CombiEditRow{
			id:            rid
			partner_id:    pid
			partner_name:  resolve_entity_name(conn, partner_kind, pid, lang)
			partner_image: unlock_entity_image(conn, partner_kind, pid)
			effect:        combi_effect_text(conn, row.effect_id, lang)
			is_hidden:     row.is_hidden
		}
	}
	return result
}

// IdNameOption is a lightweight (id, name) pair for admin-form dropdowns;
// image is populated for treasure options so the combobox can show sprites.
pub struct IdNameOption {
pub:
	id    int
	name  string
	image ?string
}

// treasure_options lists every treasure's id and localized name for the
// cookie/pet admin forms' "unlocks treasure" selector.
pub fn treasure_options(conn sqlite.DB, lang string) ![]IdNameOption {
	treasures := sql conn {
		select from models.Treasure
	}!
	user_lang := lang
	translations := sql conn {
		select from models.TreasureTranslation where lang == user_lang || lang == 'en'
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
	mut out := []IdNameOption{}
	for t in treasures {
		tid := t.treasure_id or { continue }
		if tr := tmap[tid] {
			out << IdNameOption{
				id:    tid
				name:  tr.name
				image: t.image
			}
		}
	}
	out.sort_with_compare(fn (a &IdNameOption, b &IdNameOption) int {
		if a.name < b.name {
			return -1
		}
		if a.name > b.name {
			return 1
		}
		return 0
	})
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
	mut out := []IdNameOption{}
	for c in cookies {
		cid := c.cookie_id or { continue }
		if tr := tmap[cid] {
			out << IdNameOption{
				id:    cid
				name:  tr.name
				image: c.image
			}
		}
	}
	out.sort_with_compare(fn (a &IdNameOption, b &IdNameOption) int {
		if a.name < b.name {
			return -1
		}
		if a.name > b.name {
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
	mut out := []IdNameOption{}
	for p in pets {
		pid := p.pet_id or { continue }
		if tr := tmap[pid] {
			out << IdNameOption{
				id:    pid
				name:  tr.name
				image: p.image
			}
		}
	}
	out.sort_with_compare(fn (a &IdNameOption, b &IdNameOption) int {
		if a.name < b.name {
			return -1
		}
		if a.name > b.name {
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
			v0, v9, text := split_effect_value(link, tr.name)
			result << EffectView{
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

// parse_value_cell splits one +0/+9 column value ("6%", "30", "1.5s") into
// its number and unit suffix; ok=false when the cell is empty or not a plain
// number ("???").
fn parse_value_cell(s string) (f64, string, bool) {
	if s == '' {
		return 0, '', false
	}
	mut num := s
	mut suffix := ''
	for u in ['%', 's'] {
		if s.ends_with(u) {
			num = s[..s.len - 1]
			suffix = u
			break
		}
	}
	if num.len == 0 {
		return 0, '', false
	}
	mut i := 0
	if num[0] == `-` {
		i = 1
	}
	if i >= num.len {
		return 0, '', false
	}
	mut dots := 0
	for i < num.len {
		if num[i] == `.` {
			dots++
			if dots > 1 {
				return 0, '', false
			}
		} else if !num[i].is_digit() {
			return 0, '', false
		}
		i++
	}
	return num.f64(), suffix, true
}

// value_delta renders the signed difference between a normal and a blessed
// column value ("6%" vs "8%" -> "+2%", "30" vs "40" -> "+10"); empty when
// either side is empty or non-numeric (no diff to show).
fn value_delta(normal string, blessed string) string {
	nn, nunit, nok := parse_value_cell(normal)
	bn, bunit, bok := parse_value_cell(blessed)
	if !nok || !bok {
		return ''
	}
	d := bn - nn
	unit := if nunit != '' { nunit } else { bunit }
	mut txt := ''
	if d == f64(int(d)) {
		txt = '${int(d)}'
	} else {
		txt = '${d}'
	}
	if d > 0 {
		txt = '+' + txt
	}
	return txt + unit
}

// blessed_diffs pairs each blessed-state effect with the normal-state effect
// at the same position (the wiki lists both states in the same order) and
// fills the blessed rows' diff0/diff9 columns with the per-column delta. Rows
// without a normal counterpart (or with unparseable values) carry no diff.
pub fn blessed_diffs(normal []EffectView, blessed []EffectView) []EffectView {
	mut out := []EffectView{}
	for i, e in blessed {
		mut d0 := ''
		mut d9 := ''
		if i < normal.len {
			d0 = value_delta(normal[i].value0, e.value0)
			d9 = value_delta(normal[i].value9, e.value9)
		}
		out << EffectView{
			effect_id: e.effect_id
			name:      e.name
			value0:    e.value0
			value9:    e.value9
			diff0:     d0
			diff9:     d9
		}
	}
	return out
}

// effect_name_tokens returns the number-like tokens of an effect name:
// "get 5-15 extra points" -> ["5-15"], "6-11% higher" -> ["6-11%"],
// "get 1.5-3.3 extra seconds" -> ["1.5-3.3"]. Both '-' and '~' separate
// ranges. Scanning is byte-wise but only advances through ASCII digits and
// separators, so multi-byte runes (Thai) are never sliced mid-rune.
fn effect_name_tokens(s string) []string {
	mut out := []string{}
	mut i := 0
	for i < s.len {
		if !s[i].is_digit() {
			i++
			continue
		}
		mut j := i
		for j < s.len && (s[j].is_digit() || s[j] == `.`) {
			j++
		}
		// optional range separator then a second number
		if j < s.len && (s[j] == `-` || s[j] == `~`) && j + 1 < s.len && s[j + 1].is_digit() {
			j++
			for j < s.len && (s[j].is_digit() || s[j] == `.`) {
				j++
			}
		}
		// optional unit symbol
		if j < s.len && (s[j] == `%` || s[j] == `s`) {
			j++
		}
		out << s[i..j]
		i = j
	}
	return out
}

// split_token splits one token into low, high (empty for a single) and the
// unit symbol ("30-75%" -> 30, 75, "%"; "6" -> 6, "", "").
fn split_token(t string) (string, string, string) {
	mut i := 0
	for i < t.len && (t[i].is_digit() || t[i] == `.`) {
		i++
	}
	low := t[..i]
	mut rest := t[i..]
	mut high := ''
	if rest.len > 0 && (rest[0] == `-` || rest[0] == `~`) {
		mut j := 1
		for j < rest.len && (rest[j].is_digit() || rest[j] == `.`) {
			j++
		}
		high = rest[1..j]
		rest = rest[j..]
	}
	return low, high, rest
}

// collapse_spaces trims s and collapses runs of whitespace into one space,
// byte-wise so multi-byte runes (Thai) survive untouched.
fn collapse_spaces(s string) string {
	mut b := strings.Builder{}
	mut prev_space := false
	for c in s {
		if c == ` ` {
			if !prev_space {
				b.write_byte(c)
			}
			prev_space = true
		} else {
			b.write_byte(c)
			prev_space = false
		}
	}
	return b.str().trim_space()
}

// strip_tokens removes the given tokens from s and collapses leftover
// whitespace ("get 5-15 extra points" -> "get extra points").
fn strip_tokens(s string, tokens []string) string {
	mut out := s
	for t in tokens {
		out = out.replace(t, ' ')
	}
	return collapse_spaces(out)
}

// strip_value_placeholder removes the {value} placeholder (and an attached
// unit symbol) from a translation name, collapsing the leftover whitespace.
fn strip_value_placeholder(s string) string {
	mut t := s.replace('{value}%', ' ')
	t = t.replace('{value}s', ' ')
	t = t.replace('{value}', ' ')
	return collapse_spaces(t)
}

// ends_dangling reports whether stripping the value from a {value} name left
// a dangling word ("Base speed increased by") — the substituted form reads
// better then.
fn ends_dangling(s string) bool {
	t := s.trim_space()
	if t == '' {
		return true
	}
	for w in [' by', ' with', ' for', ' to', ' of', ' from', ' an', ' a'] {
		if t.ends_with(w) {
			return true
		}
	}
	return false
}

// split_structured_value renders the +0/+9 columns from the link's stored
// value (range -> endpoints, single -> repeated for both levels) and the
// display text with the {value} placeholder stripped when it reads cleanly.
fn split_structured_value(link models.TreasureEffect, name string) (string, string, string) {
	suffix := match link.unit {
		.percent { '%' }
		.second { 's' }
		.flat { '' }
	}
	mut v0 := ''
	mut v9 := ''
	mut bare := ''
	if mn := link.value_min {
		if mx := link.value_max {
			v0 = '${mn}${suffix}'
			v9 = '${mx}${suffix}'
			bare = '${mn}-${mx}'
		} else {
			v0 = '${mn}${suffix}'
			v9 = v0
			bare = '${mn}'
		}
	} else if v := link.value {
		v0 = '${v}${suffix}'
		v9 = v0
		bare = '${v}'
	}
	mut text := name
	if name.contains('{value}') {
		if bare == '' {
			// legacy placeholder with no structured value: never leak the token
			text = strip_value_placeholder(name)
		} else if bare.contains('-') {
			// a range reads naturally inline; keep the substituted form
			text = name.replace('{value}', bare)
		} else {
			stripped := strip_value_placeholder(name)
			if !ends_dangling(stripped) {
				text = stripped
			} else {
				text = name.replace('{value}', bare)
			}
		}
	}
	return v0, v9, text
}

// split_baked_value derives the +0/+9 columns from the number baked into the
// translation name (the wiki recorded the range/single inline): the first
// range token splits into its endpoints, a lone single token repeats for both
// levels. Unrelated extra tokens stay in the text untouched.
fn split_baked_value(name string) (string, string, string) {
	toks := effect_name_tokens(name)
	mut v0 := ''
	mut v9 := ''
	mut strip := []string{}
	for t in toks {
		low, high, suffix := split_token(t)
		if high != '' {
			v0 = low + suffix
			v9 = high + suffix
			strip = [t]
			break
		}
	}
	if v0 == '' && toks.len == 1 {
		low, _, suffix := split_token(toks[0])
		v0 = low + suffix
		v9 = v0
		strip = [toks[0]]
	}
	mut text := if strip.len > 0 { strip_tokens(name, strip) } else { name }
	// "increased by X-Y" phrases would dangle without their number — keep the
	// inline form then (the columns stay filled; mild redundancy is fine)
	if strip.len > 0 && ends_dangling(text) {
		text = name
	}
	return v0, v9, text
}

// split_effect_value derives the +0/+9 column values and the display text for
// one treasure-effect link: columns come from the link's structured value
// when present, else from the range/single number baked into the name.
fn split_effect_value(link models.TreasureEffect, name string) (string, string, string) {
	if link.value != none || link.value_min != none || link.value_max != none {
		return split_structured_value(link, name)
	}
	return split_baked_value(name)
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
	name      string
	value     ?int
	value_min ?int
	value_max ?int
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
