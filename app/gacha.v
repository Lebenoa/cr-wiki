module app

import veb
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

// gacha_tier_label localizes a draw-pool tier code (normal/great/supreme for
// the treasure chests, hatch/special/luxury for the pet eggs).
pub fn (ctx &Context) gacha_tier_label(tier string) string {
	return veb.tr(ctx.lang, 'gacha_tier_${tier}')
}
