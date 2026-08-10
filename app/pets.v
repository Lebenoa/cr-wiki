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
		return $veb.html("./templates/partials/pet_cards.html")
	}

	return $veb.html()
}

@['/pets/:id']
pub fn (wapp &App) pet_info(mut ctx Context, id int) veb.Result {
	pet := database.get_pet(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	ctx.page_title = '${pet.name} | Classic Fan/Wiki'
	is_admin := ctx.is_admin()
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
		ctx.page_title = 'Edit ${pet.name} | Classic/FanWiki'
		languages := api.available_lang()
		grades := models.grade_values
		edit_mode := true
		entity_id := pet.pet_id
		entity_name := pet.name
		entity_abilities := pet.abilities
		entity_description := pet.description
		entity_grade := pet.grade.str()
		rd := pet.release_date
		entity_release_date := '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
		entity_lang := pet.lang
		entity_image := pet.image
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

@['/pets/new'; get; post]
pub fn (wapp &App) new_pet(mut ctx Context) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	if ctx.req.method == .get {
		ctx.page_title = 'New Pet | Classic/FanWiki'
		languages := api.available_lang()
		grades := models.grade_values
		edit_mode := false
		entity_id := 0
		entity_name := ''
		entity_abilities := ''
		entity_description := ''
		entity_grade := 'c'
		entity_release_date := ''
		entity_lang := ctx.lang
		entity_image := ?string(none)
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
