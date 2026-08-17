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
			description = params.description, power_plus = params.power_plus,
			power_plus_requirement = params.power_plus_requirement, unlock_goal = params.unlock_goal
			where owner_id == id && lang == params.lang
		}!
	} else {
		new_tr := models.CookieTranslation{
			owner_id:                id
			lang:                    params.lang
			name:                    params.name
			abilities:               params.abilities
			description:             params.description
			power_plus:              params.power_plus
			power_plus_requirement:  params.power_plus_requirement
			unlock_goal:             params.unlock_goal
		}
		sql conn {
			insert new_tr into models.CookieTranslation
		}!
	}

	// keep the treasure-unlock link in sync: clear the treasure this cookie
	// previously unlocked (if it differs from the newly chosen one), then point
	// the chosen treasure at this cookie. A "new treasure" name creates the
	// treasure first, then links it.
	mut target_id := params.unlock_treasure_id
	if params.new_treasure_name != '' {
		target_id = create_treasure(conn, CreateTreasureParams{
			lang:         params.lang
			name:         params.new_treasure_name
			release_date: params.release_date // treasure released together with its cookie
		})!
	}
	prev := sql conn {
		select from models.Treasure where unlock_cookie_id == id
	}!
	if prev.len > 0 {
		prev_id := prev.first().treasure_id or { 0 }
		mut should_clear := true
		if tid := target_id {
			if tid == prev_id {
				should_clear = false
			}
		}
		if should_clear {
			sql conn {
				update models.Treasure set unlock_cookie_id = none where treasure_id == prev_id
			}!
		}
	}
	if tid := target_id {
		sql conn {
			update models.Treasure set unlock_cookie_id = id where treasure_id == tid
		}!
	}

	apply_combi_diff(conn, 'cookie', id, params.combi_keep, params.combi_new, params.lang)!
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

	// keep the treasure-unlock link in sync (see update_cookie); a "new
	// treasure" name creates the treasure first, then links it
	mut target_id := params.unlock_treasure_id
	if params.new_treasure_name != '' {
		target_id = create_treasure(conn, CreateTreasureParams{
			lang:         params.lang
			name:         params.new_treasure_name
			release_date: params.release_date // treasure released together with its pet
		})!
	}
	prev := sql conn {
		select from models.Treasure where unlock_pet_id == id
	}!
	if prev.len > 0 {
		prev_id := prev.first().treasure_id or { 0 }
		mut should_clear := true
		if tid := target_id {
			if tid == prev_id {
				should_clear = false
			}
		}
		if should_clear {
			sql conn {
				update models.Treasure set unlock_pet_id = none where treasure_id == prev_id
			}!
		}
	}
	if tid := target_id {
		sql conn {
			update models.Treasure set unlock_pet_id = id where treasure_id == tid
		}!
	}

	apply_combi_diff(conn, 'pet', id, params.combi_keep, params.combi_new, params.lang)!
}

pub fn update_treasure(conn sqlite.DB, id int, params CreateTreasureParams) ! {
	validate_evolved_link(id, params)!

	sql conn {
		update models.Treasure set grade = params.grade, is_evolved = params.is_evolved,
		is_power_plus = params.is_power_plus, base_treasure_id = params.base_treasure_id,
		release_date = params.release_date, unlock_cookie_id = params.unlock_cookie_id,
		unlock_pet_id = params.unlock_pet_id where treasure_id == id
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

// apply_combi_diff reconciles the combi rows of one entity (cookie or pet,
// `kind`) against the form submission: kept rows get their effect and hidden
// flag updated (the effect name resolves into the shared effect table in the
// form's language), rows missing from the submission are deleted, and new rows
// are created. Effect rows are never deleted here: other combos and treasures
// may share them.
fn apply_combi_diff(conn sqlite.DB, kind string, id int, keep map[int]CombiRowUpdate, new_rows []CombiNewRow, lang string) ! {
	mut cur := []models.CombiBonus{}
	if kind == 'cookie' {
		cur = sql conn {
			select from models.CombiBonus where cookie_id == id
		}!
	} else {
		cur = sql conn {
			select from models.CombiBonus where pet_id == id
		}!
	}
	for row in cur {
		rid := row.id or { continue }
		if rid in keep {
			upd := keep[rid]
			eid := combi_effect_id(conn, lang, upd.effect)!
			new_effect := if eid > 0 {
				eid
			} else {
				none
			}
			sql conn {
				update models.CombiBonus set effect_id = new_effect, is_hidden = upd.is_hidden where id == rid
			}!
		} else {
			sql conn {
				delete from models.CombiBonus where id == rid
			}!
		}
	}
	if new_rows.len > 0 {
		cookie_side := if kind == 'cookie' { id } else { 0 }
		pet_side := if kind == 'pet' { id } else { 0 }
		add_combi_rows(conn, cookie_side, pet_side, new_rows, lang)!
	}
}

// update_build overwrites a build's loadout and run fields. Ownership is
// enforced by the caller; author/user_id/expiry are left untouched so a
// rename or an expiry change never happens through the edit form.
pub fn update_build(conn sqlite.DB, id int, new_cookie int, new_cookie2 int, new_pet int, new_t1 int, new_t2 int, new_t3 int, new_t1_blessed int, new_t2_blessed int, new_t3_blessed int, new_t1_level int, new_t2_level int, new_t3_level int, new_ep int, new_ep_special int, new_tags []string, new_boosts []string, new_boost string, new_power_effects []string, new_score u64, new_coin u64, new_time_ms u64, new_boxes u64, new_description string, new_youtube string) ! {
	combi_id := combi_bonus_id_for(conn, new_cookie, new_pet)!
	sql conn {
		update models.Build set cookie_id = new_cookie, cookie2_id = new_cookie2, pet_id = new_pet,
		combi_bonus_id = if combi_id > 0 {
			combi_id
		} else {
			none
		},
		treasure1_id = new_t1, treasure2_id = new_t2, treasure3_id = new_t3,
		treasure1_blessed = new_t1_blessed, treasure2_blessed = new_t2_blessed,
		treasure3_blessed = new_t3_blessed, treasure1_level = clamp_level(new_t1_level),
		treasure2_level = clamp_level(new_t2_level), treasure3_level = clamp_level(new_t3_level),
		ep = new_ep,
		ep_special = new_ep_special, tag = new_tags.join(','), boosts = new_boosts.join(','),
		boost = new_boost, power_effects = new_power_effects.join(','),
		score = new_score, coin = new_coin, time = new_time_ms, boxes = new_boxes,
		description = new_description, youtube_url = new_youtube
		where build_id == id
	}!
}

// delete_build removes a build entirely (the author's edit/delete flow).
pub fn delete_build(conn sqlite.DB, id int) ! {
	sql conn {
		delete from models.Build where build_id == id
	}!
}
