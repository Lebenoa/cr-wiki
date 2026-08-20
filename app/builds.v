module app

import os
import veb
import app.util
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

// build_boost_label localizes a run boost (energy / item_time / fast_start).
pub fn (ctx &Context) build_boost_label(boost string) string {
	return veb.tr(ctx.lang, 'build_boost_${boost}')
}

// purchase_boost_label localizes a purchased pre-run boost key
// (build_purchase_boost_* keys in the .tr files).
pub fn (ctx &Context) purchase_boost_label(key string) string {
	return veb.tr(ctx.lang, 'build_purchase_boost_${key}')
}

// power_effect_label localizes an owned Power+ effect key (power_effect_*
// keys in the .tr files).
pub fn (ctx &Context) power_effect_label(key string) string {
	return veb.tr(ctx.lang, 'power_effect_${key}')
}

// boost_img_src returns the boost icon when the asset exists on disk
// (static/img/boosts/{key}.png), else a placeholder — the icons are
// placeholders until the real sprites are dropped in.
pub fn (ctx &Context) boost_img_src(key string) string {
	if os.exists('static/img/boosts/${key}.png') {
		return ctx.img_src('boosts', '${key}.png')
	}
	return ctx.img_src('boosts', none)
}

// power_effect_img_src is boost_img_src for owned Power+ effect icons
// (static/img/owned_effects/{key}.png).
pub fn (ctx &Context) power_effect_img_src(key string) string {
	if os.exists('static/img/owned_effects/${key}.png') {
		return ctx.img_src('owned_effects', '${key}.png')
	}
	return ctx.img_src('owned_effects', none)
}

// format_num inserts thousands separators for display (e.g. 710,000,000).
pub fn (ctx &Context) format_num(n u64) string {
	s := n.str()
	mut out := ''
	mut count := 0
	for i := s.len - 1; i >= 0; i-- {
		out = s[i].ascii_str() + out
		count++
		if count % 3 == 0 && i > 0 {
			out = ',' + out
		}
	}
	return out
}

// build_date_label formats a build's creation date as YYYY.MM.DD (hub style).
pub fn (ctx &Context) build_date_label(t time.Time) string {
	month := if t.month < 10 { '0' + t.month.str() } else { t.month.str() }
	day := if t.day < 10 { '0' + t.day.str() } else { t.day.str() }
	return '${t.year}.${month}.${day}'
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
			preview.combi = find_combi(wapp, ctx, c.id, p.id)
		}
	}
	return preview
}

// find_combi returns the combo bonus of a cookie+pet pair, or none when the
// pair has no combo. Shared by the planner preview and the build detail page.
fn find_combi(wapp &App, ctx &Context, cookie_id int, pet_id int) ?database.CombiBonusView {
	combis := database.get_combi_bonus(wapp.db, ctx.lang, 'cookie', cookie_id) or { return none }
	for cb in combis {
		if cb.partner_kind == 'pet' && cb.partner_id == pet_id {
			return cb
		}
	}
	return none
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
// cookie/pet/treasure/min-EP filters and score/coin/time/latest sort.
// htmx requests (filter changes, infinite scroll) get only the cards
// partial so filters live in the URL.
@['/builds']
pub fn (mut wapp App) builds(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('builds_page_title')
	ctx.set_translate_desc('builds_page_description')
	filter_cookie := (ctx.query['cookie'] or { '' }).int()
	filter_pet := (ctx.query['pet'] or { '' }).int()
	filter_treasure := (ctx.query['treasure'] or { '' }).int()
	filter_ep_raw := ctx.query['ep'] or { '' }
	filter_ep, filter_ep_special := parse_ep_tier(filter_ep_raw)
	mut sort := ctx.query['sort'] or { 'latest' }
	if sort !in ['score', 'coin', 'time', 'latest'] {
		sort = 'latest'
	}
	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	page_size := 30
	builds := database.select_builds(wapp.db, ctx.lang, filter_cookie, filter_pet, filter_treasure, filter_ep, filter_ep_special, sort, page_size, (page - 1) * page_size) or {
		[]
	}
	next_page := if builds.len == page_size { page + 1 } else { 0 }
	ep_tiers := [1, 2, 3, 4, 5, 6, 7]
	ep_specials := [1, 2, 3]

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html('./templates/components/build_cards.html')
	}

	cookies := wapp.cookie_options(ctx.lang)
	pets := wapp.pet_options(ctx.lang)
	treasures := wapp.treasure_options(ctx.lang, true)
	sel_cookie := resolve_option(cookies, filter_cookie)
	sel_pet := resolve_option(pets, filter_pet)
	sel_treasure := resolve_option(treasures, filter_treasure)
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
	ctx.noindex = true
	sel := selection_from_query(ctx)
	cookies := wapp.cookie_options(ctx.lang)
	pets := wapp.pet_options(ctx.lang)
	treasures := wapp.treasure_options(ctx.lang, true)
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
	build_boosts := ['energy', 'item_time', 'fast_start']
	purchase_boosts := util.run_boost_keys()
	power_effect_keys := util.power_effect_keys()
	return $veb.html()
}

