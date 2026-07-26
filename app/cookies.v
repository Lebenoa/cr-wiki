module app

import veb
import database

pub fn (wapp &App) cookies(mut ctx Context) veb.Result {
	ctx.set_translate_title("cookies_page_title")
	cookies := database.select_cookies(wapp.db, ctx.lang) or {
		println(err)
		return ctx.html("Something went wrong")
	}

	return $veb.html()
}
