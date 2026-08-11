module models

@[table: 'combi_bonus']
pub struct CombiBonus {
	id ?int @[primary; serial]
	cookie_id int @[required; references: 'cookie(cookie_id)']
	pet_id int @[required; references: 'pet(pet_id)']
	effect string @[required]
	is_hidden bool
}