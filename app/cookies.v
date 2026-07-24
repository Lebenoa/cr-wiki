module app

import veb

pub fn (wapp &App) cookies(mut ctx Context) veb.Result {
	ctx.set_translate_title("cookies_page_title")
	return $veb.html()
}