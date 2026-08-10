module database

import db.sqlite
import models

pub fn update_cookie(conn sqlite.DB, id int, params CreateCookieParams) ! {
	sql conn {
		update models.Cookie set grade = params.grade, release_date = params.release_date
		where cookie_id == id
	}!

	if image := params.image {
		sql conn {
			update models.Cookie set image = image where cookie_id == id
		}!
	}

	// Upsert translation for the given lang (insert if missing, update fields)
	existing := sql conn {
		select from models.CookieTranslation where owner_id == id && lang == params.lang
	}!
	if existing.len > 0 {
		sql conn {
			update models.CookieTranslation set name = params.name, abilities = params.abilities,
			description = params.description, power_plus = params.power_plus where owner_id == id
			&& lang == params.lang
		}!
	} else {
		new_tr := models.CookieTranslation{
			owner_id:    id
			lang:        params.lang
			name:        params.name
			abilities:   params.abilities
			description: params.description
			power_plus:  params.power_plus
		}
		sql conn {
			insert new_tr into models.CookieTranslation
		}!
	}
}

pub fn update_pet(conn sqlite.DB, id int, params CreatePetParams) ! {
	sql conn {
		update models.Pet set grade = params.grade, release_date = params.release_date where pet_id == id
	}!

	if image := params.image {
		sql conn {
			update models.Pet set image = image where pet_id == id
		}!
	}

	existing := sql conn {
		select from models.PetTranslation where pet_id == id && lang == params.lang
	}!
	if existing.len > 0 {
		sql conn {
			update models.PetTranslation set name = params.name, abilities = params.abilities,
			description = params.description where pet_id == id && lang == params.lang
		}!
	} else {
		new_tr := models.PetTranslation{
			pet_id:      id
			lang:        params.lang
			name:        params.name
			abilities:   params.abilities
			description: params.description
		}
		sql conn {
			insert new_tr into models.PetTranslation
		}!
	}
}

pub fn update_treasure(conn sqlite.DB, id int, params CreateTreasureParams) ! {
	sql conn {
		update models.Treasure set grade = params.grade, is_evolved = params.is_evolved,
		release_date = params.release_date where treasure_id == id
	}!

	if image := params.image {
		sql conn {
			update models.Treasure set image = image where treasure_id == id
		}!
	}

	existing := sql conn {
		select from models.TreasureTranslation where treasure_id == id && lang == params.lang
	}!
	if existing.len > 0 {
		sql conn {
			update models.TreasureTranslation set name = params.name, description = params.description
			where treasure_id == id && lang == params.lang
		}!
	} else {
		new_tr := models.TreasureTranslation{
			treasure_id: id
			lang:        params.lang
			name:        params.name
			description: params.description
		}
		sql conn {
			insert new_tr into models.TreasureTranslation
		}!
	}

	replace_effects(conn, id, params.lang, params.effects, models.EffectState.normal)!
	replace_effects(conn, id, params.lang, params.blessed_effects, models.EffectState.blessed)!
}
