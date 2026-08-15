module app

import veb
import database
import time

// anon_build_ttl is how long anonymous build submissions live before they
// are dropped from the /builds list. Logged-in submissions are permanent.
const anon_build_ttl = 24 * time.hour

// expires_in_label localizes the anonymous-build expiry badge.
pub fn (ctx &Context) expires_in_label(hours int) string {
	mut h := hours
	if h < 1 {
		h = 1
	}
	return veb.tr(ctx.lang, 'build_expires_in').replace('{h}', h.str())
}

// build_ep_label localizes a build's EP tier: "EP 5" for regular tiers and
// "Special EP 2" for special ones.
pub fn (ctx &Context) build_ep_label(ep int, ep_special int) string {
	if ep_special > 0 {
		return veb.tr(ctx.lang, 'build_ep_special').replace('{n}', ep_special.str())
	}
	return veb.tr(ctx.lang, 'build_ep_tier').replace('{n}', ep.str())
}

// build_tag_label localizes a build tag with its leading '#' (e.g. #score).
pub fn (ctx &Context) build_tag_label(tag string) string {
	return '#' + veb.tr(ctx.lang, 'build_tag_${tag}')
}

// BuildSelection is the loadout picked on the build planner: one cookie, an
// optional relay cookie, one pet and up to three treasures (0 = not picked).
struct BuildSelection {
pub:
	cookie    int
	cookie2   int // relay cookie; 0 = none
	pet       int
	treasure1 int
	treasure2 int
	treasure3 int
}

// BuildPreview is the resolved loadout rendered by build_preview.html: the
// picked entities plus the combo bonus of the cookie+pet pair, if any.
pub struct BuildPreview {
pub mut:
	has_selection bool
	cookie        ?database.IdNameOption
	cookie2       ?database.IdNameOption // relay cookie; none when not picked
	pet           ?database.IdNameOption
	treasures     []database.IdNameOption
	combi         ?database.CombiBonusView
}

fn resolve_option(options []database.IdNameOption, id int) ?database.IdNameOption {
	if id <= 0 {
		return none
	}
	for o in options {
		if o.id == id {
			return o
		}
	}
	return none
}

fn build_preview(wapp &App, ctx &Context, sel BuildSelection, cookies []database.IdNameOption, pets []database.IdNameOption, treasures []database.IdNameOption) BuildPreview {
	mut preview := BuildPreview{
		cookie:  resolve_option(cookies, sel.cookie)
		cookie2: resolve_option(cookies, sel.cookie2)
		pet:     resolve_option(pets, sel.pet)
	}
	for tid in [sel.treasure1, sel.treasure2, sel.treasure3] {
		if o := resolve_option(treasures, tid) {
			preview.treasures << o
		}
	}
	preview.has_selection = preview.cookie != none || preview.cookie2 != none || preview.pet != none || preview.treasures.len > 0
	if c := preview.cookie {
		if p := preview.pet {
			combis := database.get_combi_bonus(wapp.db, ctx.lang, 'cookie', c.id) or { [] }
			for cb in combis {
				if cb.partner_kind == 'pet' && cb.partner_id == p.id {
					preview.combi = cb
					break
				}
			}
		}
	}
	return preview
}

fn selection_from_query(ctx &Context) BuildSelection {
	return BuildSelection{
		cookie:    (ctx.query['cookie'] or { '' }).int()
		cookie2:   (ctx.query['c2'] or { '' }).int()
		pet:       (ctx.query['pet'] or { '' }).int()
		treasure1: (ctx.query['t1'] or { '' }).int()
		treasure2: (ctx.query['t2'] or { '' }).int()
		treasure3: (ctx.query['t3'] or { '' }).int()
	}
}

// builds renders the community build list: paginated (30/page) with
// cookie/pet/min-EP filters and EP/new sort. htmx requests (filter changes,
// infinite scroll) get only the cards partial so filters live in the URL.
@['/builds']
pub fn (wapp &App) builds(mut ctx Context) veb.Result {
	ctx.set_translate_title('builds_page_title')
	filter_cookie := (ctx.query['cookie'] or { '' }).int()
	filter_pet := (ctx.query['pet'] or { '' }).int()
	filter_ep_raw := ctx.query['ep'] or { '' }
	filter_ep, filter_ep_special := parse_ep_tier(filter_ep_raw)
	mut sort := ctx.query['sort'] or { 'ep' }
	if sort != 'new' {
		sort = 'ep'
	}
	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	page_size := 30
	builds := database.select_builds(wapp.db, ctx.lang, filter_cookie, filter_pet, filter_ep, filter_ep_special, sort, page_size, (page - 1) * page_size) or {
		[]
	}
	next_page := if builds.len == page_size { page + 1 } else { 0 }
	ep_tiers := [1, 2, 3, 4, 5, 6, 7]
	ep_specials := [1, 2, 3]

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html('./templates/components/build_cards.html')
	}

	cookies := database.cookie_options(wapp.db, ctx.lang) or { [] }
	pets := database.pet_options(wapp.db, ctx.lang) or { [] }
	return $veb.html()
}

