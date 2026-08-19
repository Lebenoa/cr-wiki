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
pub fn (mut wapp App) new_cookie(mut ctx Context) veb.Result {
	if !ctx.is_admin() {
		return ctx.not_found()
	}

	if ctx.req.method == .get {
		ctx.set_translate_title('new_cookie_page_title')
		ctx.noindex = true
		languages := api.available_lang()
		grades := models.grade_values
		state := cookie_form_state(wapp, ctx, none)
		return $veb.html('./templates/admin/new_cookie.html')
	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'cookie_form') {
			ctx.res.set_status(.forbidden)
			return cookie_submit_error(wapp, ctx, none, veb.tr(ctx.lang, 'turnstile_form_failed'))
		}

		params := parse_cookie_form(mut wapp, mut ctx) or {
			ctx.res.set_status(.bad_request)
			return cookie_submit_error(wapp, ctx, none, err.msg())
		}

		cookie_id := database.create_cookie(wapp.db, params) or {
			ctx.res.set_status(.bad_request)
			return cookie_submit_error(wapp, ctx, none, 'Failed to create cookie: ${err}')
		}
		// picker lists cache the catalog; a write has to show up next request
		wapp.invalidate_options()

		return submit_success(mut ctx, '/cookies/${cookie_id}')
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
pub fn (mut wapp App) edit_cookie(mut ctx Context, id int) veb.Result {
	if !ctx.is_admin() {
		return ctx.not_found()
	}

	cookie := database.get_cookie(wapp.db, ctx.lang, id) or { return ctx.not_found() }

	if ctx.req.method == .get {
		ctx.set_translate_title('edit_cookie_page_title', cookie.name)
		ctx.noindex = true
		languages := api.available_lang()
		grades := models.grade_values
		state := cookie_form_state(wapp, ctx, cookie)
		return $veb.html('./templates/admin/new_cookie.html')
	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'cookie_form') {
			ctx.res.set_status(.forbidden)
			return cookie_submit_error(wapp, ctx, cookie, veb.tr(ctx.lang, 'turnstile_form_failed'))
		}

		params := parse_cookie_form(mut wapp, mut ctx) or {
			ctx.res.set_status(.bad_request)
			return cookie_submit_error(wapp, ctx, cookie, err.msg())
		}

		database.update_cookie(wapp.db, id, params) or {
			ctx.res.set_status(.bad_request)
			return cookie_submit_error(wapp, ctx, cookie, 'Failed to update cookie: ${err}')
		}
		// picker lists cache the catalog; a write has to show up next request
		wapp.invalidate_options()

		return submit_success(mut ctx, '/cookies/${id}')
	}

	return ctx.not_found()
}
