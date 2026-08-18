module app

import veb
import database

// crafting renders the ingredient index for treasure crafting: every
// ingredient with the number of treasures it crafts, seeded from cookierundb's
// used_in_recipes links.
pub fn (mut wapp App) crafting(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('crafting_page_title')
	ctx.set_translate_desc('crafting_page_description')
	ingredients := database.select_ingredients(wapp.db, ctx.lang)
	return $veb.html()
}

// ingredient_info renders one ingredient's detail: its catalog facts and the
// treasures it is used to craft.
@['/crafting/:id']
pub fn (mut wapp App) ingredient_info(mut ctx Context, id int) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ingredient := database.select_ingredient(wapp.db, ctx.lang, id) or {
		return ctx.not_found()
	}
	ctx.set_translate_title('crafting_ingredient_title', ingredient.name)
	ctx.set_translate_desc('crafting_ingredient_description', ingredient.name)
	ctx.set_og_image('ingredients', ingredient.image)
	return $veb.html('./templates/views/ingredient.html')
}

// episode_short renders an episode id as the short badge the ingredient cards
// and the drop-location tile use: "EP 2" for a story episode, "SP 1" for a
// special one (ids 501+), "EP 6-1" for an event episode nested under a story
// one (ids 601/602/701). Empty for none, so a template can test it directly.
pub fn (ctx &Context) episode_short(id ?int) string {
	eid := id or { return '' }
	if eid >= 500 && eid < 600 {
		return '${veb.tr(ctx.lang, 'special_episode_abbrev')} ${eid - 500}'
	}
	if eid >= 600 {
		return '${veb.tr(ctx.lang, 'episode_abbrev')} ${eid / 100}-${eid % 100}'
	}
	return '${veb.tr(ctx.lang, 'episode_abbrev')} ${eid}'
}
