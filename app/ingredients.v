module app

import veb
import database

// ingredients renders the ingredient index for treasure crafting: every
// ingredient with the number of treasures it crafts, seeded from cookierundb's
// used_in_recipes links.
pub fn (mut wapp App) ingredients(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('ingredients_page_title')
	ctx.set_translate_desc('ingredients_page_description')
	ingredients := database.select_ingredients(wapp.db, ctx.lang)
	return $veb.html()
}

// ingredient_info renders one ingredient's detail: its catalog facts and the
// treasures it is used to craft.
@['/ingredients/:id']
pub fn (mut wapp App) ingredient_info(mut ctx Context, id int) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ingredient := database.select_ingredient(wapp.db, ctx.lang, id) or {
		return ctx.not_found()
	}
	ctx.set_translate_title('ingredient_detail_title', ingredient.name)
	ctx.set_translate_desc('ingredient_detail_description', ingredient.name)
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

// episode_badge_cls returns the full badge class list for an episode id, so
// each episode reads as its own colour on the ingredient cards. Story
// episodes run cool-to-warm in play order, the special episodes take the
// remaining cool hues and the event ones the reds — every id its own hue.
//
// Each arm is a whole literal (the shared base is repeated) because these
// render once per ingredient card, 242 of them on /ingredients: concatenating a
// base string per card allocated for nothing. UnoCSS scans app/*.v, so the
// colours here are generated into static/styles.css.
pub fn (ctx &Context) episode_badge_cls(id ?int) string {
	eid := id or { return '' }
	return match eid {
		1 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-emerald-400 text-emerald-400' }
		2 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-lime-400 text-lime-400' }
		3 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-orange-400 text-orange-400' }
		4 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-sky-400 text-sky-400' }
		5 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-pink-400 text-pink-400' }
		6 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-fuchsia-400 text-fuchsia-400' }
		7 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-amber-400 text-amber-400' }
		501 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-yellow-400 text-yellow-400' }
		502 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-cyan-400 text-cyan-400' }
		503 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-violet-400 text-violet-400' }
		601 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-teal-400 text-teal-400' }
		602 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-indigo-400 text-indigo-400' }
		701 { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-red-400 text-red-400' }
		else { 'text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-secondary/40 text-foreground-muted' }
	}
}

// episode_text_cls is the same palette as episode_badge_cls without the pill,
// for the drop-location tile where the number sits inline before the name.
pub fn (ctx &Context) episode_text_cls(id ?int) string {
	eid := id or { return '' }
	return match eid {
		1 { 'text-emerald-400' }
		2 { 'text-lime-400' }
		3 { 'text-orange-400' }
		4 { 'text-sky-400' }
		5 { 'text-pink-400' }
		6 { 'text-fuchsia-400' }
		7 { 'text-amber-400' }
		501 { 'text-yellow-400' }
		502 { 'text-cyan-400' }
		503 { 'text-violet-400' }
		601 { 'text-teal-400' }
		602 { 'text-indigo-400' }
		701 { 'text-red-400' }
		else { 'text-accent' }
	}
}
