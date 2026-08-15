module app

import os
import time
import net.http
import db.sqlite
import encoding.html
import veb
import database
import database.models
import app.util

// TestContext carries the shared state of one test session: the server base
// URL and the fresh test database. The session cookie is captured by the
// login test and reused by the admin-write tests.
struct TestContext {
mut:
	base           string
	db             sqlite.DB
	session_cookie string
}

struct TestCase {
	name string
	run  fn (mut tc TestContext) ! @[required]
}

// run_test_session boots the app against a fresh throwaway database on a
// dedicated port, runs the data-integrity + HTTP suite against it, and exits
// the process with a pass/fail code. It is only reachable from main.v inside
// `$if debug`, so non-debug binaries never contain the session.
pub fn run_test_session() {
	raw_port := os.getenv('CR_TEST_PORT')
	mut port := if raw_port != '' {
		raw_port.int()
	} else {
		6798
	}
	if port < 1024 {
		port = 6798
	}
	mut db_path := os.getenv('CR_TEST_DB')
	if db_path == '' {
		db_path = 'sqlite_test.db'
	}

	println('== cookie run test session ==')
	println('db   : ${db_path} (fresh)')
	println('port : ${port}')

	if os.exists(db_path) {
		os.rm(db_path) or { panic('failed to remove old test db: ${err}') }
	}
	db := database.initialize(db_path) or { panic('failed to init test db: ${err}') }
	// Users are never seeded; create the admin used by the auth tests.
	_ := database.create_user(db, 'test', 'test') or { panic(err) }
	sql db {
		update models.User set is_admin = true where username == 'test'
	} or { panic(err) }

	base := 'http://127.0.0.1:${port}'
	mut tc := TestContext{
		base: base
		db:   db
	}

	spawn serve_test_app(db, port)
	wait_for_server(base, port)

	suite := [
		TestCase{
			name: 'every cookie unlocks a treasure'
			run:  test_every_cookie_unlocks
		},
		TestCase{
			name: 'every pet unlocks a treasure'
			run:  test_every_pet_unlocks
		},
		TestCase{
			name: 'treasure unlock links resolve'
			run:  test_unlock_links_resolve
		},
		TestCase{
			name: 'treasure effect links resolve'
			run:  test_effect_links_resolve
		},
		TestCase{
			name: 'translations complete for en and th'
			run:  test_translations_complete
		},
		TestCase{
			name: 'combi bonus links resolve'
			run:  test_combi_bonus_resolves
		},
		TestCase{
			name: 'effect value parsing'
			run:  test_effect_value_parse
		},
		TestCase{
			name: 'effect {value} placeholder substitution'
			run:  test_effect_placeholder
		},
		TestCase{
			name: 'treasure effect +0/+9 pairs'
			run:  test_effect_pairs
		},
		TestCase{
			name: 'rich text rendering'
			run:  test_rich_text
		},
		TestCase{
			name: 'rich text autocomplete names'
			run:  test_richtext_autocomplete
		},
		TestCase{
			name: 'public pages render'
			run:  test_public_pages
		},
		TestCase{
			name: 'detail pages render'
			run:  test_detail_pages
		},
		TestCase{
			name: 'thai language renders'
			run:  test_thai_language
		},
		TestCase{
			name: 'search returns results'
			run:  test_search
		},
		TestCase{
			name: 'search matches cross-language'
			run:  test_search_cross_language
		},
		TestCase{
			name: 'build picker matches cross-language'
			run:  test_build_picker_cross_language
		},
		TestCase{
			name: 'unknown route 404s'
			run:  test_not_found
		},
		TestCase{
			name: 'admin pages gated without session'
			run:  test_admin_gated
		},
		TestCase{
			name: 'admin login'
			run:  test_admin_login
		},
		TestCase{
			name: 'builds list, planner and anonymous expiry'
			run:  test_builds
		},
		TestCase{
			name: 'build detail page'
			run:  test_build_detail
		},
		TestCase{
			name: 'effect {value} placeholder submission guards'
			run:  test_placeholder_submission_guards
		},
		TestCase{
			name: 'admin creates cookie with treasure'
			run:  test_admin_create_cookie
		},
		TestCase{
			name: 'admin creates pet with new treasure'
			run:  test_admin_create_pet_new_treasure
		},
		TestCase{
			name: 'admin combi bonus edit form'
			run:  test_admin_combi_edit_form
		},
		TestCase{
			name: 'admin entity combobox matches cross-language'
			run:  test_admin_combobox_cross_language
		},
	]

	mut failures := 0
	for tcase in suite {
		mut ok := true
		tcase.run(mut tc) or {
			ok = false
			failures++
			println('  [FAIL] ${tcase.name}: ${err.msg()}')
		}
		if ok {
			println('  [ OK ] ${tcase.name}')
		}
	}
	println('== ${suite.len - failures}/${suite.len} tests passed ==')
	exit(if failures > 0 { 1 } else { 0 })
}

// serve_test_app runs the veb server on the test port inside the spawned
// thread. The db schema is already initialized/seeded by the caller.
fn serve_test_app(db sqlite.DB, port int) {
	mut wapp := initialize(db) or {
		eprintln('test session: app init failed: ${err}')
		return
	}
	veb.run_at[App, Context](mut wapp,
		family: .ip
		host:   '127.0.0.1'
		port:   port
	) or {
		eprintln('test session: server error: ${err}')
	}
}

// wait_for_server polls the base url until the server answers or a deadline
// passes; on timeout it exits with a clear failure.
fn wait_for_server(base string, port int) {
	deadline := time.now().add(15 * time.second)
	for time.now() < deadline {
		if resp := http.get('${base}/') {
			if resp.status_code == 200 {
				return
			}
		}
		time.sleep(200 * time.millisecond)
	}
	println('== test server failed to come up on port ${port} ==')
	exit(1)
}

// ---------- data-integrity tests ----------

fn test_every_cookie_unlocks(mut tc TestContext) ! {
	total := tc.db.exec('SELECT COUNT(*) AS n FROM cookie')![0].get_int('n')
	if total == 0 {
		return error('seed has no cookies')
	}
	missing := tc.db.exec('SELECT COUNT(*) AS n FROM cookie c WHERE NOT EXISTS (SELECT 1 FROM treasure t WHERE t.unlock_cookie_id = c.cookie_id)')![0].get_int('n')
	if missing > 0 {
		return error('${missing} of ${total} cookies have no unlock treasure')
	}
}

fn test_every_pet_unlocks(mut tc TestContext) ! {
	total := tc.db.exec('SELECT COUNT(*) AS n FROM pet')![0].get_int('n')
	if total == 0 {
		return error('seed has no pets')
	}
	missing := tc.db.exec('SELECT COUNT(*) AS n FROM pet p WHERE NOT EXISTS (SELECT 1 FROM treasure t WHERE t.unlock_pet_id = p.pet_id)')![0].get_int('n')
	if missing > 0 {
		return error('${missing} of ${total} pets have no unlock treasure')
	}
}

fn test_unlock_links_resolve(mut tc TestContext) ! {
	bad := tc.db.exec('SELECT COUNT(*) AS n FROM treasure WHERE (unlock_cookie_id IS NOT NULL AND unlock_cookie_id NOT IN (SELECT cookie_id FROM cookie)) OR (unlock_pet_id IS NOT NULL AND unlock_pet_id NOT IN (SELECT pet_id FROM pet))')![0].get_int('n')
	if bad > 0 {
		return error('${bad} treasure unlock links point at missing rows')
	}
	dup_cookie := tc.db.exec('SELECT unlock_cookie_id FROM treasure WHERE unlock_cookie_id IS NOT NULL GROUP BY unlock_cookie_id HAVING COUNT(*) > 1')!
	if dup_cookie.len > 0 {
		return error('${dup_cookie.len} cookies referenced by more than one treasure')
	}
	dup_pet := tc.db.exec('SELECT unlock_pet_id FROM treasure WHERE unlock_pet_id IS NOT NULL GROUP BY unlock_pet_id HAVING COUNT(*) > 1')!
	if dup_pet.len > 0 {
		return error('${dup_pet.len} pets referenced by more than one treasure')
	}
}

