module models

import time

pub struct Pet {
	pet_id ?int @[primary; serial]
	image ?string
	grade Grade
	tip_for_player ?string
	release_date time.Time
}

@[table: 'pet_translation']
@[unique_key: 'pet_id, lang']
pub struct PetTranslation {
    pet_translation_id ?int @[primary; serial]

    pet_id int @[required; references: 'pet(pet_id)'; index]
    lang string @[required; index]

    name string @[required]
    abilities string @[required]
    description string
}
