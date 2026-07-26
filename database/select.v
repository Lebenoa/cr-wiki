module database

import db.sqlite
import models
import time

@[table: 'cookie']
pub struct CookieView {
	pub:
		cookie_id int
		image ?string
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

fn backup_translations[T](conn sqlite.DB, lang string, params ?BackupTranslationParam) !map[int]T {
	abs_params := params or { BackupTranslationParam{} }

	translations := if ow_id := abs_params.owner_id {
		sql conn {
			select name, owner_id, lang from T
			where (lang == lang || lang == 'en') && owner_id == ow_id
		}!
	} else {
		sql conn {
			select name, owner_id, lang from T
			where lang == lang || lang == 'en'
		}!
	}

	mut translation_map := map[int]models.CookieTranslation{}

	// Requested language first
	for tr in translations {
		if tr.lang == lang {
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
		order by cookie_id desc
		limit 30
	}!

	if cookies.len == 0 {
		return []
	}

	mut ids := []int{}
	for cookie in cookies {
		ids << cookie.cookie_id or { continue }
	}

	translations := sql conn {
		select name, owner_id, lang from models.CookieTranslation
		where owner_id in ids && (lang == lang || lang == 'en')
	}!

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
	cookie := sql conn {
		select from models.Cookie
		where cookie_id == id
	}!

	if cookie.len == 0 {
		return error('cookie not found')
	}

	translations := backup_translations[models.CookieTranslation](conn, lang, owner_id: id)!

	println(translations[id])
	return CookieView{}
}