fn test_effect_links_resolve(mut tc TestContext) ! {
	orphan_effect := tc.db.exec('SELECT COUNT(*) AS n FROM treasure_effect te LEFT JOIN effect e ON e.effect_id = te.effect_id WHERE e.effect_id IS NULL')![0].get_int('n')
	if orphan_effect > 0 {
		return error('${orphan_effect} treasure_effect rows reference missing effects')
	}
	orphan_treasure := tc.db.exec('SELECT COUNT(*) AS n FROM treasure_effect te LEFT JOIN treasure t ON t.treasure_id = te.treasure_id WHERE t.treasure_id IS NULL')![0].get_int('n')
	if orphan_treasure > 0 {
		return error('${orphan_treasure} treasure_effect rows reference missing treasures')
	}
	// every effect must carry both language names (the dedup merge key is the
	// Thai name, so a th-less effect row is a data bug)
	no_th := tc.db.exec('SELECT COUNT(*) AS n FROM effect e WHERE NOT EXISTS (SELECT 1 FROM effect_translation et WHERE et.effect_id = e.effect_id AND et.lang = "th")')![0].get_int('n')
	if no_th > 0 {
		return error('${no_th} effects lack a th translation')
	}
}

fn test_translations_complete(mut tc TestContext) ! {
	for q in [
		'SELECT COUNT(*) AS n FROM cookie c WHERE NOT EXISTS (SELECT 1 FROM cookie_translation ct WHERE ct.owner_id = c.cookie_id AND ct.lang = "en")',
		'SELECT COUNT(*) AS n FROM cookie c WHERE NOT EXISTS (SELECT 1 FROM cookie_translation ct WHERE ct.owner_id = c.cookie_id AND ct.lang = "th")',
		'SELECT COUNT(*) AS n FROM pet p WHERE NOT EXISTS (SELECT 1 FROM pet_translation pt WHERE pt.pet_id = p.pet_id AND pt.lang = "en")',
		'SELECT COUNT(*) AS n FROM pet p WHERE NOT EXISTS (SELECT 1 FROM pet_translation pt WHERE pt.pet_id = p.pet_id AND pt.lang = "th")',
		'SELECT COUNT(*) AS n FROM treasure t WHERE NOT EXISTS (SELECT 1 FROM treasure_translation tt WHERE tt.treasure_id = t.treasure_id AND tt.lang = "en")',
		'SELECT COUNT(*) AS n FROM treasure t WHERE NOT EXISTS (SELECT 1 FROM treasure_translation tt WHERE tt.treasure_id = t.treasure_id AND tt.lang = "th")',
	] {
		if tc.db.exec(q)![0].get_int('n') > 0 {
			return error('missing translations: ${q}')
		}
	}
}

fn test_combi_bonus_resolves(mut tc TestContext) ! {
	total := tc.db.exec('SELECT COUNT(*) AS n FROM combi_bonus')![0].get_int('n')
	if total == 0 {
		return error('seed has no combi bonus rows')
	}
	bad := tc.db.exec('SELECT COUNT(*) AS n FROM combi_bonus cb WHERE cb.cookie_id NOT IN (SELECT cookie_id FROM cookie) OR cb.pet_id NOT IN (SELECT pet_id FROM pet)')![0].get_int('n')
	if bad > 0 {
		return error('${bad} combi bonus rows reference missing cookie/pet')
	}
	// effect links must resolve too (effect text is shared/translatable)
	bad_effect := tc.db.exec('SELECT COUNT(*) AS n FROM combi_bonus cb LEFT JOIN effect e ON e.effect_id = cb.effect_id WHERE cb.effect_id IS NOT NULL AND e.effect_id IS NULL')![0].get_int('n')
	if bad_effect > 0 {
		return error('${bad_effect} combi bonus rows reference missing effects')
	}
	// the fixture's combi effect must have both languages (the shared table
	// invariant the treasure side enforces too)
	no_th := tc.db.exec('SELECT COUNT(*) AS n FROM effect e WHERE e.effect_id IN (SELECT DISTINCT effect_id FROM combi_bonus WHERE effect_id IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM effect_translation et WHERE et.effect_id = e.effect_id AND et.lang = "th")')![0].get_int('n')
	if no_th > 0 {
		return error('${no_th} combi effect rows lack a th translation')
	}
}

fn test_effect_value_parse(mut _tc TestContext) ! {
	// round-trip: parse(raw) -> format_effect_value(...) must reproduce raw
	for raw in ['12%', '2-3%', '-2%', '-2--3%', '2--3%', '-2-3%', '-5-10s', '500000', ''] {
		parts := parse_effect_value(raw) or { return error('parse ${raw}: ${err}') }
		got := util.format_effect_value(parts.value, parts.value_min, parts.value_max, parts.unit)
		if got != raw {
			return error('parse ${raw}: round-trip mismatch, got ${got}')
		}
	}
	// unit detection
	pct := parse_effect_value('-2--3%')!
	if pct.unit != models.EffectUnit.percent {
		return error('parse -2--3%: expected percent unit')
	}
	sec := parse_effect_value('-5-10s')!
	if sec.unit != models.EffectUnit.second {
		return error('parse -5-10s: expected second unit')
	}
	// malformed inputs must error
	for bad in ['abc', '1-2-3', '-', '12-', '2..3', '%', '--2'] {
		if _ := parse_effect_value(bad) {
			return error('parse ${bad}: expected an error')
		}
	}
}

fn test_effect_placeholder(mut tc TestContext) ! {
	// invariant 1: a {value} placeholder on a linked effect must resolve — the
	// link has to carry the value, or the render strips the token and the
	// reader sees mangled text. Orphan translations (nothing links them) never
	// render, so they are harmless and excluded by the JOIN.
	bad := tc.db.exec('SELECT COUNT(DISTINCT et.effect_id) AS n FROM effect_translation et JOIN treasure_effect te ON te.effect_id = et.effect_id WHERE et.name LIKE "%{value}%" AND te.value IS NULL AND te.value_min IS NULL AND te.value_max IS NULL')![0].get_int('n')
	if bad > 0 {
		return error('${bad} linked effects carry a {value} placeholder with no structured value')
	}
	// invariant 2: no effect translation bakes its own structured value into
	// the name — numbers live on the link (per treasure/state), so identical
	// effects with different values dedupe. Token-boundary compare so a bare
	// "2" never matches inside "20 Energy" (a legitimate secondary number).
	mut baked := []string{}
	links := sql tc.db {
		select from models.TreasureEffect where value !is none || value_min !is none || value_max !is none
	}!
	for link in links {
		fv := util.format_effect_value(link.value, link.value_min, link.value_max, link.unit)
		if fv == '' {
			continue
		}
		translations := sql tc.db {
			select from models.EffectTranslation where effect_id == link.effect_id
		}!
		for tr in translations {
			if ' ${tr.name} '.contains(' ${fv} ') {
				baked << 'effect ${link.effect_id} "${tr.name}" carries ${fv}'
			}
		}
	}
	if baked.len > 0 {
		return error('${baked.len} effect translations bake their structured value: ${baked}')
	}
	// the placeholder renders into the +0/+9 columns with no literal token:
	// treasure 482 normal "Base speed 6%" is a flat single value
	effects := database.get_treasure_effects(tc.db, 'en', 482)!
	mut saw := false
	for e in effects {
		if e.name.contains('{value}') {
			return error('placeholder token leaked into rendered name: ${e.name}')
		}
		if e.name == 'Base speed' && e.value0 == '6%' && e.value9 == '6%' {
			saw = true
		}
	}
	if !saw {
		return error('expected "Base speed" 6%/6% on treasure 482, got ${effects}')
	}
	// th renders the unit in Thai word order
	th := database.get_treasure_effects(tc.db, 'th', 482)!
	mut saw_th := false
	for e in th {
		if e.name == 'ความเร็วพื้นฐาน' && e.value0 == '6%' && e.value9 == '6%' {
			saw_th = true
		}
	}
	if !saw_th {
		return error('expected th "ความเร็วพื้นฐาน" 6%/6% on treasure 482, got ${th}')
	}
	// bare formatter: no unit suffix — the unit lives in the translation text
	bare := util.format_effect_bare_value(6, none, none)
	if bare != '6' {
		return error('format_effect_bare_value(6) = ${bare}, want 6')
	}
	rng := util.format_effect_bare_value(none, 2, 3)
	if rng != '2-3' {
		return error('format_effect_bare_value range = ${rng}, want 2-3')
	}
}

