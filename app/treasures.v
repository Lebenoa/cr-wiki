module app

import veb
import api
import database

pub fn (wapp &App) treasures(mut ctx Context) veb.Result {
	ctx.set_translate_title("treasures_page_title")
	page_size := 30

	mut tab := ctx.query['tab'] or { 'all' }
	if tab !in ['all', 'normal', 'evo'] {
		tab = 'all'
	}
	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	treasures := database.select_treasures(wapp.db, ctx.lang, page_size, (page - 1) * page_size, tab) or {
		println(err)
		return ctx.html("Something went wrong")
	}
	next_page := if treasures.len == page_size {
		page + 1
	} else {
		0
	}

	all_cls := pill_cls(tab == 'all')
	normal_cls := pill_cls(tab == 'normal')
	evo_cls := pill_cls(tab == 'evo')

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html("./templates/partials/treasure_cards.html")
	}

	return $veb.html()
}

// pill_cls returns the pill button classes for the treasure list tabs
fn pill_cls(active bool) string {
	base := 'rounded-full px-4 py-2 font-bold transition-colors'
	return if active {
		'${base} bg-primary text-foreground'
	} else {
		'${base} border-2 border-secondary/50 text-secondary'
	}
}

@['/treasures/:id']
pub fn (wapp &App) treasure_info(mut ctx Context, id int) veb.Result {
	treasure := database.get_treasure(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	effects := database.get_treasure_effects(wapp.db, ctx.lang, id) or { [] }
	// blessed-state effects (same table, state column); empty for non-evolved
	blessed_effects := database.get_treasure_blessed_effects(wapp.db, ctx.lang, id) or { [] }
	// the linked variant is the base for evolved rows, the evolved form for
	// normal rows; the two are mutually exclusive, so a single optional
	// suffices (the template labels the panel via treasure.is_evolved)
	mut variant_treasure := ?database.TreasureView(none)
	if bid := treasure.base_treasure_id {
		if b := database.get_treasure_base(wapp.db, ctx.lang, bid) {
			variant_treasure = b
		}
	} else if !treasure.is_evolved {
		if e := database.get_treasure_evo(wapp.db, ctx.lang, treasure.treasure_id) {
			variant_treasure = e
		}
	}
	ctx.page_title = '${treasure.name} | Classic Fan/Wiki'
	is_admin := ctx.is_admin()
	return $veb.html("./templates/views/treasure.html")
}

@['/treasures/:id/edit'; get; post]
pub fn (wapp &App) edit_treasure(mut ctx Context, id int) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	treasure := database.get_treasure(wapp.db, ctx.lang, id) or { return ctx.not_found() }

	if ctx.req.method == .get {
		ctx.page_title = 'Edit ${treasure.name} | Classic/FanWiki'
		languages := api.available_lang()
		edit_mode := true
		entity_id := treasure.treasure_id
		entity_name := treasure.name
		entity_description := treasure.description
		entity_is_evolved := treasure.is_evolved
		rd := treasure.release_date
		entity_release_date := '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
		entity_lang := treasure.lang
		return $veb.html("./templates/admin/new_treasure.html")

	} else if ctx.req.method == .post {
		params := parse_treasure_form(mut ctx) or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
		}

		database.update_treasure(wapp.db, id, params) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Failed to update treasure: ${err}')
		}

		return ctx.redirect('/treasures/${id}')
	}

	return ctx.not_found()
}

@['/treasures/new'; get; post]
pub fn (wapp &App) new_treasure(mut ctx Context) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	if ctx.req.method == .get {
		ctx.page_title = 'New Treasure | Classic/FanWiki'
		languages := api.available_lang()
		edit_mode := false
		entity_id := 0
		entity_name := ''
		entity_description := ''
		entity_is_evolved := false
		entity_release_date := ''
		entity_lang := ctx.lang
		return $veb.html("./templates/admin/new_treasure.html")

	} else if ctx.req.method == .post {
		params := parse_treasure_form(mut ctx) or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
		}

		database.create_treasure(wapp.db, params) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Failed to create treasure: ${err}')
		}

		return ctx.redirect('/treasures')
	}

	return ctx.not_found()
}