// new_build renders the build planner (anyone can use it) and accepts
// submissions. Anonymous submitters get an expiry time; logged-in ones are
// permanent.
@['/builds/new'; get; post]
pub fn (wapp &App) new_build(mut ctx Context) veb.Result {
	if ctx.req.method == .post {
		return submit_build(wapp, mut ctx)
	}

	ctx.set_translate_title('new_build_page_title')
	sel := selection_from_query(ctx)
	cookies := database.cookie_options(wapp.db, ctx.lang) or { [] }
	pets := database.pet_options(wapp.db, ctx.lang) or { [] }
	treasures := database.treasure_options(wapp.db, ctx.lang) or { [] }
	preview := build_preview(wapp, ctx, sel, cookies, pets, treasures)
	sel_cookie := resolve_option(cookies, sel.cookie)
	sel_cookie2 := resolve_option(cookies, sel.cookie2)
	sel_pet := resolve_option(pets, sel.pet)
	sel_t1 := resolve_option(treasures, sel.treasure1)
	sel_t2 := resolve_option(treasures, sel.treasure2)
	sel_t3 := resolve_option(treasures, sel.treasure3)
	ep_tiers := [1, 2, 3, 4, 5, 6, 7]
	ep_specials := [1, 2, 3]
	build_tags := ['score', 'coin', 'autofarm']
	return $veb.html()
}

// preview_partial re-renders just the loadout preview from query params, so
// the planner can live-update the combi/treasure cards after each pick.
@['/builds/preview']
pub fn (wapp &App) preview_partial(mut ctx Context) veb.Result {
	sel := selection_from_query(ctx)
	cookies := database.cookie_options(wapp.db, ctx.lang) or { [] }
	pets := database.pet_options(wapp.db, ctx.lang) or { [] }
	treasures := database.treasure_options(wapp.db, ctx.lang) or { [] }
	preview := build_preview(wapp, ctx, sel, cookies, pets, treasures)
	return $veb.html('./templates/components/build_preview.html')
}

// parse_ep_tier decodes the EP combobox value: '1'-'7' for regular tiers and
// 's1'-'s3' for special ones (returns ep, ep_special).
fn parse_ep_tier(raw string) (int, int) {
	if raw.len > 1 && raw[0] == `s` {
		special := raw[1..].int()
		if special >= 1 && special <= 3 {
			return 0, special
		}
		return 0, 0
	}
	ep := raw.int()
	if ep >= 1 && ep <= 7 {
		return ep, 0
	}
	return 0, 0
}

fn submit_build(wapp &App, mut ctx Context) veb.Result {
	sel := BuildSelection{
		cookie:    (ctx.form['cookie'] or { '' }).int()
		cookie2:   (ctx.form['c2'] or { '' }).int()
		pet:       (ctx.form['pet'] or { '' }).int()
		treasure1: (ctx.form['t1'] or { '' }).int()
		treasure2: (ctx.form['t2'] or { '' }).int()
		treasure3: (ctx.form['t3'] or { '' }).int()
	}
	ep, ep_special := parse_ep_tier(ctx.form['ep'] or { '' })
	score := (ctx.form['score'] or { '' }).u64()
	coin := (ctx.form['coin'] or { '' }).u64()
	time_ms := (ctx.form['time'] or { '' }).u64()
	boxes := (ctx.form['boxes'] or { '' }).u64()
	description := (ctx.form['description'] or { '' }).trim_space()
	youtube_url := (ctx.form['youtube_url'] or { '' }).trim_space()
	mut tags := []string{}
	for t in ['score', 'coin', 'autofarm'] {
		if (ctx.form['tag_${t}'] or { '' }) != '' {
			tags << t
		}
	}
	if sel.cookie <= 0 || sel.pet <= 0 || sel.treasure1 <= 0 || sel.treasure2 <= 0 || sel.treasure3 <= 0 {
		ctx.res.set_status(.bad_request)
		return ctx.text('A build needs one cookie, one pet and three treasures')
	}
	if (ep == 0 && ep_special == 0) || tags.len == 0 {
		ctx.res.set_status(.bad_request)
		return ctx.text('Pick an EP tier (EP 1-7 or Special EP 1-3) and at least one tag (#score, #coin, #autofarm)')
	}

	// Reject unknown ids so the list never links to deleted entities.
	cookies := database.cookie_options(wapp.db, ctx.lang) or { [] }
	pets := database.pet_options(wapp.db, ctx.lang) or { [] }
	treasures := database.treasure_options(wapp.db, ctx.lang) or { [] }
	if resolve_option(cookies, sel.cookie) == none || resolve_option(pets, sel.pet) == none {
		ctx.res.set_status(.bad_request)
		return ctx.text('Unknown cookie or pet')
	}
	if sel.cookie2 > 0 && resolve_option(cookies, sel.cookie2) == none {
		ctx.res.set_status(.bad_request)
		return ctx.text('Unknown relay cookie')
	}
	for tid in [sel.treasure1, sel.treasure2, sel.treasure3] {
		if resolve_option(treasures, tid) == none {
			ctx.res.set_status(.bad_request)
			return ctx.text('Unknown treasure')
		}
	}

	user := ctx.user
	mut author := (ctx.form['author'] or { '' }).trim_space()
	mut user_id := ?int(none)
	mut expires := ?time.Time(none)
	if u := user {
		author = u.username
		user_id = u.user_id
	} else {
		if author.len > 48 {
			author = author[..48]
		}
		expires = time.now().add(anon_build_ttl)
	}

	database.create_build(wapp.db, sel.cookie, sel.cookie2, sel.pet, sel.treasure1, sel.treasure2, sel.treasure3, ep, ep_special, tags, score, coin, time_ms, boxes, description, youtube_url, author, user_id, expires) or {
		ctx.res.set_status(.bad_request)
		return ctx.text(err.msg())
	}
	return ctx.redirect('/builds')
}
