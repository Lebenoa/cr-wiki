module app

import veb
import config
import db.sqlite
import os
import database.models

const lang_cookie_key = 'wikilang'
const session_cookie_key = 'CRSESSID'

pub struct Context {
	veb.Context
pub mut:
	lang       string = 'en'
	page_title string
	page_desc  string
	og_image   string
	noindex    bool
	user       ?models.User
}

pub struct App {
	veb.StaticHandler
	veb.Middleware[Context]

	db sqlite.DB
	// per-IP rate-limit tuning from the [ratelimit] table of Config.toml
	rate_cfg config.RateLimitConfig
mut:
	// in-memory login sessions (cookie -> user), keyed by CRSESSID. Shared
	// fields carry their own lock; access goes through lock/rlock blocks.
	sessions shared map[string]models.User
	// per-IP rate-limit buckets for the public read endpoints
	rate_buckets shared map[string]RateBucket
}

pub fn initialize(conn sqlite.DB) !&App {
	cfg := config.load() or { config.Config{} }
	mut new_app := &App{
		db:       conn
		rate_cfg: cfg.ratelimit
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

// set_translate_desc fills the meta description from a .tr key, with an
// optional {name} placeholder like set_translate_title.
pub fn (mut ctx Context) set_translate_desc(key string, name ?string) {
	desc := veb.tr(ctx.lang, key)
	ctx.page_desc = if n := name {
		desc.replace('{name}', n)
	} else {
		desc
	}
}

// meta_description returns the page description or the site-wide fallback.
pub fn (ctx &Context) meta_description() string {
	if ctx.page_desc != '' {
		return ctx.page_desc
	}
	return veb.tr(ctx.lang, 'site_description')
}

// set_og_image records an absolute Open Graph image URL for an entity image.
pub fn (mut ctx Context) set_og_image(dir string, image ?string) {
	if img := image {
		ctx.og_image = ctx.site_url() + '/img/${dir}/${img}'
	}
}

// site_url returns the public origin used for canonical/OG URLs. Derived
// from the request Host header so it stays correct behind proxies/domains;
// CR_BASE_URL still wins when set (Host can be spoofed, so an explicit
// override is the trusted option).
pub fn (ctx &Context) site_url() string {
	base := os.getenv('CR_BASE_URL')
	if base != '' {
		return base.trim_right('/')
	}
	scheme := if host := ctx.get_custom_header('X-Forwarded-Proto') {
		host
	} else {
		'http'
	}
	if host := ctx.get_custom_header('Host') {
		return '${scheme}://${host}'
	}
	return 'http://localhost:6785'
}

// canonical_url is the clean (query-free) absolute URL of the current page.
pub fn (ctx &Context) canonical_url() string {
	return '${ctx.site_url()}${ctx.req.url.split('?')[0]}'
}

// og_locale maps the wikilang cookie to an Open Graph locale tag.
pub fn (ctx &Context) og_locale() string {
	return if ctx.lang == 'th' { 'th_TH' } else { 'en_US' }
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
	return 'https://placehold.co/600x600'
}

pub fn (ctx &Context) cookie_img_src(image ?string) string {
	return ctx.img_src('cookies', image)
}

pub fn (ctx &Context) nav_link(href string, tr_key ?string) veb.RawHtml {
	active := ctx.req.url == '/${href}' || ctx.req.url.starts_with('/' + href + '/')

	class := if active {
		'font-semibold text-center text-primary border-b-2 border-primary pb-1 transition-all duration-1000'
	} else {
		'font-medium text-center text-foreground-muted hover:text-primary transition-all duration-1000'
	}

	abs_tr_key := tr_key or { href }

	return '<a class="${class}" href="/${href}">${veb.tr(ctx.lang, abs_tr_key)}</a>'
}

// theme_options lists the selectable themes. The palettes live in the UnoCSS
// preflight (`html[data-theme="..."]` blocks in uno.config.ts); the dropdown
// just toggles the `data-theme` attribute (see static/js/theme.js).
pub fn (ctx &Context) theme_options() []string {
	return ['default', 'light', 'tokyo_night', 'cappuccino', 'dracula', 'nord', 'gruvbox', 'rose_pine']
}

// theme_label localizes a theme name (theme_* keys in the .tr files).
pub fn (ctx &Context) theme_label(name string) string {
	return veb.tr(ctx.lang, 'theme_${name}')
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
		mut has_session := false
		mut user := models.User{}
		rlock wapp.sessions {
			has_session = user_cookie.value in wapp.sessions
			user = wapp.sessions[user_cookie.value]
		}
		// struct-field map index yields the value (zero struct on a miss), not
		// an Option, so presence must be checked with `in` — a garbage cookie
		// must not look logged in
		if has_session {
			user
		} else {
			none
		}
	} else {
		none
	}

	return true
}

pub fn after_request(mut ctx Context) bool {
	println('${ctx.req.method} ${ctx.req.url} ${ctx.res.status()}')
	return true
}
