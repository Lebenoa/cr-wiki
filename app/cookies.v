module app

import veb
import api
import database
import database.models

pub fn (mut wapp App) cookies(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('cookies_page_title')
	ctx.set_translate_desc('cookies_page_description')
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
		ctx.noindex = true
		languages := api.available_lang()
		grades := models.grade_values
		state := CookieForm{
			lang:         ctx.lang
			grade:        'c'
			treasures:    database.treasure_options(wapp.db, ctx.lang, false) or { [] }
			partners:     database.pet_options(wapp.db, ctx.lang) or { [] }
			partner_kind: 'pet'
			effect_names: database.effect_names(wapp.db, ctx.lang) or { [] }
		}
		return $veb.html('./templates/admin/new_cookie.html')
	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'cookie_form') {
			ctx.res.set_status(.forbidden)
			return ctx.text('forbidden')
		}

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
pub fn (mut wapp App) cookie_info(mut ctx Context, id int) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	cookie := database.get_cookie(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	// the treasure unlocked by upgrading this cookie to max level (none for
	// cookies without one)
	mut unlocked_treasure := ?database.TreasureView(none)
	if t := database.get_unlocked_treasure(wapp.db, ctx.lang, 'cookie', cookie.cookie_id) {
		unlocked_treasure = t
	}
	// combo bonuses pairing this cookie with a pet (empty when it has none)
	combi_bonus := database.get_combi_bonus(wapp.db, ctx.lang, 'cookie', cookie.cookie_id) or { [] }
	ctx.set_translate_title('entity_detail_title', cookie.name)
	ctx.set_translate_desc('entity_detail_description', cookie.name)
	ctx.set_og_image('cookies', cookie.image)
	is_admin := ctx.is_admin()
	// rich text: [[Cookie Name]] links + {color:x} spans in prose fields
	description_html := render_rich_text(wapp.db, ctx.lang, cookie.description)
	power_plus_html := render_rich_text(wapp.db, ctx.lang, cookie.power_plus)
	power_plus_requirement_html := render_rich_text(wapp.db, ctx.lang, cookie.power_plus_requirement)
	unlock_goal_html := render_rich_text(wapp.db, ctx.lang, cookie.unlock_goal)
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
		ctx.noindex = true
		languages := api.available_lang()
		grades := models.grade_values
		rd := cookie.release_date
		unlock_tid := if ut := database.get_unlocked_treasure(wapp.db, ctx.lang, 'cookie',
			cookie.cookie_id) {
			ut.treasure_id
		} else {
			0
		}
		state := CookieForm{
			edit_mode:         true
			id:                cookie.cookie_id
			name:              cookie.name
			abilities:         cookie.abilities
			description:       cookie.description
			power_plus:        cookie.power_plus
			grade:             cookie.grade.str()
			release_date:      '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
			lang:              cookie.lang
			image:             cookie.image
			unlock_treasure_id: unlock_tid
			treasures:         database.treasure_options(wapp.db, ctx.lang, false) or { [] }
			combis:            database.combi_edit_rows(wapp.db, ctx.lang, 'cookie', id) or { [] }
			partners:          database.pet_options(wapp.db, ctx.lang) or { [] }
			partner_kind:      'pet'
			effect_names:      database.effect_names(wapp.db, ctx.lang) or { [] }
		}
		return $veb.html('./templates/admin/new_cookie.html')
	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'cookie_form') {
			ctx.res.set_status(.forbidden)
			return ctx.text('forbidden')
		}

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
