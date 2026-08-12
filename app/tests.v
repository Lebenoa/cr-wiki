module app

import os
import time
import net.http
import db.sqlite
import veb
import database
import database.models

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

fn test_effect_value_parse(mut tc TestContext) ! {
	// round-trip: parse(raw) -> format_effect_value(...) must reproduce raw
	for raw in ['12%', '2-3%', '-2%', '-2--3%', '2--3%', '-2-3%', '-5-10s', '500000', ''] {
		parts := parse_effect_value(raw) or { return error('parse ${raw}: ${err}') }
		got := database.format_effect_value(parts.value, parts.value_min, parts.value_max, parts.unit)
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

// ---------- HTTP tests ----------

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