fn test_effect_pairs(mut tc TestContext) ! {
	// treasure 482 (Young Girly Apple), normal tab:
	// - placeholder effect: flat single value repeats in both columns
	// - name-baked range: endpoints split into the +0/+9 columns
	effects := database.get_treasure_effects(tc.db, 'en', 482)!
	if effects.len != 2 {
		return error('482 effects = ${effects.len}, want 2')
	}
	if effects[0].name != 'Base speed' || effects[0].value0 != '6%' || effects[0].value9 != '6%' {
		return error('482 effect0 = ${effects[0]}, want Base speed 6%/6%')
	}
	if effects[1].name != 'extra Energy during Cookie Relays' {
		return error('482 effect1 name = ${effects[1].name}, want stripped text')
	}
	if effects[1].value0 != '30' || effects[1].value9 != '75' {
		return error('482 effect1 values = ${effects[1].value0}/${effects[1].value9}, want 30/75')
	}
	// blessed tab of the same treasure: per-state values 8% and 40-85
	blessed := database.get_treasure_blessed_effects(tc.db, 'en', 482)!
	if blessed[0].value0 != '8%' || blessed[0].value9 != '8%' {
		return error('482 blessed0 values = ${blessed[0].value0}/${blessed[0].value9}, want 8%/8%')
	}
	if blessed[1].value0 != '40' || blessed[1].value9 != '85' {
		return error('482 blessed1 values = ${blessed[1].value0}/${blessed[1].value9}, want 40/85')
	}
	// blessed tab shows the per-column delta vs the normal state: 6%->8% and
	// 30->40 / 75->85; raw values stay available on the same rows
	diffs := util.blessed_diffs(effects, blessed)
	if diffs[0].diff0 != '+2%' || diffs[0].diff9 != '+2%' {
		return error('482 blessed delta0 = ${diffs[0].diff0}/${diffs[0].diff9}, want +2%/+2%')
	}
	if diffs[0].value0 != '8%' {
		return error('482 blessed raw value0 = ${diffs[0].value0}, want 8%')
	}
	if diffs[1].diff0 != '+10' || diffs[1].diff9 != '+10' {
		return error('482 blessed delta1 = ${diffs[1].diff0}/${diffs[1].diff9}, want +10/+10')
	}
	// unchanged values delta to 0; missing normal counterpart keeps its value
	d398 := util.blessed_diffs(database.get_treasure_effects(tc.db, 'en', 398)!,
		database.get_treasure_blessed_effects(tc.db, 'en', 398)!)
	if d398[0].diff0 != '0' || d398[0].diff9 != '0' {
		return error('398 blessed delta0 = ${d398[0].diff0}/${d398[0].diff9}, want 0/0')
	}
	if d398[1].diff0 != '' || d398[1].diff9 != '' {
		return error('398 unpaired blessed = ${d398[1].diff0}/${d398[1].diff9}, want no diff')
	}
	// same value but different text: the blessed text carries a word diff
	d434 := util.blessed_diffs(database.get_treasure_effects(tc.db, 'en', 434)!,
		database.get_treasure_blessed_effects(tc.db, 'en', 434)!)
	if d434[1].diff0 != '0' || d434[1].diff9 != '0' {
		return error('434 blessed values = ${d434[1].diff0}/${d434[1].diff9}, want 0/0')
	}
	dh := d434[1].name_html.str()
	if !dh.contains('<del') || !dh.contains('>Intermediate<') || !dh.contains('<ins') || !dh.contains('>Advanced<') {
		return error('434 blessed text diff = ${dh}, want Intermediate struck + Advanced added')
	}
	// identical texts carry no diff HTML
	if d434[0].name_html.str() != '' {
		return error('434 same-text blessed = ${d434[0].name_html.str()}, want empty')
	}
	// Thai values parse the same way (suffix-aware, byte-safe)
	thd := util.blessed_diffs(database.get_treasure_effects(tc.db, 'th', 482)!,
		database.get_treasure_blessed_effects(tc.db, 'th', 482)!)
	if thd[0].diff0 != '+2%' || thd[1].diff0 != '+10' {
		return error('482 th blessed deltas = ${thd[0].diff0}/${thd[1].diff0}, want +2%/+10')
	}
	// range with a % unit keeps the symbol on both columns
	w := database.get_treasure_effects(tc.db, 'en', 368)!
	if w[0].value0 != '6%' || w[0].value9 != '11%' {
		return error('368 effect0 values = ${w[0].value0}/${w[0].value9}, want 6%/11%')
	}
	if w[0].name != 'higher base speed' {
		return error('368 effect0 name = ${w[0].name}, want "higher base speed"')
	}
	// {value} word-unit names strip the placeholder, keeping the sentence
	if w[1].name != 'Revives once with energy' {
		return error('368 effect1 name = ${w[1].name}, want "Revives once with energy"')
	}
	if w[1].value0 != '20' || w[1].value9 != '20' {
		return error('368 effect1 values = ${w[1].value0}/${w[1].value9}, want 20/20')
	}
	q := database.get_treasure_effects(tc.db, 'en', 390)!
	if q[0].value0 != '3%' || q[0].value9 != '4%' {
		return error('390 effect0 values = ${q[0].value0}/${q[0].value9}, want 3%/4%')
	}
	if q[0].name != 'Energy Drain slower' {
		return error('390 effect0 name = ${q[0].name}, want stripped text')
	}
	// dangling preposition: the value stays in the text instead of dangling
	inc := database.get_treasure_effects(tc.db, 'en', 15)!
	if inc[0].name != 'Base speed increased by 5%' {
		return error('treasure 15 name = ${inc[0].name}, want "Base speed increased by 5%"')
	}
	if inc[0].value0 != '5%' || inc[0].value9 != '5%' {
		return error('treasure 15 values = ${inc[0].value0}/${inc[0].value9}, want 5%/5%')
	}
	// Thai: byte-wise stripping keeps multi-byte text intact
	th := database.get_treasure_effects(tc.db, 'th', 482)!
	if th[0].name != 'ความเร็วพื้นฐาน' || th[0].value0 != '6%' {
		return error('482 th effect0 = ${th[0]}, want "ความเร็วพื้นฐาน" 6%')
	}
	if th[1].name != 'พลังงานพิเศษ ระหว่างรีเลย์คุกกี้' {
		return error('482 th effect1 name = ${th[1].name}, want intact Thai text')
	}
	if th[1].value0 != '30' || th[1].value9 != '75' {
		return error('482 th effect1 values = ${th[1].value0}/${th[1].value9}, want 30/75')
	}
	// no-value flavor text: both columns empty, text untouched
	one := database.get_treasure_effects(tc.db, 'en', 1)!
	if one.len != 1 {
		return error('treasure 1 effects = ${one.len}, want 1')
	}
	if one[0].name != 'get more and more performance points' {
		return error('treasure 1 name = ${one[0].name}, want upgraded prefix stripped')
	}
	if one[0].value0 != '' || one[0].value9 != '' {
		return error('treasure 1 values = ${one[0].value0}/${one[0].value9}, want empty/empty')
	}
	// binary-float noise: 0.5 - 0.3 computes as 0.19999999999999996;
	// the delta must snap to a clean +0.2, not the full f64 expansion
	// (values are name-baked: "0.3-0.8" normal -> "0.5-1.0" blessed, row 1)
	d432 := util.blessed_diffs(database.get_treasure_effects(tc.db, 'en', 432)!,
		database.get_treasure_blessed_effects(tc.db, 'en', 432)!)
	if d432[0].diff0 != '0%' || d432[0].diff9 != '0%' {
		return error('432 same-text delta = ${d432[0].diff0}/${d432[0].diff9}, want 0%/0%')
	}
	if d432[1].diff0 != '+0.2' || d432[1].diff9 != '+0.2' {
		return error('432 blessed delta = ${d432[1].diff0}/${d432[1].diff9}, want +0.2/+0.2')
	}
}