// preview_partial re-renders just the loadout preview from query params, so
// the planner can live-update the combi/treasure cards after each pick.
@['/builds/preview']
pub fn (mut wapp App) preview_partial(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	sel := selection_from_query(ctx)
	cookies := wapp.cookie_options(ctx.lang)
	pets := wapp.pet_options(ctx.lang)
	treasures := wapp.treasure_options(ctx.lang, true)
	preview := build_preview(wapp, ctx, sel, cookies, pets, treasures)
	return $veb.html('./templates/components/build_preview.html')
}

// picker_options serves one picker dialog's option grid. The three grids are
// ~2.2MB of HTML together, which every /builds, /builds/new and build-edit
// response used to carry inline even though most visitors never open a
// dialog; picker.js now htmx-fetches the grid on the dialog's first open.
// Cached in memory per language (app/options_cache.v), so this is template
// rendering only.
@['/builds/options/:kind']
pub fn (mut wapp App) picker_options(mut ctx Context, kind string) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	match kind {
		'cookie' {
			cookies := wapp.cookie_options(ctx.lang)
			return $veb.html('./templates/components/picker_options_cookie.html')
		}
		'pet' {
			pets := wapp.pet_options(ctx.lang)
			return $veb.html('./templates/components/picker_options_pet.html')
		}
		'treasure' {
			treasures := wapp.treasure_options(ctx.lang, true)
			return $veb.html('./templates/components/picker_options_treasure.html')
		}
		else {
			return ctx.not_found()
		}
	}
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

// treasure_level_field parses one treasure slot's level form value: empty
// (field absent, e.g. pre-level forms) or non-numeric (tampered) means the
// level wasn't set and defaults to max; out-of-range values clamp to 0-9.
// Only a strict 0-9 digit string is accepted, so a bogus value can never be
// stored as level 0 — which is a legitimate level, so a naive `.int()` of
// 'abc' would silently persist the wrong value.
fn treasure_level_field(raw string) int {
	if raw == '' {
		return 9
	}
	for c in raw {
		if c < `0` || c > `9` {
			return 9
		}
	}
	l := raw.int()
	if l > 9 {
		return 9
	}
	return l
}

// BuildForm is the parsed planner fields shared by create and edit.
struct BuildForm {
	sel         BuildSelection
	blessed1    int // 1 when treasure slot 1 is blessed
	blessed2    int
	blessed3    int
	level1      int // equipped treasure level 0-9 per slot
	level2      int
	level3      int
	ep          int
	ep_special  int
	score       u64
	coin        u64
	time_ms     u64
	boxes       u64
	description string
	youtube_url string
	tags        []string
	boosts      []string // run toggles: energy/item_time/fast_start
	boost       string // purchased pre-run boost key ('' = none)
	power_effects []string // owned Power+ effect keys marked used
}

