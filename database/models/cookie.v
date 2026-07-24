module models

import time

pub struct Cookie {
	id ?int @[primary; serial]
	image ?string
	release_date time.Time
}

@[table: 'cookie_translation']
@[unique_key: 'cookie_id, lang']
pub struct CookieTranslation {
    id ?int @[primary; serial]

    cookie_id int @[required; references: 'cookie'; index]
    lang string @[required; index]

    name string @[required]
    abilities string @[required]
    description string
}