module models

// Skin is one costume skin for a cookie. skin_id is the stable game id
// (1800001-...), not a serial. subtitle is the catalog's long name
// ("Gingerbrave's Celestial Emperor"), English as scraped.
@[table: 'skin']
pub struct Skin {
pub:
	skin_id  ?int @[primary]
	cookie_id int @[required; references: 'cookie(cookie_id)'; index]
	image    ?string
	grade    ?int // models.Grade value (scrape: E for collab skins, S, …)
	collab   bool
	subtitle string
}

@[table: 'skin_translation']
@[unique_key: 'skin_id, lang']
pub struct SkinTranslation {
pub:
	skin_translation_id ?int @[primary; serial]

	skin_id int @[required; references: 'skin(skin_id)'; index]
	lang    string @[required; index]

	name        string @[required]
	description string
}
