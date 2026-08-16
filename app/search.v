module app

import veb
import net.urllib
import database

@['/search']
pub fn (wapp &App) search(mut ctx Context) veb.Result {
	if !rate_limit_ok(mut ctx) {
		return ctx.text('too many requests')
	}
	ctx.set_translate_title("search_page_title")
	ctx.noindex = true
	q := (ctx.query['q'] or { '' }).trim_space()
	is_fragment := ctx.is_htmx_request() && !ctx.is_boosted_request()
	limit := if is_fragment { 8 } else { 30 }
	mut results := database.SearchResults{}
	if q != '' {
		results = database.search_all(wapp.db, ctx.lang, q, limit) or {
			println(err)
			return ctx.html("Something went wrong")
		}
	}
	// flattened so the card partials (which expect `cookies`/`pets`) and the
	// templates can iterate them; next_page keeps the partials' infinite-scroll
	// sentinel quiet (search results are not paginated). tab satisfies the
	// treasure_cards sentinel's URL, which search never renders.
	cookies := results.cookies
	pets := results.pets
	treasures := results.treasures
	next_page := 0
	tab := 'all'
	q_url := urllib.query_escape(q)
	if is_fragment {
		if q == '' {
			return ctx.text('')
		}
		return $veb.html("./templates/components/search_results.html")
	}
	return $veb.html("./templates/search.html")
}
