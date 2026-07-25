module app

import veb
import api
import os

pub fn (ctx &Context) lang_map(lang string) string {
	return veb.tr('lang_map', lang)
}

@['/api/available-langs']
pub fn (wapp &App) available_lang(mut ctx Context) veb.Result {
	if !ctx.is_htmx_request {
		return ctx.json(api.available_lang())
	}

	current_url := ctx.get_custom_header('HX-Current-URL') or { '/' }
	languages := api.available_lang()
	return $veb.html("./templates/components/lang_selector.html")
}

@['/api/set-lang'; post]
pub fn (wapp &App) set_lang(mut ctx Context) veb.Result {
	og_url := ctx.form['url'] or { '/' }
	lang := ctx.form['lang'] or { 'en' }
	if os.exists('./translations/${lang}.tr') {
		ctx.set_cookie(
			name: 'wikilang'
			value: lang
			path: '/'
		)
	}

	return ctx.redirect(og_url)
}
