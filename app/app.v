module app

import veb
import db.sqlite
import database.models

const lang_cookie_key = 'wikilang'
const session_cookie_key = 'CRSESSID'

__global (
	sessions shared map[string]models.User
)

pub struct Context {
	veb.Context
pub mut:
	lang       string = 'en'
	page_title string
	user ?models.User
}

pub struct App {
	veb.StaticHandler
	veb.Middleware[Context]

	db sqlite.DB
mut:
}

pub fn initialize(conn sqlite.DB) !&App {
	mut new_app := &App{
		db: conn,
	}
	new_app.static_mime_types['.avif'] = 'image/avif'
	new_app.handle_static('static', true)!
	new_app.use(handler: new_app.before_request)
	// new_app.use(handler: after_request, after: true)
	return new_app
}

pub fn (mut ctx Context) set_translate_title(key string, name ?string) {
	title := veb.tr(ctx.lang, key)
	ctx.page_title = if n := name {
		title.replace('{name}', n)
	} else {
		title
	}
}

pub fn (mut ctx Context) not_found() veb.Result {
	// set HTTP status 404
	ctx.res.set_status(.not_found)
	ctx.set_translate_title('not_found_title')
	return $veb.html()
}

// img_src returns the local image path for an entity, or a placeholder URL when missing.
pub fn (ctx &Context) img_src(dir string, image ?string) string {
	if img := image {
		return '/img/${dir}/${img}'
	}
	return 'https://placehold.co/600x400'
}

pub fn (ctx &Context) cookie_img_src(image ?string) string {
	return ctx.img_src('cookies', image)
}

pub fn (ctx &Context) nav_link(href string, tr_key ?string) veb.RawHtml {
	active := ctx.req.url == '/${href}' || ctx.req.url.starts_with('/' + href + '/')

	class := if active {
		'font-semibold text-primary border-b-2 border-primary pb-1 transition-all duration-1000'
	} else {
		'font-medium text-foreground-muted hover:text-primary transition-all duration-1000'
	}

	abs_tr_key := tr_key or { href }

	return '<a class="${class}" href="/${href}">${veb.tr(ctx.lang, abs_tr_key)}</a>'
}

// grade_name returns the full grade name (e.g. "Extra" for e, "Legend" for l)
// for use in tooltips; other grades keep their letter.
pub fn (ctx &Context) grade_name(g string) string {
	return models.grade_name(g)
}

// is_admin reports whether the current session belongs to an admin user.
pub fn (ctx &Context) is_admin() bool {
	if user := ctx.user {
		return user.is_admin
	}
	return false
}

pub fn (ctx &Context) is_htmx_request() bool {
	return if _ := ctx.get_custom_header("HX-Request") {
		true
	} else {
		false
	}
}

// is_boosted_request reports whether the request came from htmx's hx-boost
// navigation (full-page swap), as opposed to a fragment fetch like the
// infinite-scroll sentinel. Boosted nav must get the full page, not a partial.
pub fn (ctx &Context) is_boosted_request() bool {
	return if _ := ctx.get_custom_header("HX-Boosted") {
		true
	} else {
		false
	}
}

pub fn (mut wapp App) before_request(mut ctx Context) bool {
	ctx.lang = if lang_cookie := ctx.req.cookie('wikilang') {
		lang_cookie.value
	} else {
		ctx.set_cookie(
			name:  lang_cookie_key
			value: 'en'
			path:  '/'
		)
		'en'
	}

	ctx.user = if user_cookie := ctx.req.cookie(session_cookie_key) {
		println("Found cookie: ${user_cookie}")
		if user := sessions[user_cookie.value] {
			println("Found user: ${user}")
			user
		} else {
			println("No user found for cookie: ${user_cookie.value}")
			none
		}
	} else {
		println("No cookie found")
		none
	}

	return true
}

pub fn after_request(mut ctx Context) bool {
	println('${ctx.req.method} ${ctx.req.url} ${ctx.res.status()}')
	return true
}
