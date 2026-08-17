module app

import veb
import database

// episodes renders the episode/quest browser: story, special and event
// episodes with their stage/quest/relic counts, seeded from cookierundb.
pub fn (mut wapp App) episodes(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('episodes_page_title')
	ctx.set_translate_desc('episodes_page_description')
	episodes := database.select_episodes(wapp.db, ctx.lang)
	return $veb.html()
}

// episode_info renders one episode's detail: stages, relic completion set,
// quest chains, relic-draw rewards, mystery-box odds and ingredient drops.
@['/episodes/:id']
pub fn (mut wapp App) episode_info(mut ctx Context, id int) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	episode := database.select_episode(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	ctx.set_translate_title('episode_detail_title', episode.name)
	ctx.set_translate_desc('episode_detail_description', episode.name)
	ctx.set_og_image('episodes', episode.image)
	return $veb.html('./templates/views/episode.html')
}
