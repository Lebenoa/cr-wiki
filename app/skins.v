module app

import veb
import database

// skins renders the costume catalog: every cookie/pet skin with its grade and
// collab badge, seeded from cookierundb.
pub fn (mut wapp App) skins(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('skins_page_title')
	ctx.set_translate_desc('skins_page_description')
	skin_views := database.select_skins(wapp.db, ctx.lang)
	return $veb.html()
}
