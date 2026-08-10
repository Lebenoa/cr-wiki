module app

import veb
import time
import api
import database

pub fn (wapp &App) treasures(mut ctx Context) veb.Result {
	ctx.set_translate_title("treasures_page_title")
	page_size := 30

	mut tab := ctx.query['tab'] or { 'normal' }
	if tab != 'normal' && tab != 'evo' {
		tab = 'normal'
	}
	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	treasures := database.select_treasures(wapp.db, ctx.lang, page_size, (page - 1) * page_size, tab == 'evo') or {
		println(err)
		return ctx.html("Something went wrong")
	}
	next_page := if treasures.len == page_size {
		page + 1
	} else {
		0
	}

	pill := 'rounded-full px-4 py-2 font-bold transition-colors'
	normal_cls := if tab == 'normal' {
		'${pill} bg-primary text-foreground'
	} else {
		'${pill} border-2 border-secondary/50 text-secondary'
	}
	evo_cls := if tab == 'evo' {
		'${pill} bg-primary text-foreground'
	} else {
		'${pill} border-2 border-secondary/50 text-secondary'
	}

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html("./templates/partials/treasure_cards.html")
	}

	return $veb.html()
}

@['/treasures/:id']
pub fn (wapp &App) treasure_info(mut ctx Context, id int) veb.Result {
	treasure := database.get_treasure(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	effects := database.get_treasure_effects(wapp.db, ctx.lang, id) or { [] }
	// evolved rows link to their base; base treasures with an evolved form link
	// to it (the normal-state evolved row)
	mut base_treasure := ?database.TreasureView(none)
	if bid := treasure.base_treasure_id {
		if b := database.get_treasure_base(wapp.db, ctx.lang, bid) {
			base_treasure = b
		}
	}
	mut evo_treasure := ?database.TreasureView(none)
	if !treasure.is_evolved {
		if e := database.get_treasure_evo(wapp.db, ctx.lang, treasure.treasure_id) {
			evo_treasure = e
		}
	}
	ctx.page_title = '${treasure.name} | Classic Fan/Wiki'
	return $veb.html("./templates/views/treasure.html")
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
		return $veb.html("./templates/admin/new_treasure.html")

	} else if ctx.req.method == .post {
		image := upload_image(mut ctx, 'treasures') or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
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
		database.create_treasure(wapp.db, database.CreateTreasureParams{
			lang:         lang_str
			name:         ctx.form['name']
			description:  ctx.form['description']
			image:        if image == '' {
				none
			} else {
				image
			}
			is_evolved:   ctx.form['is_evolved'] == 'true'
			is_blessed:   ctx.form['is_blessed'] == 'true'
			release_date: release_date
		}) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Failed to create treasure: ${err}')
		}

		return ctx.redirect('/treasures')
	}

	return ctx.not_found()
}
