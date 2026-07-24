module app

import veb
import db.sqlite
import net.http

pub struct Context {
	veb.Context
pub mut:
	lang       string = 'en'
	page_title string
}

pub struct App {
	veb.StaticHandler
	veb.Middleware[Context]
	db sqlite.DB
}

pub fn initialize(conn sqlite.DB) !&App {
	mut new_app := &App{}
	new_app.static_mime_types['.avif'] = 'image/avif'
	new_app.handle_static('static', true)!
	new_app.use(handler: before_request)
	return new_app
}

pub fn (mut ctx Context) set_translate_title(key string) {
	ctx.page_title = veb.tr(ctx.lang, key)
}

pub fn (mut ctx Context) not_found() veb.Result {
	// set HTTP status 404
	ctx.res.set_status(.not_found)
	ctx.page_title = 'Why are you here? Just to suffer?'
	return $veb.html()
}

pub fn (ctx &Context) nav_link(href string, tr_key ?string) veb.RawHtml {
	active := ctx.req.url == '/${href}' || ctx.req.url.starts_with('/' + href + '/')

	class := if active {
		'font-semibold text-primary border-b-2 border-primary pb-1'
	} else {
		'font-medium text-foreground-muted hover:text-primary transition-all'
	}

	abs_tr_key := tr_key or { href }

	return '<a class="${class}" href="/${href}">${veb.tr(ctx.lang, abs_tr_key)}</a>'
}

pub fn before_request(mut ctx Context) bool {
	ctx.lang = if lang_cookie := ctx.req.cookie('wikilang') {
		lang_cookie.value
	} else {
		ctx.res.header.add(.set_cookie, http.Cookie{
			name: "wikilang"
			value: "en"
			path: '/'
		}.str())
		'en'
	}
	return true
}
