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
	println(translations)

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
