module models

// Jelly is one in-run jelly/pickup with its score value. jelly_id is the
// stable game id (the catalog's j402 -> 402), not a serial.
@[table: 'jelly']
pub struct Jelly {
pub:
	jelly_id ?int @[primary]
	image    ?string
	score    f64 // "432.9"
}

@[table: 'jelly_translation']
@[unique_key: 'jelly_id, lang']
pub struct JellyTranslation {
pub:
	jelly_translation_id ?int @[primary; serial]

	jelly_id int @[required; references: 'jelly(jelly_id)'; index]
	lang     string @[required; index]

	name        string @[required]
	description string
}

// JellyMaker links a jelly to the cookie/pet/treasure whose skill creates it.
// entity_kind is 'cookie' | 'pet' | 'treasure' (app-enforced; the polymorphic
// id is the one place this repo needs it — a jelly's maker is always one of
// the three entity kinds).
@[table: 'jelly_maker']
@[unique_key: 'jelly_id, entity_kind, entity_id']
pub struct JellyMaker {
pub:
	jelly_maker_id ?int @[primary; serial]
	jelly_id       int @[required; references: 'jelly(jelly_id)'; index]
	entity_kind    string @[required; index]
	entity_id      int @[required; index]
}
