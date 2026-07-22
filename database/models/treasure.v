module models

import time

pub struct Treasure {
	id ?int @[primary; serial]
	name_key string @[required]
	description_key string @[required]
	release_date time.Time
}

pub struct Effect {
	id ?int @[primary; serial]
	name_key string @[required]
}

pub struct TreasureEffect {
	id ?int @[primary; serial]

	treasure_id int @[required]
	effect_id int @[required]

	value ?f32
	unit string @[required]
}