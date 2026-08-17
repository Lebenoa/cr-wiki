module models

import time

pub struct Treasure {
pub:
	treasure_id ?int @[primary; serial]
	image ?string
	grade ?int // models.Grade value; none = no wiki grade (no badge)
	base_treasure_id ?int // evolved rows point at their normal base treasure
	unlock_cookie_id ?int // treasure unlocked by upgrading this cookie to max level
	unlock_pet_id ?int // treasure unlocked by upgrading this pet to max level
	is_evolved bool
	is_power_plus bool // POWER+ treasures (friendly-run bonus items) cannot be equipped in a run
	// catalog metadata from cookierundb (source of truth for the rebuilt seed):
	family string // draw | pet | cookie | consumable | special — list-page filter code
	source string // draw | pet_draw | recipe | event | login | special | cookie_upgrade | pet_upgrade — mapped to .tr keys in the UI
	sub    string // one-line effect summary (English catalog text; display fallback)
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
	effect_id   int @[required; references: 'effect'; index]

	// membership only: the effect's +0/+9 column values and the per-level
	// +1..+9 progression live in treasure_level (one row per level per
	// effect); the runtime splits those values and substitutes them into the
	// effect translation's {value} placeholders.
	state EffectState @[required]
}
