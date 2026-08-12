module database

import db.sqlite
import crypto.argon2
import models
import time

pub fn create_user(conn sqlite.DB, username string, password string) !int {
	new_user := models.User{
		username:   username
		password:   argon2.generate_from_password(password.bytes())!
		created_at: time.now()
	}

	id := sql conn {
		insert new_user into models.User
	}!

	return id
}

pub struct CreateCookieParams {
pub:
	lang               string
	name               string
	abilities          string
	description        string
	grade              models.Grade
	image              ?string
	power_plus         string
	release_date       time.Time
	unlock_treasure_id ?int // treasure this cookie unlocks at max level
	new_treasure_name  string // non-empty: create this treasure, then link it
	combi_keep         map[int]CombiRowUpdate // existing combi rows to keep (id -> effect/hidden)
	combi_new          []CombiNewRow          // brand-new combi rows to create
}

// CombiRowUpdate is the submitted state of one existing combi row on the
// cookie/pet edit form: the effect name (in the form's language) and whether
// it is hidden from... nothing — wiki readers see everything, the flag just
// marks the row for the badge.
pub struct CombiRowUpdate {
pub:
	effect    string
	is_hidden bool
}

// CombiNewRow is one new combi row added on the cookie/pet form: the partner
// id (the other half of the pair) plus effect name and hidden flag.
pub struct CombiNewRow {
pub:
	partner_id int
	effect     string
	is_hidden  bool
}

// combi_effect_id resolves a combi effect name into the shared effect table
// (creating the effect when new); 0 for an empty name (no effect).
fn combi_effect_id(conn sqlite.DB, lang string, name string) !int {
	n := name.trim_space()
	if n == '' {
		return 0
	}
	return find_or_create_effect(conn, lang, n)
}

// add_combi_rows inserts new combi rows where one side is fixed (the cookie
// or pet being edited, `cookie_side`/`pet_side`; exactly one is non-zero) and
// the other comes from the form. Duplicate pairs are skipped.
fn add_combi_rows(conn sqlite.DB, cookie_side int, pet_side int, rows []CombiNewRow, lang string) ! {
	for nr in rows {
		pid := nr.partner_id
		if pid <= 0 {
			continue
		}
		cid := if cookie_side > 0 { cookie_side } else { pid }
		pid2 := if pet_side > 0 { pet_side } else { pid }
		dup := sql conn {
			select from models.CombiBonus where cookie_id == cid && pet_id == pid2
		}!
		if dup.len > 0 {
			continue
		}
		eid := combi_effect_id(conn, lang, nr.effect)!
		new_row := models.CombiBonus{
			cookie_id: cid
			pet_id:    pid2
			effect_id: if eid > 0 {
				eid
			} else {
				none
			}
			is_hidden: nr.is_hidden
		}
		sql conn {
			insert new_row into models.CombiBonus
		}!
	}
}

pub fn create_cookie(conn sqlite.DB, params CreateCookieParams) !int {
	if params.lang == '' {
		return error('cookie lang is required')
	}
	if params.name == '' {
		return error('cookie name is required')
	}
	if params.abilities == '' {
		return error('cookie abilities are required')
	}

	new_cookie := models.Cookie{
		grade:        params.grade
		image:        params.image
		release_date: params.release_date
	}

	cookie_id := sql conn {
		insert new_cookie into models.Cookie
	}!

	new_translation := models.CookieTranslation{
		owner_id:    cookie_id
		lang:        params.lang
		name:        params.name
		abilities:   params.abilities
		description: params.description
		power_plus:  params.power_plus
	}

	sql conn {
		insert new_translation into models.CookieTranslation
	}!

	// link the chosen treasure to this cookie (the unlock lives on the treasure);
	// a "new treasure" name creates the treasure first, then links it
	if params.new_treasure_name != '' {
		tid := create_treasure(conn, CreateTreasureParams{
			lang:         params.lang
			name:         params.new_treasure_name
			release_date: params.release_date // treasure released together with its cookie
		})!
		sql conn {
			update models.Treasure set unlock_cookie_id = cookie_id where treasure_id == tid
		}!
	} else if utid := params.unlock_treasure_id {
		sql conn {
			update models.Treasure set unlock_cookie_id = cookie_id where treasure_id == utid
		}!
	}

	if params.combi_new.len > 0 {
		add_combi_rows(conn, cookie_id, 0, params.combi_new, params.lang)!
	}

	return cookie_id
}

pub struct CreatePetParams {
pub:
	lang               string
	name               string
	abilities          string
	description        string
	grade              models.Grade
	image              ?string
	release_date       time.Time
	unlock_treasure_id ?int // treasure this pet unlocks at max level
	new_treasure_name  string // non-empty: create this treasure, then link it
	combi_keep         map[int]CombiRowUpdate
	combi_new          []CombiNewRow
}