fn test_placeholder_submission_guards(mut tc TestContext) ! {
	if tc.session_cookie == '' {
		return error('run admin login first')
	}
	// a {value}-placeholder treasure effect submitted without a value would
	// render a literal "{value}" token on the page — reject at submit
	resp := http.fetch(
		method: .post
		url:    '${tc.base}/treasures/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data({
			'name':            'Guard Test Treasure'
			'effects_name_0':  'Base speed increased by {value}%'
			'effects_value_0': ''
		})
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('guard test: treasure POST: ${err}') }
	if resp.status_code != 400 {
		return error('guard test: placeholder treasure effect without value expected 400, got ${resp.status_code}')
	}
	// same name WITH a value is the normal flow — must still be accepted
	ok := http.fetch(
		method: .post
		url:    '${tc.base}/treasures/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data({
			'name':            'Guard Test Treasure'
			'effects_name_0':  'Base speed increased by {value}%'
			'effects_value_0': '7%'
		})
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('guard test: valid treasure POST: ${err}') }
	if ok.status_code != 200 {
		return error('guard test: placeholder effect WITH value expected 200, got ${ok.status_code}')
	}
	// a combo bonus has no per-link value, so a placeholder name must be
	// rejected there outright
	tid := tc.db.exec('SELECT treasure_id FROM treasure ORDER BY treasure_id LIMIT 1')![0].get_int('treasure_id')
	pid := tc.db.exec('SELECT pet_id FROM pet ORDER BY pet_id LIMIT 1')![0].get_int('pet_id')
	resp2 := http.fetch(
		method: .post
		url:    '${tc.base}/cookies/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data({
			'name':                'Guard Test Cookie'
			'abilities':           'Test abilities'
			'grade':               'c'
			'unlock_treasure_id':  '${tid}'
			'new_combi_partner_0': '${pid}'
			'new_combi_effect_0':  'Base speed increased by {value}%'
		})
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('guard test: cookie POST: ${err}') }
	if resp2.status_code != 400 {
		return error('guard test: placeholder combi effect expected 400, got ${resp2.status_code}')
	}
	// and the treasure POST must not have leaked a row through the failed guard
	n := tc.db.exec("SELECT COUNT(*) AS n FROM treasure_translation WHERE name = 'Guard Test Treasure'")![0].get_int('n')
	if n != 1 {
		return error('guard test: valid placeholder treasure created ${n} rows, want 1')
	}
}

fn test_rich_text(mut tc TestContext) ! {
	// resolve a real seed cookie id for the link tests
	row := tc.db.exec('SELECT name, owner_id FROM cookie_translation WHERE lang = "en" LIMIT 1')![0]
	name := row.get_string('name')
	cid := row.get_int('owner_id')
	// [[cookie:id]] -> link to the cookie page with the localized display name
	html1 := render_rich_text(tc.db, 'en', 'see [[cookie:${cid}]] here')
	if !html1.contains('<a href="/cookies/${cid}"') || !html1.contains('>${html.escape(name)}</a>') {
		return error('rich text: [[cookie:id]] did not become a cookie link, got: ${html1}')
	}
	// bare [[id]] defaults to a cookie link (mirroring the old [[Name]] form)
	html1d := render_rich_text(tc.db, 'en', 'see [[${cid}]] here')
	if !html1d.contains('<a href="/cookies/${cid}"') {
		return error('rich text: bare [[id]] did not become a cookie link, got: ${html1d}')
	}
	// [[pet:id]] / [[treasure:id]] -> links to the pet/treasure pages
	prow := tc.db.exec('SELECT name, pet_id FROM pet_translation WHERE lang = "en" LIMIT 1')![0]
	pname := prow.get_string('name')
	pid := prow.get_int('pet_id')
	html1b := render_rich_text(tc.db, 'en', 'see [[pet:${pid}]] here')
	if !html1b.contains('<a href="/pets/${pid}"') || !html1b.contains('>${html.escape(pname)}</a>') {
		return error('rich text: [[pet:id]] did not become a pet link, got: ${html1b}')
	}
	trow := tc.db.exec('SELECT name, treasure_id FROM treasure_translation WHERE lang = "en" LIMIT 1')![0]
	trname := trow.get_string('name')
	tid := trow.get_int('treasure_id')
	html1c := render_rich_text(tc.db, 'en', 'see [[treasure:${tid}]] here')
	// the link text is HTML-escaped (apostrophes render as &#39;)
	if !html1c.contains('<a href="/treasures/${tid}"') || !html1c.contains('>${html.escape(trname)}</a>') {
		return error('rich text: [[treasure:id]] did not become a treasure link, got: ${html1c}')
	}
	// the same [[id]] markup localizes at render time — the point of ids over
	// names: no re-linking per language (falls back to the en name if the
	// cookie has no th row)
	mut thname := name
	if throws := tc.db.exec('SELECT name FROM cookie_translation WHERE owner_id = ${cid} AND lang = "th"') {
		if throws.len > 0 {
			thname = throws[0].get_string('name')
		}
	}
	html1f := render_rich_text(tc.db, 'th', 'see [[cookie:${cid}]] here')
	if !html1f.contains('>${html.escape(thname)}</a>') {
		return error('rich text: [[cookie:id]] must render the localized name, got: ${html1f}')
	}
	// unknown kind prefix stays literal
	html1e := render_rich_text(tc.db, 'en', 'see [[gadget:${cid}]] here')
	if html1e.contains('<a href=') {
		return error('rich text: unknown kind prefix must stay literal, got: ${html1e}')
	}
	// names are no longer refs: [[Name]] and [[kind:Name]] render literally
	html1g := render_rich_text(tc.db, 'en', 'see [[${name}]] here')
	if html1g.contains('<a href=') {
		return error('rich text: name-based refs must stay literal after the id revamp, got: ${html1g}')
	}
	// unknown or non-positive ids stay literal
	html1h := render_rich_text(tc.db, 'en', '[[cookie:99999999]] [[pet:-2]] [[treasure:0]]')
	if html1h.contains('<a href=') {
		return error('rich text: unknown or non-positive ids must stay literal, got: ${html1h}')
	}
	// {color:x}text{/color} -> colored span
	html2 := render_rich_text(tc.db, 'en', 'a {color:red}red{/color} word')
	if !html2.contains('<span style="color:red">red</span>') {
		return error('rich text: color span missing, got: ${html2}')
	}
	// rgb() with spaces (standard CSS) must pass the charset check
	html2b := render_rich_text(tc.db, 'en', '{color:rgb(255, 0, 0)}x{/color}')
	if !html2b.contains('<span style="color:rgb(255, 0, 0)">x</span>') {
		return error('rich text: rgb() with spaces rejected, got: ${html2b}')
	}
	// plain text is escaped
	html3 := render_rich_text(tc.db, 'en', '<script>alert(1)</script>')
	if html3.contains('<script>') {
		return error('rich text: script tag not escaped, got: ${html3}')
	}
	// unresolvable name stays literal (no link, still escaped)
	html4 := render_rich_text(tc.db, 'en', '[[No Such Cookie Ever]]')
	if html4.contains('<a href=') {
		return error('rich text: unresolvable name became a link, got: ${html4}')
	}
	// color injection is rejected: no span is emitted, the literal text stays
	html5 := render_rich_text(tc.db, 'en', '{color:red" onclick="x}y{/color}')
	if html5.contains('<span') || !html5.contains('{/color}') {
		return error('rich text: color injection not rejected, got: ${html5}')
	}
	// missing close tag leaves the markup literal
	html6 := render_rich_text(tc.db, 'en', '{color:blue}never closed')
	if html6.contains('<span') {
		return error('rich text: unclosed color span emitted, got: ${html6}')
	}
}

