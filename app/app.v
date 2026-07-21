module app

import veb

pub struct Context {
	veb.Context

pub mut:
	lang string = "en"
	page_title string
}

pub struct App {
	veb.StaticHandler
}

pub fn initialize() !&App {
	mut new_app := &App{}
	new_app.handle_static("static", true)!
	return new_app
}

pub fn (mut ctx Context) set_translate_title(key string) {
	ctx.page_title = veb.tr(ctx.lang, key)
}

pub fn (mut ctx Context) not_found() veb.Result {
    // set HTTP status 404
    ctx.res.set_status(.not_found)
	return $veb.html()
}