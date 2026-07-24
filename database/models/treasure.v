module models

import time

pub struct Treasure {
	id ?int @[primary; serial]
	is_evolved bool
	release_date time.Time
}

@[table: 'treasure_translation']
@[unique_key: 'treasure_id, lang']
pub struct TreasureTranslation {
    id ?int @[primary; serial]

    treasure_id int @[required; references: 'treasure'; index]
    lang string @[required; index]

    name string @[required]
    description string
}

pub struct Effect {
	id ?int @[primary; serial]
}

@[table: 'effect_translation']
@[unique_key: 'effect_id, lang']
pub struct EffectTranslation {
    id ?int @[primary; serial]

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
@[index: 'treasure_id, sort_order']
pub struct TreasureEffect {
	id ?int @[primary; serial]

	treasure_id int @[required; references: 'treasure'; index]
	effect_id int @[required; references: 'effect'; index]
	sort_order int

	value ?f32
	unit EffectUnit @[required]
}