// ---------- HTTP tests ----------

fn test_richtext_autocomplete(mut tc TestContext) ! {
	// the en list must contain a real cookie name
	row := tc.db.exec('SELECT name FROM cookie_translation WHERE lang = "en" LIMIT 1')![0]
	name := row.get_string('name')
	resp := http.get('${tc.base}/api/richtext-names?lang=en') or {
		return error('GET /api/richtext-names en: ${err}')
	}
	if !resp.body.contains(name) || !resp.body.contains('"id"') {
		return error('richtext autocomplete: en list missing "${name}"/id, got: ${resp.body}')
	}
	// the th list carries th names plus the en fallback (render_rich_text's
	// resolution set), so authors in either language can link
	row2 := tc.db.exec('SELECT name FROM cookie_translation WHERE lang = "th" LIMIT 1')![0]
	tname := row2.get_string('name')
	resp2 := http.get('${tc.base}/api/richtext-names?lang=th') or {
		return error('GET /api/richtext-names th: ${err}')
	}
	if !resp2.body.contains(tname) || !resp2.body.contains(name) {
		return error('richtext autocomplete: th list missing ${tname}/${name}, got: ${resp2.body}')
	}
	// unknown lang degrades to en
	resp3 := http.get('${tc.base}/api/richtext-names?lang=xx') or {
		return error('GET /api/richtext-names xx: ${err}')
	}
	if !resp3.body.contains(name) {
		return error('richtext autocomplete: unknown-lang list missing "${name}", got: ${resp3.body}')
	}
	// kind=pet / kind=treasure return their own rosters
	prow := tc.db.exec('SELECT name FROM pet_translation WHERE lang = "en" LIMIT 1')![0]
	pname := prow.get_string('name')
	resp4 := http.get('${tc.base}/api/richtext-names?lang=en&kind=pet') or {
		return error('GET /api/richtext-names pet: ${err}')
	}
	if !resp4.body.contains(pname) {
		return error('richtext autocomplete: pet list missing "${pname}", got: ${resp4.body}')
	}
	trow := tc.db.exec('SELECT name FROM treasure_translation WHERE lang = "en" LIMIT 1')![0]
	trname := trow.get_string('name')
	resp5 := http.get('${tc.base}/api/richtext-names?lang=en&kind=treasure') or {
		return error('GET /api/richtext-names treasure: ${err}')
	}
	if !resp5.body.contains(trname) {
		return error('richtext autocomplete: treasure list missing "${trname}", got: ${resp5.body}')
	}
	// unknown kind degrades to cookies
	resp6 := http.get('${tc.base}/api/richtext-names?lang=en&kind=xx') or {
		return error('GET /api/richtext-names kind=xx: ${err}')
	}
	if !resp6.body.contains(name) {
		return error('richtext autocomplete: unknown-kind list missing "${name}", got: ${resp6.body}')
	}
}

fn test_public_pages(mut tc TestContext) ! {
	for path in ['/', '/cookies', '/pets', '/treasures', '/search'] {
		resp := http.get('${tc.base}${path}') or { return error('GET ${path}: ${err}') }
		if resp.status_code != 200 {
			return error('GET ${path}: expected 200, got ${resp.status_code}')
		}
	}
}

fn test_detail_pages(mut tc TestContext) ! {
	cid := tc.db.exec('SELECT cookie_id FROM cookie ORDER BY cookie_id LIMIT 1')![0].get_int('cookie_id')
	pid := tc.db.exec('SELECT pet_id FROM pet ORDER BY pet_id LIMIT 1')![0].get_int('pet_id')
	tid := tc.db.exec('SELECT treasure_id FROM treasure ORDER BY treasure_id LIMIT 1')![0].get_int('treasure_id')
	for path in ['/cookies/${cid}', '/pets/${pid}', '/treasures/${tid}'] {
		resp := http.get('${tc.base}${path}') or { return error('GET ${path}: ${err}') }
		if resp.status_code != 200 {
			return error('GET ${path}: expected 200, got ${resp.status_code}')
		}
		if resp.body.len == 0 {
			return error('GET ${path}: empty body')
		}
	}
}

fn test_thai_language(mut tc TestContext) ! {
	resp := http.fetch(method: .get, url: '${tc.base}/cookies', cookies: {
		lang_cookie_key: 'th'
	}) or { return err }
	if resp.status_code != 200 {
		return error('th cookies page: expected 200, got ${resp.status_code}')
	}
	if !resp.body.contains('คุกกี้') {
		return error('th cookies page missing thai cookie names')
	}
}

fn test_search(mut tc TestContext) ! {
	name := tc.db.exec("SELECT name FROM cookie_translation WHERE lang = 'en' ORDER BY owner_id LIMIT 1")![0].get_string('name')
	q := name.split(' ')[0]
	resp := http.fetch(method: .get, url: '${tc.base}/search', params: {
		'q': q
	}) or { return err }
	if resp.status_code != 200 {
		return error('search?q=${q}: expected 200, got ${resp.status_code}')
	}
	if resp.body.len == 0 {
		return error('search?q=${q}: empty body')
	}
}

fn test_search_cross_language(mut tc TestContext) ! {
	// an English query on a th page must surface the th-named entity (ids link
	// to localized names; search matches across languages)
	row := tc.db.exec("SELECT ct.name AS en_name, ctt.name AS th_name, ct.owner_id AS cid FROM cookie_translation ct JOIN cookie_translation ctt ON ctt.owner_id = ct.owner_id AND ctt.lang = 'th' WHERE ct.lang = 'en' AND ct.name != ctt.name AND ct.name != '' AND ctt.name != '' LIMIT 1")![0]
	en_name := row.get_string('en_name')
	th_name := row.get_string('th_name')
	cid := row.get_int('cid')
	resp := http.fetch(method: .get, url: '${tc.base}/search', params: {
		'q': en_name
	}, cookies: {
		lang_cookie_key: 'th'
	}) or {
		return error('search cross-language: ${err}')
	}
	if resp.status_code != 200 || !resp.body.contains(th_name) {
		return error('search: en query "${en_name}" on a th page must surface th name "${th_name}", got ${resp.status_code}')
	}
	// the list-page cards carry the en name so the client-side filter matches
	// English queries on localized pages (page of the entity, id desc order)
	rank := tc.db.exec('SELECT COUNT(*) AS n FROM cookie WHERE cookie_id > ${cid}')![0].get_int('n')
	page := rank / 30 + 1
	resp2 := http.fetch(method: .get, url: '${tc.base}/cookies', params: {
		'page': page.str()
	}, cookies: {
		lang_cookie_key: 'th'
	}) or {
		return error('cookie list cross-language: ${err}')
	}
	if resp2.status_code != 200 || !resp2.body.contains('data-name-en="${html.escape(en_name)}"') {
		return error('cookie list page ${page} must carry data-name-en="${html.escape(en_name)}", got ${resp2.status_code}')
	}
}

fn test_build_picker_cross_language(mut tc TestContext) ! {
	// the /builds/new picker buttons carry the en name so the modal search
	// matches English queries on a th page
	row := tc.db.exec("SELECT name, owner_id FROM cookie_translation WHERE lang = 'en' AND name != '' LIMIT 1")![0]
	en_name := row.get_string('name')
	resp := http.fetch(method: .get, url: '${tc.base}/builds/new', cookies: {
		lang_cookie_key: 'th'
	}) or { return err }
	if resp.status_code != 200 || !resp.body.contains('data-name-en="${html.escape(en_name)}"') {
		return error('build picker must carry data-name-en="${html.escape(en_name)}", got ${resp.status_code}')
	}
}