// build_form_fields parses and validates the shared planner fields: the
// loadout, EP tier, tags and run stats. Callers own author/expiry handling.
fn build_form_fields(wapp &App, ctx &Context) !BuildForm {
	sel := BuildSelection{
		cookie:    (ctx.form['cookie'] or { '' }).int()
		cookie2:   (ctx.form['c2'] or { '' }).int()
		pet:       (ctx.form['pet'] or { '' }).int()
		treasure1: (ctx.form['t1'] or { '' }).int()
		treasure2: (ctx.form['t2'] or { '' }).int()
		treasure3: (ctx.form['t3'] or { '' }).int()
	}	blessed1 := (ctx.form['blessed1'] or { '' }).int()
	blessed2 := (ctx.form['blessed2'] or { '' }).int()
	blessed3 := (ctx.form['blessed3'] or { '' }).int()
	level1 := treasure_level_field(ctx.form['t1_level'] or { '' })
	level2 := treasure_level_field(ctx.form['t2_level'] or { '' })
	level3 := treasure_level_field(ctx.form['t3_level'] or { '' })
	ep, ep_special := parse_ep_tier(ctx.form['ep'] or { '' })
	score := (ctx.form['score'] or { '' }).u64()
	coin := (ctx.form['coin'] or { '' }).u64()
	time_ms := (ctx.form['time'] or { '' }).u64() * 1000 // form is in seconds; stored as ms
	boxes := (ctx.form['boxes'] or { '' }).u64()
	description := (ctx.form['description'] or { '' }).trim_space()
	youtube_url := (ctx.form['youtube_url'] or { '' }).trim_space()
	mut tags := []string{}
	for t in ['score', 'coin', 'autofarm'] {
		if (ctx.form['tag_${t}'] or { '' }) != '' {
			tags << t
		}
	}
	mut boosts := []string{}
	for b in ['energy', 'item_time', 'fast_start'] {
		if (ctx.form['boost_${b}'] or { '' }) != '' {
			boosts << b
		}
	}
	boost := (ctx.form['boost'] or { '' }).trim_space()
	if boost != '' && boost !in util.run_boost_keys() {
		return error('Unknown boost')
	}
	mut power_effects := []string{}
	for k in util.power_effect_keys() {
		if (ctx.form['power_effect_${k}'] or { '' }) != '' {
			power_effects << k
		}
	}
	if sel.cookie <= 0 || sel.pet <= 0 || sel.treasure1 <= 0 || sel.treasure2 <= 0 || sel.treasure3 <= 0 {
		return error('A build needs one cookie, one pet and three treasures')
	}
	if (ep == 0 && ep_special == 0) || tags.len == 0 {
		return error('Pick an EP tier (EP 1-7 or Special EP 1-3) and at least one tag (#score, #coin, #autofarm)')
	}

	// Reject unknown ids so the list never links to deleted entities.
	cookies := wapp.cookie_options(ctx.lang)
	pets := wapp.pet_options(ctx.lang)
	treasures := wapp.treasure_options(ctx.lang, true)
	if resolve_option(cookies, sel.cookie) == none || resolve_option(pets, sel.pet) == none {
		return error('Unknown cookie or pet')
	}
	if sel.cookie2 > 0 && resolve_option(cookies, sel.cookie2) == none {
		return error('Unknown relay cookie')
	}
	for tid in [sel.treasure1, sel.treasure2, sel.treasure3] {
		if resolve_option(treasures, tid) == none {
			return error('Unknown treasure')
		}
	}
	return BuildForm{
		sel:         sel
		blessed1:    if blessed1 > 0 { 1 } else { 0 }
		blessed2:    if blessed2 > 0 { 1 } else { 0 }
		blessed3:    if blessed3 > 0 { 1 } else { 0 }
		level1:      level1
		level2:      level2
		level3:      level3
		ep:          ep
		ep_special:  ep_special
		score:       score
		coin:        coin
		time_ms:     time_ms
		boxes:       boxes
		description: description
		youtube_url: youtube_url
		tags:        tags
		boosts:      boosts
		boost:       boost
		power_effects: power_effects
	}
}

