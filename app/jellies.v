module app

import veb
import database

// jellies renders the in-run jelly catalog: every jelly with its score value
// and the cookies/pets/treasures whose skills create it, seeded from
// cookierundb.
pub fn (mut wapp App) jellies(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('jellies_page_title')
	ctx.set_translate_desc('jellies_page_description')
	jelly_views := database.select_jellies(wapp.db, ctx.lang)
	return $veb.html()
}

// jelly_info renders one jelly's detail: its score and every maker link.
@['/jellies/:id']
pub fn (mut wapp App) jelly_info(mut ctx Context, id int) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	jelly_view := database.select_jelly(wapp.db, id, ctx.lang) or {
		return ctx.not_found()
	}
	ctx.set_translate_title('jelly_detail_title', jelly_view.name)
	ctx.set_translate_desc('jelly_detail_description', jelly_view.name)
	ctx.set_og_image('jellies', jelly_view.image)
	return $veb.html('./templates/views/jelly_detail.html')
}

// score_label renders a jelly's score value without trailing zeroes
// (432.9 -> '432.9', 5000 -> '5000').
pub fn (ctx &Context) score_label(score f64) string {
	if score == f64(int(score)) {
		return '${int(score)}'
	}
	return '${score}'
}