fn test_build_detail(mut tc TestContext) ! {
	// the list cards are clickable (stretched link to the detail page)
	list := http.get('${tc.base}/builds') or { return error('GET /builds: ${err}') }
	if list.status_code != 200 || !list.body.contains('class="absolute inset-0 rounded-xl"') {
		return error('builds list cards must link to the detail page')
	}
	// create a build, then its detail page renders the full loadout
	post := http.fetch(
		method: .post
		url:    '${tc.base}/builds/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data: http.url_encode_form_data({
			'cookie':      '23'
			'pet':         '58'
			't1':          '180'
			't2':          '284'
			't3':          '378'
			'ep':          '5'
			'tag_score':   'score'
			'score':       '12345'
			'description': 'Detail page strat'
			'author':      'detailtest'
		})
		allow_redirect: false
	) or { return error('detail test submit: ${err}') }
	if post.status_code != 302 {
		return error('detail test submit: expected 302, got ${post.status_code}')
	}
	bid := tc.db.exec("SELECT build_id FROM build WHERE author = 'detailtest' ORDER BY build_id DESC LIMIT 1")![0].get_int('build_id')
	resp := http.get('${tc.base}/builds/${bid}') or { return error('GET /builds/${bid}: ${err}') }
	if resp.status_code != 200 {
		return error('GET /builds/${bid}: expected 200, got ${resp.status_code}')
	}
	for needle in ['href="/builds"', '/cookies/23', '/pets/58', '/treasures/180', 'Detail page strat', '12345'] {
		if !resp.body.contains(needle) {
			return error('build detail missing "${needle}"')
		}
	}
	// a missing build id 404s
	missing := http.get('${tc.base}/builds/99999999') or { return error('GET /builds/99999999: ${err}') }
	if missing.status_code != 404 {
		return error('GET /builds/99999999: expected 404, got ${missing.status_code}')
	}
}

fn test_admin_combobox_cross_language(mut tc TestContext) ! {
	if tc.session_cookie == '' {
		return error('run admin login first')
	}
	// the admin entity comboboxes (unlock treasure + combi partner) carry the
	// English name so the filter matches English queries on a th form
	resp := http.fetch(method: .get, url: '${tc.base}/cookies/new', cookies: {
		lang_cookie_key:    'th'
		session_cookie_key: tc.session_cookie
	}) or { return error('GET /cookies/new (th): ${err}') }
	if resp.status_code != 200 {
		return error('GET /cookies/new (th): expected 200, got ${resp.status_code}')
	}
	if !resp.body.contains('data-cb-en="') {
		return error('admin entity comboboxes must carry the en name for cross-language search')
	}
}

fn test_not_found(mut tc TestContext) ! {
	resp := http.get('${tc.base}/definitely-not-a-real-route-12345') or { return err }
	if resp.status_code != 404 {
		return error('unknown route: expected 404, got ${resp.status_code}')
	}
}

fn test_admin_gated(mut tc TestContext) ! {
	resp := http.get('${tc.base}/cookies/new') or { return err }
	if resp.status_code != 404 {
		return error('/cookies/new without session: expected 404, got ${resp.status_code}')
	}
}

fn test_admin_login(mut tc TestContext) ! {
	resp := http.fetch(
		method: .post
		url:    '${tc.base}/login'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data({
			'username': 'test'
			'password': 'test'
		})
		allow_redirect: false
	) or { return error('login POST: ${err}') }
	if resp.status_code != 302 {
		return error('login: expected 302, got ${resp.status_code}')
	}
	// the login response also sets the lang cookie; pick the session one
	mut sid := ''
	for c in resp.header.values(.set_cookie) {
		if c.all_before('=').trim_space() == session_cookie_key {
			sid = c.all_after('=').all_before(';')
			break
		}
	}
	if sid == '' {
		return error('login: no ${session_cookie_key} cookie in Set-Cookie')
	}
	tc.session_cookie = sid
}

