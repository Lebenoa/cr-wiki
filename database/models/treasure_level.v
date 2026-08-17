module models

// TreasureLevel is one upgrade level (0-9) of one effect of a treasure: the
// values string for that level ("12%", "0.3-0.8s", '' for no-value effects).
// Blessed states carry their own rows (the blessed catalog entry has its own
// per-level table). The runtime renders the +0/+9 columns from the level 0
// and level 9 rows and substitutes {value} placeholders in the effect
// translation with the split values.
@[table: 'treasure_level']
@[unique_key: 'treasure_id, level, effect_id, state']
pub struct TreasureLevel {
pub:
	treasure_level_id ?int @[primary; serial]
	treasure_id       int @[required; references: 'treasure(treasure_id)'; index]
	level             int @[required]
	effect_id         int @[required; references: 'effect'; index]
	state             EffectState @[required]
	values            string @[required]
}
