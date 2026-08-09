module models

import time

pub struct Treasure {
pub:
	treasure_id ?int @[primary; serial]
	image ?string
	is_evolved bool
	is_blessed bool
	release_date time.Time
}

@[table: 'treasure_translation']
@[unique_key: 'treasure_id, lang']
pub struct TreasureTranslation {
pub:
    treasure_translation_id ?int @[primary; serial]

    treasure_id int @[required; references: 'treasure(treasure_id)'; index]
    lang string @[required; index]

    name string @[required]
    description string
}

pub struct Effect {
	effect_id ?int @[primary; serial]
}

@[table: 'effect_translation']
@[unique_key: 'effect_id, lang']
pub struct EffectTranslation {
    effect_translation_id ?int @[primary; serial]

    effect_id int @[required; references: 'effect'; index]
    lang string @[required; index]

    name string @[required]
    description string
}

pub enum EffectUnit {
	flat
	percent
	second
}

@[table: 'treasure_effect']
@[unique_key: 'treasure_id, effect_id']
pub struct TreasureEffect {
	treasure_effect_id ?int @[primary; serial]

	treasure_id int @[required; references: 'treasure'; index]
	effect_id int @[required; references: 'effect'; index]

	value ?f32
	unit EffectUnit @[required]
}