fn test_builds(mut tc TestContext) ! {
	// the community list renders the filters
	resp := http.get('${tc.base}/builds') or { return error('GET /builds: ${err}') }
	if resp.status_code != 200 {
		return error('GET /builds: expected 200, got ${resp.status_code}')
	}
	for name in ['cookie', 'pet', 'ep', 'sort'] {
		if !resp.body.contains('name="${name}"') {
			return error('builds list missing filter "${name}"')
		}
	}
	// filter row: pet select left of cookie select, no Cookie/Pet labels
	pet_select := resp.body.index('name="pet"') or { return error('builds list missing pet select') }
	cookie_select := resp.body.index('name="cookie"') or { return error('builds list missing cookie select') }
	if pet_select >= cookie_select {
		return error('builds filter should show pet left of cookie')
	}
	if resp.body.contains('uppercase">Cookie<') || resp.body.contains('uppercase">Pet<') {
		return error('builds filter still has Cookie/Pet labels')
	}

	// the planner is open to everyone and previews the combi on load
	planner := http.get('${tc.base}/builds/new?cookie=23&pet=58') or {
		return error('GET /builds/new: ${err}')
	}
	if !planner.body.contains('<option value="" selected>') {
		return error('builds/new: EP selector should default to NONE')
	}
	if !planner.body.contains('1200 Bonus Points for all Jellies') {
		return error('build planner missing combi preview')
	}
	// picker dialogs: one per cookie/pet plus a shared treasure dialog,
	// capturing values via form method=dialog, with options listed
	for dlg in ['dialog-cookie', 'dialog-pet', 'dialog-treasure'] {
		if !planner.body.contains('id="${dlg}"') {
			return error('planner missing ${dlg}')
		}
	}
	if !planner.body.contains('form method="dialog"') {
		return error('picker should capture values via form method=dialog')
	}
	if !planner.body.contains('Wizard Cookie') || !planner.body.contains('Mini Jackson No. 2') {
		return error('picker dialogs missing options')
	}
	// a multi-effect treasure's picker option shows every effect, not just
	// the first
	multi_at := planner.body.index("data-name=\"Macaron Cookie's Blusher Brush\"") or {
		return error('planner missing multi-effect treasure option')
	}
	multi_block := planner.body[multi_at..multi_at + 1800]
	for fx in ['gives extra points for Macaron Parade Jelly', 'Revives once with energy'] {
		if !multi_block.contains(fx) {
			return error('treasure picker option missing effect "${fx}"')
		}
	}
	// a filled treasure slot shows all its effects under the name
	tslot := http.get('${tc.base}/builds/new?t1=180') or {
		return error('GET /builds/new (t1=180): ${err}')
	}
	t1_at := tslot.body.index('id="slot-t1"') or { return error('planner missing slot-t1') }
	for fx in ['Miniscule Magnetic Aura', 'Giant landing makes Jelly sprout from the ground'] {
		if !tslot.body[t1_at..t1_at + 1200].contains(fx) {
			return error('filled treasure slot missing effect "${fx}"')
		}
	}
	// an evolved treasure's option renders both states: the blessed group is
	// hidden until toggled and carries the value bump (2-4 normal, 4-6 blessed)
	evo_opt := planner.body.index('data-name="Glistening Green Leaves"') or {
		return error('planner missing evolved treasure option')
	}
	evo_block := planner.body[evo_opt..evo_opt + 2000]
	for v in ['2-4', '4-6'] {
		if !evo_block.contains(v) {
			return error('evolved treasure option missing value "${v}"')
		}
	}
	if !evo_block.contains('data-state="blessed"') {
		return error('evolved treasure option missing blessed toggle')
	}
	// a filled evolved slot shows its value alongside the effect
	evo_slot := http.get('${tc.base}/builds/new?t1=364') or {
		return error('GET /builds/new (t1=364): ${err}')
	}
	evo_at := evo_slot.body.index('id="slot-t1"') or { return error('planner missing slot-t1') }
	if !evo_slot.body[evo_at..evo_at + 1200].contains('2-4') {
		return error('filled evolved slot missing effect value')
	}
	// relay cookie slot sits next to the lead cookie on the first row
	if !planner.body.contains('id="slot-cookie2"') || !planner.body.contains('Relay Cookie') {
		return error('planner missing relay cookie slot')
	}
	// filled slots carry a corner ✕ to clear the pick back to its placeholder
	for slot in ['cookie', 'pet'] {
		if !planner.body.contains('id="slot-${slot}" data-empty=') {
			return error('slot ${slot} missing empty label for clear')
		}
		if !planner.body.contains("onclick=\"clearSlot('${slot}')\"") {
			return error('slot ${slot} missing clear button')
		}
	}
	// the planner preview shows only the combi bonus box (no cookie/pet cards)
	pv_start := planner.body.index('id="build-preview"') or { return error('planner missing build-preview') }
	pv_block := planner.body[pv_start..]
	if pv_block.contains('/pets/58"') || pv_block.contains('/cookies/23"') {
		return error('planner preview should not render cookie/pet cards')
	}
	if !planner.body.contains('Combo bonus') {
		return error('planner preview missing combo box')
	}
	// the preview partial re-renders the loadout live from query params
	preview_part := http.get('${tc.base}/builds/preview?cookie=23&pet=58') or {
		return error('GET /builds/preview: ${err}')
	}
	if !preview_part.body.contains('1200 Bonus Points for all Jellies') {
		return error('build preview partial missing combi bonus')
	}
	if preview_part.body.contains('<!DOCTYPE') || preview_part.body.contains('id="dialog-cookie"') {
		return error('preview partial should be a fragment, not a full page')
	}
	// the combo box renders whenever cookie+pet are picked: real effect when
	// the pair has a registered combo, a no-combo hint otherwise
	none_pair := http.get('${tc.base}/builds/preview?cookie=1&pet=2') or {
		return error('GET /builds/preview (no-combo pair): ${err}')
	}
	if !none_pair.body.contains('No combo bonus for this pair.') {
		return error('preview should hint when the cookie/pet pair has no combo')
	}

	// submitting with the EP selector left on NONE is rejected
	rej := http.fetch(
		method: .post
		url:    '${tc.base}/builds/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data: http.url_encode_form_data({
			'cookie': '23'
			'pet':    '58'
			't1':     '180'
			't2':     '284'
			't3':     '378'			'ep':           ''
			'tag_score':    'score'
			'author':       'noep'
		})
		allow_redirect: false
	) or { return error('no-EP submit: ${err}') }
	if rej.status_code != 400 {
		return error('no-EP submit: expected 400, got ${rej.status_code}')
	}

	// anonymous submit: build appears in the list and carries an expiry
	post := http.fetch(
		method: .post
		url:    '${tc.base}/builds/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')		data: http.url_encode_form_data({
			'cookie':  '23'
			'c2':      '95'
			'pet':     '58'
			't1':      '180'
			't2':      '284'
			't3':      '378'
			'ep':          '5'
			'tag_score':   'score'
			'tag_coin':    'coin'
			'score':       '1234567'
			'coin':        '890'
			'time':        '42000'
			'boxes':       '12'
			'description': 'Cookies and cream strat'
			'youtube_url': 'https://youtu.be/abc123'
			'author':      'anonplayer'
		})
		allow_redirect: false
	) or { return error('anon submit: ${err}') }
	if post.status_code != 302 {
		return error('anon submit: expected 302, got ${post.status_code}')
	}
	anon := tc.db.exec("SELECT author, ep, ep_special, tag, score, coin, time, boxes, description, youtube_url, expires_at, cookie2_id FROM build WHERE author = 'anonplayer'")!
	if anon.len != 1 {
		return error('anon submit: build row missing')
	}
	if anon[0].get_int('ep') != 5 || anon[0].get_int('ep_special') != 0 || anon[0].get_string('tag') != 'score,coin' {
		return error('anon submit: expected EP tier 5 with #score,#coin tags')
	}
	if anon[0].get_int('score') != 1234567 || anon[0].get_int('coin') != 890 || anon[0].get_int('time') != 42000 || anon[0].get_int('boxes') != 12 {
		return error('anon submit: run-result stats not stored')
	}
	if anon[0].get_string('description') != 'Cookies and cream strat' || anon[0].get_string('youtube_url') != 'https://youtu.be/abc123' {
		return error('anon submit: description or youtube_url not stored')
	}
	if anon[0].get_int('cookie2_id') != 95 {
		return error('anon submit: expected relay cookie 95 to be stored')
	}
	if anon[0].get_int('expires_at') <= 0 {
		return error('anon submit: expected expires_at to be set')
	}

	// logged-in submit: permanent (no expiry), author from the account
	if tc.session_cookie == '' {
		return error('run admin login first')
	}
	post2 := http.fetch(
		method: .post
		url:    '${tc.base}/builds/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		cookies: {
			session_cookie_key: tc.session_cookie
		}
		data: http.url_encode_form_data({
			'cookie': '95'
			'pet':    '99'
			't1':     '180'
			't2':     '284'
			't3':     '378'
			'ep':          's2'
			'tag_autofarm': 'autofarm'
			'author':      'ignored'
		})
		allow_redirect: false
	) or { return error('logged-in submit: ${err}') }
	if post2.status_code != 302 {
		return error('logged-in submit: expected 302, got ${post2.status_code}')
	}
	perm := tc.db.exec("SELECT author, ep, ep_special, tag, expires_at FROM build WHERE ep_special = 2 AND tag = 'autofarm'")!
	if perm.len != 1 || perm[0].get_string('author') != 'test' {
		return error('logged-in submit: author should be the account name')
	}
	if perm[0].get_int('ep') != 0 || perm[0].get_int('ep_special') != 2 || perm[0].get_string('tag') != 'autofarm' {
		return error('logged-in submit: expected Special EP 2 with #autofarm tag')
	}
	if perm[0].get_int('expires_at') != 0 {
		return error('logged-in submit: expected no expiry')
	}

	// the list shows both builds and honors the filters
	list := http.get('${tc.base}/builds') or { return error('GET /builds (2): ${err}') }
	if !list.body.contains('anonplayer') || !list.body.contains('#score') || !list.body.contains('#autofarm') {
		return error('builds list missing submitted builds or their tags')
	}
	if !list.body.contains('Cookies and cream strat') || !list.body.contains('youtu.be/abc123') {
		return error('builds list missing description or video link on card')
	}
	if !list.body.contains('1234567') || !list.body.contains('42000') || !list.body.contains('Boxes:') || !list.body.contains('>12<') {
		return error('builds list missing run-result stats on card')
	}
	// the anon card carries its relay cookie (95) alongside the lead cookie
	if !list.body.contains('href="/cookies/95"') {
		return error('builds list missing relay cookie on card')
	}
	// treasure names resolve from TreasureTranslation, not cookie names
	trs := tc.db.exec("SELECT name FROM treasure_translation WHERE treasure_id = 180 AND lang = 'en'")!
	if trs.len != 1 || !list.body.contains(trs[0].get_string('name')) {
		return error('builds list missing treasure name on card')
	}
	filtered := http.get('${tc.base}/builds?cookie=23&pet=58&ep=5') or {
		return error('GET /builds?filter: ${err}')
	}
	if !filtered.body.contains('anonplayer') {
		return error('cookie/pet/EP filter hides a matching build')
	}
	special := http.get('${tc.base}/builds?ep=s2') or { return error('GET /builds?ep=s2: ${err}') }
	if !special.body.contains('#autofarm') || special.body.contains('#score') {
		return error('special EP filter shows the wrong builds')
	}
	no_match := http.get('${tc.base}/builds?ep=7') or { return error('GET /builds?ep=7: ${err}') }
	if no_match.body.contains('anonplayer') || no_match.body.contains('#autofarm') {
		return error('EP filter shows builds outside the tier')
	}

	// expired anonymous builds drop out of the list
	tc.db.exec("INSERT INTO build (cookie_id, pet_id, treasure1_id, treasure2_id, treasure3_id, ep, author, user_id, created_at, expires_at) VALUES (23, 58, 180, 284, 378, 999, 'zzexpired', NULL, strftime('%s','now'), 1)")!
	exp := http.get('${tc.base}/builds') or { return error('GET /builds (3): ${err}') }
	if exp.body.contains('zzexpired') {
		return error('expired anonymous build still listed')
	}
}

