module app

import veb
import api
import database
import database.models

pub fn (wapp &App) pets(mut ctx Context) veb.Result {
	ctx.set_translate_title("pets_page_title")
	page_size := 30
	grades := models.grade_values

	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	pets := database.select_pets(wapp.db, ctx.lang, page_size, (page - 1) * page_size) or {
		println(err)
		return ctx.html("Something went wrong")
	}
	next_page := if pets.len == page_size {
		page + 1
	} else {
		0
	}

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html("./templates/components/pet_cards.html")
	}

	return $veb.html()
}

@['/pets/new'; get; post]
pub fn (wapp &App) new_pet(mut ctx Context) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	if ctx.req.method == .get {
		ctx.set_translate_title('new_pet_page_title')
		languages := api.available_lang()
		grades := models.grade_values
		state := PetForm{
			lang:         ctx.lang
			grade:        'c'
			treasures:    database.treasure_options(wapp.db, ctx.lang, false) or { [] }
			partners:     database.cookie_options(wapp.db, ctx.lang) or { [] }
			partner_kind: 'cookie'
			effect_names: database.effect_names(wapp.db, ctx.lang) or { [] }
		}
		return $veb.html("./templates/admin/new_pet.html")
	} else if ctx.req.method == .post {
		params := parse_pet_form(mut ctx) or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
		}

		database.create_pet(wapp.db, params) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Failed to create pet: ${err}')
		}

		return ctx.redirect('/pets')
	}

	return ctx.not_found()
}

@['/pets/:id']
pub fn (wapp &App) pet_info(mut ctx Context, id int) veb.Result {
	pet := database.get_pet(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	// the treasure unlocked by upgrading this pet to max level (none for pets
	// without one)
	mut unlocked_treasure := ?database.TreasureView(none)
	if t := database.get_unlocked_treasure(wapp.db, ctx.lang, 'pet', pet.pet_id) {
		unlocked_treasure = t
	}
	// combo bonuses pairing this pet with a cookie (empty when it has none)
	combi_bonus := database.get_combi_bonus(wapp.db, ctx.lang, 'pet', pet.pet_id) or { [] }
	ctx.set_translate_title('entity_detail_title', pet.name)
	is_admin := ctx.is_admin()
	// rich text: [[Cookie Name]] links + {color:x} spans in prose fields
	description_html := render_rich_text(wapp.db, ctx.lang, pet.description)
	return $veb.html("./templates/views/pet.html")
}

@['/pets/:id/edit'; get; post]
pub fn (wapp &App) edit_pet(mut ctx Context, id int) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	pet := database.get_pet(wapp.db, ctx.lang, id) or { return ctx.not_found() }

	if ctx.req.method == .get {
		ctx.set_translate_title('edit_pet_page_title', pet.name)
		languages := api.available_lang()
		grades := models.grade_values
		rd := pet.release_date
		unlock_tid := if ut := database.get_unlocked_treasure(wapp.db, ctx.lang, 'pet',
			pet.pet_id) {
			ut.treasure_id
		} else {
			0
		}
		state := PetForm{
			edit_mode:         true
			id:                pet.pet_id
			name:              pet.name
			abilities:         pet.abilities
			description:       pet.description
			grade:             pet.grade.str()
			release_date:      '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
			lang:              pet.lang
			image:             pet.image
			unlock_treasure_id: unlock_tid
			treasures:         database.treasure_options(wapp.db, ctx.lang, false) or { [] }
			combis:            database.combi_edit_rows(wapp.db, ctx.lang, 'pet', id) or { [] }
			partners:          database.cookie_options(wapp.db, ctx.lang) or { [] }
			partner_kind:      'cookie'
			effect_names:      database.effect_names(wapp.db, ctx.lang) or { [] }
		}
		return $veb.html("./templates/admin/new_pet.html")

	} else if ctx.req.method == .post {
		params := parse_pet_form(mut ctx) or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
		}

		database.update_pet(wapp.db, id, params) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Failed to update pet: ${err}')
		}

		return ctx.redirect('/pets/${id}')
	}

	return ctx.not_found()
}
