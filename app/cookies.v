module app

import veb
import api
import database
import database.models

pub fn (wapp &App) cookies(mut ctx Context) veb.Result {
	ctx.set_translate_title('cookies_page_title')
	page_size := 30
	grades := models.grade_values

	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	cookies := database.select_cookies(wapp.db, ctx.lang, page_size, (page - 1) * page_size) or {
		println(err)
		return ctx.html('Something went wrong')
	}
	next_page := if cookies.len == page_size {
		page + 1
	} else {
		0
	}

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html('./templates/components/cookie_cards.html')
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
		ctx.set_translate_title('new_cookie_page_title')
		languages := api.available_lang()
		grades := models.grade_values
		state := CookieForm{
			lang:  ctx.lang
			grade: 'c'
		}
		return $veb.html('./templates/admin/new_cookie.html')
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
	ctx.set_translate_title('entity_detail_title', cookie.name)
	is_admin := ctx.is_admin()
	return $veb.html('./templates/views/cookie.html')
}

@['/cookies/:id/edit'; get; post]
pub fn (wapp &App) edit_cookie(mut ctx Context, id int) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	cookie := database.get_cookie(wapp.db, ctx.lang, id) or { return ctx.not_found() }

	if ctx.req.method == .get {
		ctx.set_translate_title('edit_cookie_page_title', cookie.name)
		languages := api.available_lang()
		grades := models.grade_values
		rd := cookie.release_date
		state := CookieForm{
			edit_mode:              true
			id:                     cookie.cookie_id
			name:                   cookie.name
			abilities:              cookie.abilities
			description:            cookie.description
			power_plus:             cookie.power_plus
			power_plus_requirement: cookie.power_plus_requirement
			unlock_goal:            cookie.unlock_goal
			grade:                  cookie.grade.str()
			release_date:           '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
			lang:                   cookie.lang
			image:                  cookie.image
		}
		return $veb.html('./templates/admin/new_cookie.html')
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