fn test_admin_create_cookie(mut tc TestContext) ! {
	if tc.session_cookie == '' {
		return error('run admin login first')
	}
	tid := tc.db.exec('SELECT treasure_id FROM treasure ORDER BY treasure_id LIMIT 1')![0].get_int('treasure_id')
	resp := http.fetch(
		method: .post
		url:    '${tc.base}/cookies/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data({
			'name':               'Test Cookie'
			'abilities':          'Test abilities'
			'grade':              'c'
			'unlock_treasure_id': '${tid}'
		})
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('create cookie POST: ${err}') }
	if resp.status_code != 200 {
		return error('create cookie: expected 200 after redirect, got ${resp.status_code}')
	}
	rows := tc.db.exec("SELECT owner_id FROM cookie_translation WHERE name = 'Test Cookie' AND lang = 'en'")!
	if rows.len == 0 {
		return error('create cookie: row not found')
	}
	cid := rows[0].get_int('owner_id')
	linked := tc.db.exec('SELECT COUNT(*) AS n FROM treasure WHERE unlock_cookie_id = ${cid}')![0].get_int('n')
	if linked == 0 {
		return error('create cookie: treasure not linked to new cookie')
	}
}

fn test_admin_create_pet_new_treasure(mut tc TestContext) ! {
	if tc.session_cookie == '' {
		return error('run admin login first')
	}
	resp := http.fetch(
		method: .post
		url:    '${tc.base}/pets/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data({
			'name':               'Test Pet'
			'abilities':          'Test abilities'
			'grade':              'c'
			'release_date':       '2024-01-15'
			'unlock_treasure_id': '__new__'
			'new_treasure_name':  'Test New Treasure'
		})
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('create pet POST: ${err}') }
	if resp.status_code != 200 {
		return error('create pet: expected 200 after redirect, got ${resp.status_code}')
	}
	rows := tc.db.exec("SELECT pet_id FROM pet_translation WHERE name = 'Test Pet' AND lang = 'en'")!
	if rows.len == 0 {
		return error('create pet: row not found')
	}
	pid := rows[0].get_int('pet_id')
	tr := tc.db.exec("SELECT treasure_id FROM treasure_translation WHERE name = 'Test New Treasure' AND lang = 'en'")!
	if tr.len == 0 {
		return error('create pet: new treasure not created')
	}
	tid := tr[0].get_int('treasure_id')
	linked := tc.db.exec('SELECT COUNT(*) AS n FROM treasure WHERE treasure_id = ${tid} AND unlock_pet_id = ${pid}')![0].get_int('n')
	if linked == 0 {
		return error('create pet: new treasure not linked to pet')
	}
	// a treasure created from the combobox inherits the pet's release date
	expected := tc.db.exec("SELECT CAST(strftime('%s', '2024-01-15 00:00:00') AS INT) AS ts")![0].get_int('ts')
	rel := tc.db.exec('SELECT release_date FROM treasure WHERE treasure_id = ${tid}')![0].get_int('release_date')
	if rel != expected {
		return error('create pet: new treasure release_date ${rel} != pet release_date ${expected}')
	}
}

fn test_admin_combi_edit_form(mut tc TestContext) ! {
	if tc.session_cookie == '' {
		return error('run admin login first')
	}
	tid := tc.db.exec('SELECT treasure_id FROM treasure ORDER BY treasure_id LIMIT 1')![0].get_int('treasure_id')
	pid := tc.db.exec('SELECT pet_id FROM pet ORDER BY pet_id LIMIT 1')![0].get_int('pet_id')

	// create a throwaway cookie to attach combos to
	resp := http.fetch(
		method: .post
		url:    '${tc.base}/cookies/new'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data({
			'name':               'Combi Test Cookie'
			'abilities':          'Test abilities'
			'grade':              'c'
			'unlock_treasure_id': '${tid}'
		})
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('combi test: create cookie POST: ${err}') }
	if resp.status_code != 200 {
		return error('combi test: create cookie expected 200, got ${resp.status_code}')
	}
	cid := tc.db.exec("SELECT owner_id FROM cookie_translation WHERE name = 'Combi Test Cookie' AND lang = 'en'")![0].get_int('owner_id')

	pid2 := tc.db.exec('SELECT pet_id FROM pet ORDER BY pet_id DESC LIMIT 1')![0].get_int('pet_id')

	// add combos via the edit form; indices 0 and 2 are deliberately sent with
	// a gap (index 1 missing) to prove the parser tolerates holes left by a
	// removed row instead of silently dropping the later ones
	mut form := {
		'name':                'Combi Test Cookie'
		'abilities':           'Test abilities'
		'grade':               'c'
		'unlock_treasure_id':  '${tid}'
		'new_combi_partner_0': '${pid}'
		'new_combi_effect_0':  'Combo test A'
		'new_combi_hidden_0':  'true'
		'new_combi_partner_2': '${pid2}'
		'new_combi_effect_2':  'Combo test B'
	}
	http.fetch(
		method: .post
		url:    '${tc.base}/cookies/${cid}/edit'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data(form)
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('combi test: add POST: ${err}') }
	rows := tc.db.exec('SELECT cb.id, cb.is_hidden, e.effect_id FROM combi_bonus cb JOIN effect_translation e ON e.effect_id = cb.effect_id WHERE cb.cookie_id = ${cid} AND cb.pet_id = ${pid} AND e.lang = "en" AND e.name = "Combo test A"')!
	if rows.len == 0 {
		return error('combi test: added row not found')
	}
	row_id := rows[0].get_int('id')
	if rows[0].get_int('is_hidden') != 1 {
		return error('combi test: hidden flag not stored')
	}
	// the gapped index 2 must have landed too
	second := tc.db.exec('SELECT cb.id FROM combi_bonus cb JOIN effect_translation e ON e.effect_id = cb.effect_id WHERE cb.cookie_id = ${cid} AND cb.pet_id = ${pid2} AND e.lang = "en" AND e.name = "Combo test B"')!
	if second.len == 0 {
		return error('combi test: row after index gap not created')
	}
	second_id := second[0].get_int('id')

	// update the first row: new effect text, unhidden; keep the second too
	form = {
		'name':               'Combi Test Cookie'
		'abilities':          'Test abilities'
		'grade':              'c'
		'unlock_treasure_id': '${tid}'
		'combi_id_${row_id}': '${row_id}'
		'combi_effect_${row_id}': 'Combo test C'
		'combi_id_${second_id}': '${second_id}'
	}
	http.fetch(
		method: .post
		url:    '${tc.base}/cookies/${cid}/edit'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data(form)
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('combi test: update POST: ${err}') }
	updated := tc.db.exec('SELECT cb.is_hidden, e.name FROM combi_bonus cb JOIN effect_translation e ON e.effect_id = cb.effect_id WHERE cb.id = ${row_id} AND e.lang = "en"')!
	if updated.len == 0 || updated[0].get_string('name') != 'Combo test C' {
		return error('combi test: effect text not updated')
	}
	if updated[0].get_int('is_hidden') != 0 {
		return error('combi test: hidden flag not cleared')
	}

	// remove: markers absent, so the diff deletes both rows
	form = {
		'name':               'Combi Test Cookie'
		'abilities':          'Test abilities'
		'grade':              'c'
		'unlock_treasure_id': '${tid}'
	}
	http.fetch(
		method: .post
		url:    '${tc.base}/cookies/${cid}/edit'
		header: http.new_header(key: .content_type, value: 'application/x-www-form-urlencoded')
		data:   http.url_encode_form_data(form)
		cookies: {
			session_cookie_key: tc.session_cookie
		}
	) or { return error('combi test: remove POST: ${err}') }
	gone := tc.db.exec('SELECT COUNT(*) AS n FROM combi_bonus WHERE id = ${row_id}')![0].get_int('n')
	if gone != 0 {
		return error('combi test: removed row still present')
	}
	gone2 := tc.db.exec('SELECT COUNT(*) AS n FROM combi_bonus WHERE id = ${second_id}')![0].get_int('n')
	if gone2 != 0 {
		return error('combi test: second removed row still present')
	}
}
