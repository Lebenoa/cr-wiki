module models

import time

pub struct Cookie {
pub:
	cookie_id    ?int @[primary; serial]
	grade        Grade
	image        ?string
	release_date time.Time
}

@[table: 'cookie_translation']
@[unique_key: 'owner_id, lang']
@[index: 'owner_id, lang']
pub struct CookieTranslation {
pub:
	cookie_translation_id ?int @[primary; serial]

	owner_id int    @[index; references: 'cookie(cookie_id)'; required]
	lang     string @[index; required]

	name                   string @[required]
	abilities              string @[required]
	description            string
	power_plus             string
	power_plus_requirement string
	unlock_goal            string
}
