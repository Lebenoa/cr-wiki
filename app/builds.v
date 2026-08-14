module app

import veb
import database

// BuildSelection is the loadout picked on the builds page: one cookie, one pet
// and up to three treasures (0 = not picked). Carried in the query string so
// builds are shareable via URL.
struct BuildSelection {
pub:
	cookie    int
	pet       int
	treasure1 int
	treasure2 int
	treasure3 int
}

// BuildPreview is the resolved loadout rendered by build_preview.html: the
// picked entities plus the combo bonus of the cookie+pet pair, if any.
pub struct BuildPreview {
pub mut:
	has_selection bool
	cookie        ?database.IdNameOption
	pet           ?database.IdNameOption
	treasures     []database.IdNameOption
	combi         ?database.CombiBonusView
}

fn resolve_option(options []database.IdNameOption, id int) ?database.IdNameOption {
	if id <= 0 {
		return none
	}
	for o in options {
		if o.id == id {
			return o
		}
	}
	return none
}

fn build_preview(wapp &App, ctx &Context, sel BuildSelection, cookies []database.IdNameOption, pets []database.IdNameOption, treasures []database.IdNameOption) BuildPreview {
	mut preview := BuildPreview{
		cookie: resolve_option(cookies, sel.cookie)
		pet:    resolve_option(pets, sel.pet)
	}
	for tid in [sel.treasure1, sel.treasure2, sel.treasure3] {
		if o := resolve_option(treasures, tid) {
			preview.treasures << o
		}
	}
	preview.has_selection = preview.cookie != none || preview.pet != none || preview.treasures.len > 0
	if c := preview.cookie {
		if p := preview.pet {
			combis := database.get_combi_bonus(wapp.db, ctx.lang, 'cookie', c.id) or { [] }
			for cb in combis {
				if cb.partner_kind == 'pet' && cb.partner_id == p.id {
					preview.combi = cb
					break
				}
			}
		}
	}
	return preview
}

fn selection_from_query(ctx &Context) BuildSelection {
	return BuildSelection{
		cookie:    (ctx.query['cookie'] or { '' }).int()
		pet:       (ctx.query['pet'] or { '' }).int()
		treasure1: (ctx.query['t1'] or { '' }).int()
		treasure2: (ctx.query['t2'] or { '' }).int()
		treasure3: (ctx.query['t3'] or { '' }).int()
	}
}

// builds renders the build-planner page. htmx requests (select changes) get
// only the preview partial so the URL can hold the whole build.
@['/builds']
pub fn (wapp &App) builds(mut ctx Context) veb.Result {
	ctx.set_translate_title('builds_page_title')
	sel := selection_from_query(ctx)
	cookies := database.cookie_options(wapp.db, ctx.lang) or { [] }
	pets := database.pet_options(wapp.db, ctx.lang) or { [] }
	treasures := database.treasure_options(wapp.db, ctx.lang) or { [] }
	preview := build_preview(wapp, ctx, sel, cookies, pets, treasures)

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html('./templates/components/build_preview.html')
	}
	return $veb.html()
}
