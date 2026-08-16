module app

import veb

pub fn (wapp &App) index(mut ctx Context) veb.Result {
	ctx.set_translate_title("index_page_title")
	ctx.set_translate_desc("index_page_description")
	return $veb.html()
}