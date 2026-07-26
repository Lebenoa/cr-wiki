module models

import time


pub struct Cookie {
	pub:
	cookie_id ?int @[primary; serial]
	grade Grade
	image ?string
	tip_for_player ?string
	release_date time.Time
}

@[table: 'cookie_translation']
@[unique_key: 'owner_id, lang']
@[index: 'owner_id, lang']
pub struct CookieTranslation {
	pub:
    cookie_translation_id ?int @[primary; serial]

    owner_id int @[required; references: 'cookie(cookie_id)'; index]
    lang string @[required; index]

    name string @[required]
    abilities string @[required]
    description string
}