pub fn create_pet(conn sqlite.DB, params CreatePetParams) !int {
	if params.lang == '' {
		return error('pet lang is required')
	}
	if params.name == '' {
		return error('pet name is required')
	}
	if params.abilities == '' {
		return error('pet abilities are required')
	}

	new_pet := models.Pet{
		grade:        params.grade
		image:        params.image
		release_date: params.release_date
	}

	pet_id := sql conn {
		insert new_pet into models.Pet
	}!

	new_translation := models.PetTranslation{
		pet_id:      pet_id
		lang:        params.lang
		name:        params.name
		abilities:   params.abilities
		description: params.description
	}

	sql conn {
		insert new_translation into models.PetTranslation
	}!

	// link the chosen treasure to this pet (the unlock lives on the treasure);
	// a "new treasure" name creates the treasure first, then links it
	if params.new_treasure_name != '' {
		tid := create_treasure(conn, CreateTreasureParams{
			lang:         params.lang
			name:         params.new_treasure_name
			release_date: params.release_date // treasure released together with its pet
		})!
		sql conn {
			update models.Treasure set unlock_pet_id = pet_id where treasure_id == tid
		}!
	} else if utid := params.unlock_treasure_id {
		sql conn {
			update models.Treasure set unlock_pet_id = pet_id where treasure_id == utid
		}!
	}

	if params.combi_new.len > 0 {
		add_combi_rows(conn, 0, pet_id, params.combi_new, params.lang)!
	}

	return pet_id
}

// EffectInput is one effect entered in the treasure form: a display name plus
// an optional numeric value. The value is a single number, a min/max range
// ("2-3%"), or absent (legacy names carry their own numbers); the unit comes
// from the typed suffix (%, s, or a bare number) and is validated at submit.
pub struct EffectInput {
pub:
	name      string
	value     ?int
	value_min ?int
	value_max ?int
	unit      models.EffectUnit
}

pub struct CreateTreasureParams {
pub:
	lang            string
	name            string
	description     string
	image           ?string
	grade           ?int // models.Grade value; none = no wiki grade
	is_evolved      bool
	release_date    time.Time
	effects         []EffectInput
	blessed_effects []EffectInput
	unlock_cookie_id ?int // cookie whose max-level upgrade unlocks this treasure
	unlock_pet_id    ?int // pet whose max-level upgrade unlocks this treasure
}

pub fn create_treasure(conn sqlite.DB, params CreateTreasureParams) !int {
	if params.lang == '' {
		return error('treasure lang is required')
	}
	if params.name == '' {
		return error('treasure name is required')
	}

	new_treasure := models.Treasure{
		image:            params.image
		grade:            params.grade
		is_evolved:       params.is_evolved
		release_date:     params.release_date
		unlock_cookie_id: params.unlock_cookie_id
		unlock_pet_id:    params.unlock_pet_id
	}

	treasure_id := sql conn {
		insert new_treasure into models.Treasure
	}!

	new_translation := models.TreasureTranslation{
		treasure_id: treasure_id
		lang:        params.lang
		name:        params.name
		description: params.description
	}

	sql conn {
		insert new_translation into models.TreasureTranslation
	}!

	replace_effects(conn, treasure_id, params.lang, params.effects, models.EffectState.normal)!
	replace_effects(conn, treasure_id, params.lang, params.blessed_effects,
		models.EffectState.blessed)!

	return treasure_id
}

// replace_effects rewrites the treasure's effect links for one state: existing
// links are dropped and the submitted effects re-created, so the saved order
// always matches the form. Effect names are matched in the form's language so
// re-saving an edit reuses the same effect row (translations are per-lang).
// The delete + re-insert runs in one transaction, so a failed insert cannot
// leave the treasure with its effects half-removed.
pub fn replace_effects(conn sqlite.DB, treasure_id int, lang string, effects []EffectInput, state models.EffectState) ! {
	conn.begin()!
	defer {
		conn.rollback() or {}
	}

	existing := sql conn {
		select from models.TreasureEffect where treasure_id == treasure_id && state == state
	}!
	for link in existing {
		sql conn {
			delete from models.TreasureEffect where treasure_effect_id == link.treasure_effect_id
		}!
	}

	// duplicate effect names resolve to the same effect row, and the link
	// table's unique key is (treasure_id, effect_id, state); keep the first
	// occurrence so a repeated name can't violate the constraint
	mut linked := map[int]bool{}
	for e in effects {
		effect_id := find_or_create_effect(conn, lang, e.name)!
		if effect_id in linked {
			continue
		}
		linked[effect_id] = true
		new_link := models.TreasureEffect{
			treasure_id: treasure_id
			effect_id:   effect_id
			value:       e.value
			value_min:   e.value_min
			value_max:   e.value_max
			unit:        e.unit
			state:       state
		}
		sql conn {
			insert new_link into models.TreasureEffect
		}!
	}

	conn.commit()!
}

// find_or_create_effect returns the effect id whose translation in `lang` has
// the given name, creating the effect row and its translation when new.
fn find_or_create_effect(conn sqlite.DB, lang string, name string) !int {
	plang := lang
	pname := name
	existing := sql conn {
		select from models.EffectTranslation where lang == plang && name == pname
	}!
	if existing.len > 0 {
		return existing.first().effect_id
	}

	new_effect := models.Effect{}
	effect_id := sql conn {
		insert new_effect into models.Effect
	}!
	new_tr := models.EffectTranslation{
		effect_id: effect_id
		lang:      lang
		name:      name
	}
	sql conn {
		insert new_tr into models.EffectTranslation
	}!
	return effect_id
}

