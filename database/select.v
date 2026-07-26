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
		tip_for_player ?string
		release_date time.Time

		lang string
		name string
		abilities string
		description string
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

pub fn select_cookies(conn sqlite.DB, lang string) ![]CookieView {
	cookies := sql conn {
		select from models.Cookie
		order by release_date desc
		limit 30
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
				release_date: cookie.release_date
				lang: tr.lang
				name: tr.name
				abilities: tr.abilities
				description: tr.description
			}
		}
	}

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
		release_date: cookie.release_date
		lang: translations[id].lang
		name: translations[id].name
		abilities: translations[id].abilities
		description: translations[id].description
	}
}
