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
	// url -> file path for everything under static/. veb.StaticHandler is not
	// embedded on purpose: its cache is built once at startup, while this one
	// is extended by upload_image (see app/static.v).
	static_files shared map[string]string
}

pub fn initialize(conn sqlite.DB) !&App {
	cfg := config.load() or { config.Config{} }
	mut new_app := &App{
		db:       conn
		rate_cfg: cfg.ratelimit
	}
	lock new_app.static_files {
		scan_static_dir(mut new_app.static_files, 'static')!
	}
	// static first: veb runs global middleware before routing, so a cached
	// file still wins over a route with the same path
	new_app.use(handler: new_app.serve_static)
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
	// index_u8 + one slice, instead of split('?') building a whole []string
	// just to read element 0 (runs on every rendered page)
	url := ctx.req.url
	qi := url.index_u8(`?`)
	path := if qi >= 0 { url[..qi] } else { url }
	return '${ctx.site_url()}${path}'
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
	return themes
}

// themes is a const so the navbar's theme dropdown doesn't rebuild the array
// (and its 8 strings) on every page render.
const themes = ['default', 'light', 'tokyo_night', 'cappuccino', 'dracula', 'nord', 'gruvbox',
	'rose_pine']

// theme_label localizes a theme name (theme_* keys in the .tr files).
pub fn (ctx &Context) theme_label(name string) string {
	return veb.tr(ctx.lang, 'theme_${name}')
}

// grade_label renders a grade value as its short label for tooltips and
// option text: "s_plus" -> "S+", other grades their uppercase letter.
pub fn (ctx &Context) grade_label(g string) string {
	return if g == 's_plus' { 'S+' } else { g.to_upper() }
}

// grade_badge_cls returns the full badge class list for a stored grade value
// (models.Grade), so each grade renders with its own border/text color. The
// badge base (size, pill shape, border width) is baked in so every call site
// shares one look. Returns '' when the row has no grade.
pub fn (ctx &Context) grade_badge_cls(grade ?int) string {
	if g := grade {
		gr := models.Grade.from(g) or { return '' }
		// match on the enum, not gr.str(): this renders once per card (30+ per
		// list page, plus every picker option), and .str() allocated a string
		// that was then compared against seven literals.
		// each arm is a whole literal (the shared base is repeated) so no
		// concatenation happens at render time either
		return match gr {
			.e { 'text-xs font-bold uppercase rounded-full border px-2 py-0.5 border-emerald-400 text-emerald-400' }
			.c { 'text-xs font-bold uppercase rounded-full border px-2 py-0.5 border-stone-400 text-stone-400' }
			.b { 'text-xs font-bold uppercase rounded-full border px-2 py-0.5 border-sky-400 text-sky-400' }
			.a { 'text-xs font-bold uppercase rounded-full border px-2 py-0.5 border-purple-400 text-purple-400' }
			.s { 'text-xs font-bold uppercase rounded-full border px-2 py-0.5 border-amber-400 text-amber-400' }
			.s_plus { 'text-xs font-bold uppercase rounded-full border px-2 py-0.5 border-rose-400 text-rose-400' }
			.l { 'text-xs font-bold uppercase rounded-full border px-2 py-0.5 border-orange-400 text-orange-400' }
		}
	}
	return ''
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
