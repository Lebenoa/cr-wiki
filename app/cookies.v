module app

import veb
import time
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
		return $veb.html("./templates/admin/new_cookie.html")

	} else if ctx.req.method == .post {
		image := upload_image(mut ctx, 'cookies') or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
		}

		grade := models.Grade.from(ctx.form['grade']) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
		}

		date_str := ctx.form['release_date']
		release_date := if date_str != '' {
			time.parse('${date_str} 00:00:00') or {
				ctx.res.set_status(.bad_request)
				return ctx.text('Invalid release date, expected YYYY-MM-DD')
			}
		} else {
			time.now()
		}

		lang_str := ctx.form['lang'] or { ctx.lang }
		cookie_id := database.create_cookie(wapp.db, database.CreateCookieParams{
			lang:         lang_str
			name:         ctx.form['name']
			abilities:    ctx.form['abilities']
			description:  ctx.form['description']
			grade:        grade
			image:        if image == '' {
				none
			} else {
				image
			}
			power_plus:   ctx.form['power_plus']
			release_date: release_date
		}) or {
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
	return $veb.html("./templates/views/cookie.html")
}
