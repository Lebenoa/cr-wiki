module app

import veb
import api
import database
import database.models

pub fn (wapp &App) cookies(mut ctx Context) veb.Result {
	ctx.set_translate_title("cookies_page_title")
	page_size := 30
	grades := models.grade_values

	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	cookies := database.select_cookies(wapp.db, ctx.lang, page_size, (page - 1) * page_size) or {
		println(err)
		return ctx.html("Something went wrong")
	}
	next_page := if cookies.len == page_size {
		page + 1
	} else {
		0
	}

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html("./templates/partials/cookie_cards.html")
	}

	return $veb.html()
}

@['/cookies/new'; get; post]
pub fn (wapp &App) new_cookie(mut ctx Context) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	if ctx.req.method == .get {
		ctx.page_title = 'New Cookie | Classic/FanWiki'
		languages := api.available_lang()
		grades := models.grade_values
		edit_mode := false
		entity_id := 0
		entity_name := ''
		entity_abilities := ''
		entity_description := ''
		entity_power_plus := ''
		entity_grade := 'c'
		entity_release_date := ''
		entity_lang := ctx.lang
		entity_image := ?string(none)
		return $veb.html("./templates/admin/new_cookie.html")

	} else if ctx.req.method == .post {
		params := parse_cookie_form(mut ctx) or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
		}

		cookie_id := database.create_cookie(wapp.db, params) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Failed to create cookie: ${err}')
		}

		return ctx.redirect('/cookies/${cookie_id}')
	}

	return ctx.not_found()
}

@['/cookies/:id']
pub fn (wapp &App) cookie_info(mut ctx Context, id int) veb.Result {
	cookie := database.get_cookie(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	ctx.page_title = '${cookie.name} | Classic Fan/Wiki'
	is_admin := ctx.is_admin()
	return $veb.html("./templates/views/cookie.html")
}

@['/cookies/:id/edit'; get; post]
pub fn (wapp &App) edit_cookie(mut ctx Context, id int) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	cookie := database.get_cookie(wapp.db, ctx.lang, id) or { return ctx.not_found() }

	if ctx.req.method == .get {
		ctx.page_title = 'Edit ${cookie.name} | Classic/FanWiki'
		languages := api.available_lang()
		grades := models.grade_values
		edit_mode := true
		entity_id := cookie.cookie_id
		entity_name := cookie.name
		entity_abilities := cookie.abilities
		entity_description := cookie.description
		entity_power_plus := cookie.power_plus
		entity_grade := cookie.grade.str()
		rd := cookie.release_date
		entity_release_date := '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
		entity_lang := cookie.lang
		entity_image := cookie.image
		return $veb.html("./templates/admin/new_cookie.html")

	} else if ctx.req.method == .post {
		params := parse_cookie_form(mut ctx) or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
		}

		database.update_cookie(wapp.db, id, params) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Failed to update cookie: ${err}')
		}

		return ctx.redirect('/cookies/${id}')
	}

	return ctx.not_found()
}
