module models

@[table: 'combi_bonus']
pub struct CombiBonus {
pub:
	id        ?int @[primary; serial]
	cookie_id int @[required; references: 'cookie(cookie_id)']
	pet_id    int @[required; references: 'pet(pet_id)']
	// effect_id references the shared effect table (already translatable):
	// combo bonus effects are mostly the same phrases as treasure effects.
	// none = no effect text.
	effect_id ?int @[references: 'effect(effect_id)']
	is_hidden bool
}
