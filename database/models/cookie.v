module models

import time

pub struct Cookie {
	id ?int @[primary; serial]
	name_key string @[required]
	abilities_key string @[required]
	description_key string @[required]
	image ?string
	release_date time.Time
}