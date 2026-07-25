module app

import veb
import database
import database.models

pub fn (wapp &App) cookies(mut ctx Context) veb.Result {
	ctx.set_translate_title("cookies_page_title")
	mut cookies := []models.Cookie{}
	cookies = database.select_cookies(wapp.db) or {
		println(err)
		return ctx.html("Something went wrong")
	}

	return $veb.html()
}
