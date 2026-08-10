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

// EffectState distinguishes a treasure's normal effects from its blessed
// (evolved) effects; both live in treasure_effect, one row per state.
pub enum EffectState {
	normal
	blessed
}

@[table: 'treasure_effect']
@[unique_key: 'treasure_id, effect_id, state']
pub struct TreasureEffect {
pub:
	treasure_effect_id ?int @[primary; serial]

	treasure_id int @[required; references: 'treasure'; index]
	effect_id int @[required; references: 'effect'; index]

	value ?f32
	unit EffectUnit @[required]
	state EffectState @[required]
}
