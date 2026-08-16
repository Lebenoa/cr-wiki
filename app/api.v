module app

import veb
import api
import database
import os

pub fn (ctx &Context) lang_map(lang string) string {
	return veb.tr('lang_map', lang)
}

@['/api/available-langs']
pub fn (mut wapp App) available_lang(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	if !ctx.is_htmx_request() {
		return ctx.json(api.available_lang())
	}

	current_url := ctx.get_custom_header('HX-Current-URL') or { '/' }
	languages := api.available_lang()
	return $veb.html("./templates/components/lang_selector.html")
}

@['/api/richtext-names']
pub fn (mut wapp App) richtext_names(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	lang := ctx.query['lang'] or { 'en' }
	kind := ctx.query['kind'] or { 'cookie' }
	return ctx.json(database.richtext_names(wapp.db, lang, kind))
}

@['/api/set-lang'; post]
pub fn (wapp &App) set_lang(mut ctx Context) veb.Result {
	og_url := ctx.form['url'] or { '/' }
	mut lang := ctx.form['lang'] or { 'en' }
	if lang == "lang_map" {
		lang = 'en'
	}
	if os.exists('./translations/${lang}.tr') {
		ctx.set_cookie(
			name: 'wikilang'
			value: lang
			path: '/'
		)
	}

	return submit_success(mut ctx, og_url)
}
