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
	lang                   string
	name                   string
	abilities              string
	description            string
	grade                  models.Grade
	image                  ?string
	power_plus             string
	power_plus_requirement string
	unlock_goal            string
	release_date           time.Time
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
		owner_id:               cookie_id
		lang:                   params.lang
		name:                   params.name
		abilities:              params.abilities
		description:            params.description
		power_plus:             params.power_plus
		power_plus_requirement: params.power_plus_requirement
		unlock_goal:            params.unlock_goal
	}

	sql conn {
		insert new_translation into models.CookieTranslation
	}!

	return cookie_id
}

pub struct CreatePetParams {
pub:
	lang         string
	name         string
	abilities    string
	description  string
	grade        models.Grade
	image        ?string
	release_date time.Time
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

	return pet_id
}

// EffectInput is one effect entered in the treasure form: a display name plus
// an optional numeric value whose unit comes from the value's suffix (12%,
// 3s, or a bare number).
pub struct EffectInput {
pub:
	name  string
	value ?f32
	unit  models.EffectUnit
}

pub struct CreateTreasureParams {
pub:
	lang             string
	name             string
	description      string
	image            ?string
	grade            ?int // models.Grade value; none = no wiki grade
	base_treasure_id ?int // evolved rows link back to their normal base treasure
	is_evolved       bool
	release_date     time.Time
	effects          []EffectInput
	blessed_effects  []EffectInput
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
		base_treasure_id: params.base_treasure_id
		is_evolved:       params.is_evolved
		release_date:     params.release_date
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
pub fn replace_effects(conn sqlite.DB, treasure_id int, lang string, effects []EffectInput, state models.EffectState) ! {
	existing := sql conn {
		select from models.TreasureEffect where treasure_id == treasure_id && state == state
	}!
	for link in existing {
		sql conn {
			delete from models.TreasureEffect where treasure_effect_id == link.treasure_effect_id
		}!
	}

	for e in effects {
		effect_id := find_or_create_effect(conn, lang, e.name)!
		new_link := models.TreasureEffect{
			treasure_id: treasure_id
			effect_id:   effect_id
			value:       e.value
			unit:        e.unit
			state:       state
		}
		sql conn {
			insert new_link into models.TreasureEffect
		}!
	}
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
