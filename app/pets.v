module app

import veb
import api
import database
import database.models

pub fn (mut wapp App) pets(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title("pets_page_title")
	ctx.set_translate_desc("pets_page_description")
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
pub fn (mut wapp App) new_pet(mut ctx Context) veb.Result {
	if !ctx.is_admin() {
		return ctx.not_found()
	}

	if ctx.req.method == .get {
		ctx.set_translate_title('new_pet_page_title')
		ctx.noindex = true
		languages := api.available_lang()
		grades := models.grade_values
		state := pet_form_state(wapp, ctx, none)
		return $veb.html("./templates/admin/new_pet.html")
	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'pet_form') {
			ctx.res.set_status(.forbidden)
			return pet_submit_error(wapp, ctx, none, veb.tr(ctx.lang, 'turnstile_form_failed'))
		}

		params := parse_pet_form(mut wapp, mut ctx) or {
			ctx.res.set_status(.bad_request)
			return pet_submit_error(wapp, ctx, none, err.msg())
		}

		database.create_pet(wapp.db, params) or {
			ctx.res.set_status(.bad_request)
			return pet_submit_error(wapp, ctx, none, 'Failed to create pet: ${err}')
		}

		return submit_success(mut ctx, '/pets')
	}

	return ctx.not_found()
}

@['/pets/:id']
pub fn (mut wapp App) pet_info(mut ctx Context, id int) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
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
	ctx.set_translate_desc('entity_detail_description', pet.name)
	ctx.set_og_image('pets', pet.image)
	is_admin := ctx.is_admin()
	// rich text: [[Cookie Name]] links + {color:x} spans in prose fields
	description_html := render_rich_text(wapp.db, ctx.lang, pet.description)
	return $veb.html("./templates/views/pet.html")
}

@['/pets/:id/edit'; get; post]
pub fn (mut wapp App) edit_pet(mut ctx Context, id int) veb.Result {
	if !ctx.is_admin() {
		return ctx.not_found()
	}

	pet := database.get_pet(wapp.db, ctx.lang, id) or { return ctx.not_found() }

	if ctx.req.method == .get {
		ctx.set_translate_title('edit_pet_page_title', pet.name)
		ctx.noindex = true
		languages := api.available_lang()
		grades := models.grade_values
		state := pet_form_state(wapp, ctx, pet)
		return $veb.html("./templates/admin/new_pet.html")

	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'pet_form') {
			ctx.res.set_status(.forbidden)
			return pet_submit_error(wapp, ctx, pet, veb.tr(ctx.lang, 'turnstile_form_failed'))
		}

		params := parse_pet_form(mut wapp, mut ctx) or {
			ctx.res.set_status(.bad_request)
			return pet_submit_error(wapp, ctx, pet, err.msg())
		}

		database.update_pet(wapp.db, id, params) or {
			ctx.res.set_status(.bad_request)
			return pet_submit_error(wapp, ctx, pet, 'Failed to update pet: ${err}')
		}

		return submit_success(mut ctx, '/pets/${id}')
	}

	return ctx.not_found()
}
