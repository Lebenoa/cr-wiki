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

@['/cookies/:id']
pub fn (wapp &App) cookie_info(mut ctx Context, id int) veb.Result {
	cookie := database.get_cookie(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	ctx.page_title = '${cookie.name} | Classic FanWiki'
	return $veb.html("./templates/views/cookie.html")
}
