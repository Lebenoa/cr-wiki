module app

import veb
import database

@['/cookie/:id']
pub fn (wapp &App) cookie(mut ctx Context, id int) veb.Result {
	cookie := database.get_cookie(wapp.db, ctx.lang, id) or { panic(err) }
	return $veb.html("./templates/views/cookie.html")
}