fn submit_build(wapp &App, mut ctx Context) veb.Result {
	if !verify_turnstile(ctx, 'build_form') {
		ctx.res.set_status(.forbidden)
		return submit_error(veb.tr(ctx.lang, 'turnstile_form_failed'), mut ctx)
	}

	form := build_form_fields(wapp, ctx) or {
		ctx.res.set_status(.bad_request)
		return submit_error(err.msg(), mut ctx)
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

	database.create_build(wapp.db, form.sel.cookie, form.sel.cookie2, form.sel.pet, form.sel.treasure1, form.sel.treasure2, form.sel.treasure3, form.blessed1, form.blessed2, form.blessed3, form.level1, form.level2, form.level3, form.ep, form.ep_special, form.tags, form.boosts, form.boost, form.power_effects, form.score, form.coin, form.time_ms, form.boxes, form.description, form.youtube_url, author, user_id, expires) or {
		ctx.res.set_status(.bad_request)
		return submit_error(err.msg(), mut ctx)
	}
	return submit_success(mut ctx, '/builds')
}

// build_info renders one build's detail page (the /builds list cards link
// here). Registered after the literal /builds/new and /builds/preview routes
// so they win the match; the typed int param keeps non-numeric ids from
// binding.
@['/builds/:id']
pub fn (mut wapp App) build_info(mut ctx Context, id int) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('build_detail_page_title')
	b := database.select_build(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	ctx.set_translate_desc('builds_page_description')
	ctx.set_og_image('cookies', b.cookie.image)
	combi := find_combi(wapp, ctx, b.cookie.id, b.pet.id)
	can_edit := can_edit_build(ctx, b)
	// verification section: the public verdict totals and the issue list,
	// plus the current session's own verdict so the template can render it
	verify_count, issue_count := database.build_review_counts(wapp.db, b.build_id)
	issues := database.build_review_issues(wapp.db, b.build_id)
	my_review := if u := ctx.user {
		database.get_build_review(wapp.db, b.build_id, u.user_id or { 0 })
	} else {
		none
	}
	// the detail page renders the full boost/Power+ catalogs with the used
	// ones marked, hub-style — not just the ones this build has
	boost_keys := ['energy', 'item_time', 'fast_start']
	power_effect_keys := util.power_effect_keys()
	return $veb.html('./templates/build_detail.html')
}

// build_verify records the current session's verdict on a build: verified
// (tried the loadout, it works) or a reported issue with a reason. One
// verdict per user per build — re-submitting overwrites the previous one.
// Authenticated users only; anonymous requests get 404, matching the
// edit/delete convention of not revealing whether the resource exists.
@['/builds/:id/verify'; post]
pub fn (wapp &App) build_verify(mut ctx Context, id int) veb.Result {
	u := ctx.user or { return ctx.not_found() }
	uid := u.user_id or { return ctx.not_found() }
	_ := database.select_build(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	if !verify_turnstile(ctx, 'build_verify') {
		ctx.res.set_status(.forbidden)
		return submit_error(veb.tr(ctx.lang, 'turnstile_form_failed'), mut ctx)
	}
	verified := (ctx.form['verified'] or { '' }) == '1'
	reason := (ctx.form['reason'] or { '' }).trim_space()
	if !verified && reason == '' {
		ctx.res.set_status(.bad_request)
		return submit_error(veb.tr(ctx.lang, 'build_issue_reason_required'), mut ctx)
	}
	// runes not bytes: the 500-char limit is about content, and Thai/emoji
	// reasons would otherwise be rejected at a third of the stated length
	if reason.runes().len > 500 {
		ctx.res.set_status(.bad_request)
		return submit_error(veb.tr(ctx.lang, 'build_issue_reason_too_long'), mut ctx)
	}
	database.upsert_build_review(wapp.db, id, uid, verified, reason) or {
		ctx.res.set_status(.internal_server_error)
		return submit_error('Unexpected Error', mut ctx)
	}
	return submit_success(mut ctx, '/builds/${id}')
}

// can_edit_build reports whether the current session may modify a build:
// its author, or an admin (site moderation).
fn can_edit_build(ctx &Context, b database.BuildCard) bool {
	if ctx.is_admin() {
		return true
	}
	u := ctx.user or { return false }
	return if uid := u.user_id {
		uid == b.user_id
	} else {
		false
	}
}

// build_edit renders the prefilled edit form (author or admin only) and
// accepts the update POST. 404 for anyone else, matching the admin-route
// convention of not revealing whether a resource exists.
@['/builds/:id/edit'; get; post]
pub fn (wapp &App) build_edit(mut ctx Context, id int) veb.Result {
	b := database.select_build(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	if !can_edit_build(ctx, b) {
		return ctx.not_found()
	}
	if ctx.req.method == .post {
		return update_build_submit(wapp, mut ctx, id)
	}
	ctx.set_translate_title('build_edit_page_title')
	ctx.noindex = true
	// no equippable treasure list here: the edit page's treasure dialog fetches
	// its grid from /builds/options/treasure. The cookie/pet lists stay — they
	// are inline <select>s on this form, not dialogs.
	cookies := wapp.cookie_options(ctx.lang)
	pets := wapp.pet_options(ctx.lang)
	ep_tiers := [1, 2, 3, 4, 5, 6, 7]
	ep_specials := [1, 2, 3]
	build_tags := ['score', 'coin', 'autofarm']
	build_boosts := ['energy', 'item_time', 'fast_start']
	purchase_boosts := util.run_boost_keys()
	power_effect_keys := util.power_effect_keys()
	t1_id := if b.treasures.len > 0 { b.treasures[0].id } else { 0 }
	t2_id := if b.treasures.len > 1 { b.treasures[1].id } else { 0 }
	t3_id := if b.treasures.len > 2 { b.treasures[2].id } else { 0 }
	t1_level := if b.treasure_levels.len > 0 { b.treasure_levels[0] } else { 9 }
	t2_level := if b.treasure_levels.len > 1 { b.treasure_levels[1] } else { 9 }
	t3_level := if b.treasure_levels.len > 2 { b.treasure_levels[2] } else { 9 }
	t1_blessed := if b.treasure_blessed.len > 0 && b.treasure_blessed[0] { 1 } else { 0 }
	t2_blessed := if b.treasure_blessed.len > 1 && b.treasure_blessed[1] { 1 } else { 0 }
	t3_blessed := if b.treasure_blessed.len > 2 && b.treasure_blessed[2] { 1 } else { 0 }
	// the slot prefills resolve from the FULL treasure list (Power+ included):
	// a stored build may legitimately reference a treasure the equippable
	// picker excludes (the catalog was rebuilt since some builds were saved),
	// and an empty slot would silently hide the stored pick. The dialog
	// itself keeps the equippable list, matching the planner.
	all_treasures := wapp.treasure_options(ctx.lang, false)
	sel_t1 := resolve_option(all_treasures, t1_id)
	sel_t2 := resolve_option(all_treasures, t2_id)
	sel_t3 := resolve_option(all_treasures, t3_id)
	return $veb.html('./templates/build_edit.html')
}

// update_build_submit persists an edit. Author/user_id/expiry are untouched.
fn update_build_submit(wapp &App, mut ctx Context, id int) veb.Result {
	if !verify_turnstile(ctx, 'build_form') {
		ctx.res.set_status(.forbidden)
		return submit_error(veb.tr(ctx.lang, 'turnstile_form_failed'), mut ctx)
	}

	form := build_form_fields(wapp, ctx) or {
		ctx.res.set_status(.bad_request)
		return submit_error(err.msg(), mut ctx)
	}
	database.update_build(wapp.db, id, form.sel.cookie, form.sel.cookie2, form.sel.pet, form.sel.treasure1, form.sel.treasure2, form.sel.treasure3, form.blessed1, form.blessed2, form.blessed3, form.level1, form.level2, form.level3, form.ep, form.ep_special, form.tags, form.boosts, form.boost, form.power_effects, form.score, form.coin, form.time_ms, form.boxes, form.description, form.youtube_url) or {
		ctx.res.set_status(.bad_request)
		return submit_error(err.msg(), mut ctx)
	}
	return submit_success(mut ctx, '/builds/${id}')
}

// build_delete removes a build (author or admin only). 404 for anyone else.
@['/builds/:id/delete'; post]
pub fn (wapp &App) build_delete(mut ctx Context, id int) veb.Result {
	b := database.select_build(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	if !can_edit_build(ctx, b) {
		return ctx.not_found()
	}
	if !verify_turnstile(ctx, 'build_delete') {
		ctx.res.set_status(.forbidden)
		return submit_error(veb.tr(ctx.lang, 'turnstile_form_failed'), mut ctx)
	}
	database.delete_build(wapp.db, id) or {
		ctx.res.set_status(.bad_request)
		return submit_error(err.msg(), mut ctx)
	}
	return submit_success(mut ctx, '/builds')
}
