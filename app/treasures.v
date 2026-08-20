module app

import veb
import api
import database
import database.models
import app.util
import net.urllib

pub fn (mut wapp App) treasures(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('treasures_page_title')
	ctx.set_translate_desc('treasures_page_description')
	page_size := 30

	mut tab := ctx.query['tab'] or { 'all' }
	if tab !in ['all', 'normal', 'evo'] {
		tab = 'all'
	}
	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	// The page's filter is served, not client-side: the grid is paginated, so
	// hiding loaded cards would only ever search the pages scrolled to, and
	// the cards carry no effect text to match against anyway. The template
	// fires an hx-get from a form so htmx serializes the query and the tab
	// exactly once, with submit suppressed so Enter does not navigate.
	//
	// q stays whole for the sentinel URL so the next page repeats the query;
	// terms is the split form the matcher uses
	q := (ctx.query['q'] or { '' }).trim_space().to_lower()
	terms := q.fields()

	mut treasures := []database.TreasureView{}
	mut next_page := 0
	if terms.len == 0 {
		treasures = database.select_treasures(wapp.db, ctx.lang, page_size, (page - 1) * page_size,
			tab) or {
			println(err)
			return ctx.html('Something went wrong')
		}
		if treasures.len == page_size {
			next_page = page + 1
		}
	} else {
		// searching goes through the cached option list rather than SQL: it
		// already carries every effect line for every treasure, in this page's
		// own order (grade, then newest, then name), so the list answers
		// "magnet revive" exactly the way the build picker does — across the
		// whole catalog, not just the pages scrolled to.
		mut ids := []int{}
		for opt in wapp.treasure_options(ctx.lang, false) {
			if picker_tab_ok(opt, tab) && picker_matches(opt, terms) {
				ids << opt.id
			}
		}
		start, end, ok := slice_page(page, page_size, ids.len)
		if ok {
			treasures = database.select_treasures_by_ids(wapp.db, ctx.lang, ids[start..end]) or {
				println(err)
				return ctx.html('Something went wrong')
			}
			if end < ids.len {
				next_page = page + 1
			}
		}
	}
	next_url := treasure_page_url(tab, q, next_page)

	all_cls := pill_cls(tab == 'all')
	normal_cls := pill_cls(tab == 'normal')
	evo_cls := pill_cls(tab == 'evo')

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html('./templates/components/treasure_cards.html')
	}

	return $veb.html()
}

// treasure_page_url is the infinite-scroll sentinel's href: the same tab and
// query one page further on. Empty when there is no next page.
fn treasure_page_url(tab string, q string, next_page int) string {
	if next_page == 0 {
		return ''
	}
	return '/treasures?tab=${urllib.query_escape(tab)}&q=${urllib.query_escape(q)}&page=${next_page}'
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

@['/treasures/new'; get; post]
pub fn (mut wapp App) new_treasure(mut ctx Context) veb.Result {
	if !ctx.is_admin() {
		return ctx.not_found()
	}

	if ctx.req.method == .get {
		ctx.set_translate_title('new_treasure_page_title')
		ctx.noindex = true
		languages := api.available_lang()
		grades := models.grade_values
		state := treasure_form_state(wapp, ctx, none)
		return $veb.html('./templates/admin/new_treasure.html')
	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'treasure_form') {
			ctx.res.set_status(.forbidden)
			return treasure_submit_error(wapp, ctx, none, veb.tr(ctx.lang, 'turnstile_form_failed'))
		}

		params := parse_treasure_form(mut wapp, mut ctx) or {
			ctx.res.set_status(.bad_request)
			return treasure_submit_error(wapp, ctx, none, err.msg())
		}

		database.create_treasure(wapp.db, params) or {
			ctx.res.set_status(.bad_request)
			return treasure_submit_error(wapp, ctx, none, 'Failed to create treasure: ${err}')
		}
		// picker lists cache the catalog; a write has to show up next request
		wapp.invalidate_options()

		return submit_success(mut ctx, '/treasures')
	}

	return ctx.not_found()
}

@['/treasures/:id']
pub fn (mut wapp App) treasure_info(mut ctx Context, id int) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	treasure := database.get_treasure(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	effects := database.get_treasure_effects(wapp.db, ctx.lang, id) or { [] }
	// blessed-state effects (same table, state column); empty for non-evolved.
	// Their columns carry the per-level delta vs the normal state.
	blessed_effects := util.blessed_diffs(effects,
		database.get_treasure_blessed_effects(wapp.db, ctx.lang, id) or { [] })
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
	// the cookie/pet whose max-level upgrade unlocks this treasure (none for
	// chest/event treasures)
	mut unlock_entity := ?database.TreasureUnlock(none)
	if u := database.get_treasure_unlock(wapp.db, ctx.lang, treasure) {
		unlock_entity = u
	}
	ctx.set_translate_title('entity_detail_title', treasure.name)
	ctx.set_translate_desc('entity_detail_description', treasure.name)
	ctx.set_og_image('treasures', treasure.image)
	is_admin := ctx.is_admin()
	// rich text: [[Cookie Name]] links + {color:x} spans in the description
	description_html := render_rich_text(wapp.db, ctx.lang, treasure.description)
	// the ingredients this treasure is crafted from (empty for non-recipe rows).
	// Every recipe hangs off the evolved row — no base treasure has one — so a
	// non-evolved page shows what its evolution costs instead, flagged so the
	// template can say so.
	mut craft_ingredients := database.select_treasure_ingredients(wapp.db, ctx.lang, id)
	mut craft_is_evolution := false
	if craft_ingredients.len == 0 && !treasure.is_evolved {
		if evo := variant_treasure {
			craft_ingredients = database.select_treasure_ingredients(wapp.db, ctx.lang,
				evo.treasure_id)
			craft_is_evolution = craft_ingredients.len > 0
		}
	}
	return $veb.html('./templates/views/treasure.html')
}

@['/treasures/:id/edit'; get; post]
pub fn (mut wapp App) edit_treasure(mut ctx Context, id int) veb.Result {
	if !ctx.is_admin() {
		return ctx.not_found()
	}

	treasure := database.get_treasure(wapp.db, ctx.lang, id) or { return ctx.not_found() }

	if ctx.req.method == .get {
		ctx.set_translate_title('edit_treasure_page_title', treasure.name)
		ctx.noindex = true
		languages := api.available_lang()
		grades := models.grade_values
		state := treasure_form_state(wapp, ctx, treasure)
		return $veb.html('./templates/admin/new_treasure.html')
	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'treasure_form') {
			ctx.res.set_status(.forbidden)
			return treasure_submit_error(wapp, ctx, treasure, veb.tr(ctx.lang, 'turnstile_form_failed'))
		}

		params := parse_treasure_form(mut wapp, mut ctx) or {
			ctx.res.set_status(.bad_request)
			return treasure_submit_error(wapp, ctx, treasure, err.msg())
		}

		database.update_treasure(wapp.db, id, params) or {
			ctx.res.set_status(.bad_request)
			return treasure_submit_error(wapp, ctx, treasure, 'Failed to update treasure: ${err}')
		}
		// picker lists cache the catalog; a write has to show up next request
		wapp.invalidate_options()

		return submit_success(mut ctx, '/treasures/${id}')
	}

	return ctx.not_found()
}
