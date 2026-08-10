module models

import time

pub struct Treasure {
pub:
	treasure_id ?int @[primary; serial]
	image ?string
	grade ?int // models.Grade value; none = no wiki grade (no badge)
	base_treasure_id ?int // evolved rows point at their normal base treasure
	is_evolved bool
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
pub:
	effect_id ?int @[primary; serial]
}

@[table: 'effect_translation']
@[unique_key: 'effect_id, lang']
pub struct EffectTranslation {
pub:
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
pub:
	treasure_effect_id ?int @[primary; serial]

	treasure_id int @[required; references: 'treasure'; index]
	effect_id int @[required; references: 'effect'; index]

	value ?f32
	unit EffectUnit @[required]
}

// treasure_blessed_effect holds the blessed-state effects of evolved
// treasures; the normal-state effects live in treasure_effect on the same row
@[table: 'treasure_blessed_effect']
@[unique_key: 'treasure_id, effect_id']
pub struct TreasureBlessedEffect {
pub:
	treasure_blessed_effect_id ?int @[primary; serial]

	treasure_id int @[required; references: 'treasure'; index]
	effect_id int @[required; references: 'effect'; index]

	value ?f32
	unit EffectUnit @[required]
}
