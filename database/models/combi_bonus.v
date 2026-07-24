module models

@[table: 'combi_bonus']
pub struct CombiBonus {
	id ?int @[primary; serial]
	cookie_id int @[required; references: 'cookie']
	pet_id int @[required; references: 'pet']
	effect string @[required]
	is_hidden bool
}