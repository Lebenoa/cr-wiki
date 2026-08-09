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
		treasure_id int
		image ?string
		is_evolved bool
		is_blessed bool
		release_date time.Time

		lang string
		name string
		description string
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

pub fn select_treasures(conn sqlite.DB, lang string) ![]TreasureView {
	treasures := sql conn {
		select from models.Treasure
		order by release_date desc
		limit 30
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
		if tr := translation_map[treasure.treasure_id or { continue }] {
			result << TreasureView{
				treasure_id: treasure.treasure_id or { 0 }
				image: treasure.image
				is_evolved: treasure.is_evolved
				is_blessed: treasure.is_blessed
				release_date: treasure.release_date
				lang: tr.lang
				name: tr.name
				description: tr.description
			}
		}
	}

	result.sort_with_compare(compare_treasures)

	return result
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
