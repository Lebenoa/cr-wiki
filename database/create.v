module database

import db.sqlite
import crypto.argon2
import models
import time

pub fn create_user(conn sqlite.DB, username string, password string) !int {
	new_user := models.User{
		username: username
		password: argon2.generate_from_password(password.bytes())!
		created_at: time.now()
	}

	id := sql conn {
		insert new_user into models.User
	}!

	return id
}

pub struct CreateCookieParams {
pub:
	lang           string
	name           string
	abilities      string
	description    string
	grade          models.Grade
	image          ?string
	power_plus     string
	power_plus_requirement string
	unlock_goal    string
	release_date   time.Time
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
		owner_id:                cookie_id
		lang:                    params.lang
		name:                    params.name
		abilities:               params.abilities
		description:             params.description
		power_plus:              params.power_plus
		power_plus_requirement:  params.power_plus_requirement
		unlock_goal:             params.unlock_goal
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

pub struct CreateTreasureParams {
pub:
	lang         string
	name         string
	description  string
	image        ?string
	is_evolved   bool
	is_blessed   bool
	release_date time.Time
}

pub fn create_treasure(conn sqlite.DB, params CreateTreasureParams) !int {
	if params.lang == '' {
		return error('treasure lang is required')
	}
	if params.name == '' {
		return error('treasure name is required')
	}

	new_treasure := models.Treasure{
		image:        params.image
		is_evolved:   params.is_evolved
		is_blessed:   params.is_blessed
		release_date: params.release_date
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

	return treasure_id
}
