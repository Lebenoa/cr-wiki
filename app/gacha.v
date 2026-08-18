module app

import veb
import database.models
import database

// gacha renders every disclosed draw pool (treasure-draw chest tiers and
// pet-hatch egg tiers) with each prize's grade and odds, seeded from
// cookierundb.
pub fn (mut wapp App) gacha(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('gacha_page_title')
	ctx.set_translate_desc('gacha_page_description')
	pools := database.select_gacha(wapp.db, ctx.lang)
	return $veb.html()
}

// grade_img_int maps a stored grade value to its badge image name, so a view
// holding the int (gacha entries, ingredients) can render the same
// /img/grades/<grade>.png badge the treasure and cookie cards use. Empty when
// the grade is missing or out of range — the template then skips the badge.
pub fn (ctx &Context) grade_img_int(grade ?int) string {
	if g := grade {
		gr := models.Grade.from(g) or { return '' }
		return gr.str()
	}
	return ''
}

// gacha_tier_label localizes a draw-pool tier code (normal/great/supreme for
// the treasure chests, hatch/special/luxury for the pet eggs).
pub fn (ctx &Context) gacha_tier_label(tier string) string {
	return veb.tr(ctx.lang, 'gacha_tier_${tier}')
}
