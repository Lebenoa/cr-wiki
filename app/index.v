module app

import veb

pub fn (wapp App) index(mut ctx Context) veb.Result {
	ctx.set_translate_title("index_title")
	return $veb.html()
}