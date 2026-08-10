module main

import db.sqlite
import database
import database.models
import net.http
import net.urllib
import json2
import time

struct ParsedTreasure {
	page         string // wiki page name (the href key)
	title        string // display name from the infobox
	image        string
	grade        ?models.Grade
	release_date time.Time
	description  string
	is_evolved   bool
mut:
	base            string // evolved: base treasure page name from |Treasure=
	normal_effects  []string
	blessed_effects []string
}

struct ParseResp {
	parse ParseBody
}

struct ParseBody {
	text string
}

struct RevResp {
	query RevQuery
}

struct RevQuery {
	pages []RevPage
}

struct RevPage {
	title     string
	pageid    int
	missing   bool
	revisions []Rev
}

struct Rev {
	slots RevSlots
}

struct RevSlots {
	main RevMain
}

struct RevMain {
	content string
}

fn index_from(s string, sub string, start int) int {
	if start < 0 || start >= s.len {
		return -1
	}
	mut i := start
	for i <= s.len - sub.len {
		if s[i..i + sub.len] == sub {
			return i
		}
		i++
	}
	return -1
}

fn api_get(params map[string]string) !string {
	mut q := urllib.Values{}
	for k, v in params {
		q.add(k, v)
	}
	url := 'https://cookierun.wiki/mw/api.php?' + q.encode()
	resp := http.fetch(url: url, user_agent: 'Mozilla/5.0') or {
		return error('fetch failed: ${err}')
	}
	if resp.status_code != 200 {
		return error('HTTP ${resp.status_code}')
	}
	return resp.body
}

// expand a list page and return the treasure page titles (namespace + nav
// links excluded)
fn list_page_titles(page string) ![]string {
	body := api_get({
		'action':        'parse'
		'page':          page
		'prop':          'text'
		'format':        'json'
		'formatversion': '2'
	})!
	resp := json2.decode[ParseResp](body)!
	skip := [
		'Cookie_Run_Classic',
		'LINE_Cookie_Run',
		'Cookie_Run_for_Kakao',
		'List_of_Evolve_Treasures',
		'List_of_Treasures/Classic',
		'List_of_Treasures_(Classic)',
		'List_of_Treasures/LINE/S-grade',
		'Treasure_Categories',
		'Category:Treasure_categories',
		'Elixir_of_Experience',
		'Evolve_Treasures',
		'Cookies',
		'Pets',
		'Cream_Cookie',
	]
	mut titles := []string{}
	mut idx := 0
	for {
		href := index_from(resp.parse.text, 'href="/w/', idx)
		if href < 0 {
			break
		}
		q := index_from(resp.parse.text, '"', href + 9)
		if q < 0 {
			break
		}
		link := resp.parse.text[href + 9..q]
		// hrefs encode apostrophes as %27 ("Ninja_Cookie%27s_Tree_Leaf");
		// decode before turning underscores into spaces so possessive names
		// aren't silently dropped
		decoded := (urllib.query_unescape(link) or { link }).replace('_', ' ')
		if decoded !in skip && !decoded.starts_with('Category:') && !decoded.starts_with('File:') {
			if decoded !in titles {
				titles << decoded
			}
		}
		idx = q + 1
	}
	return titles
}

// extract the balanced {{...}} block whose opening marker matches `marker`
fn extract_infobox(wt string, marker string) string {
	i := wt.index(marker) or { return '' }
	mut depth := 1
	mut j := i + 2
	for j < wt.len && depth > 0 {
		if j + 1 < wt.len && wt[j] == `{` && wt[j + 1] == `{` {
			depth++
			j += 2
			continue
		}
		if j + 1 < wt.len && wt[j] == `}` && wt[j + 1] == `}` {
			depth--
			j += 2
			continue
		}
		j++
	}
	if depth != 0 {
		return ''
	}
	return wt[i..j]
}

// read a `|field = value` out of the infobox block (whitespace around `|` and
// `=` optional), stopping at the next `|fieldname =` boundary or `}}`
fn infobox_field(block string, key string) string {
	lower := block.to_lower()
	mut i := 0
	for i < lower.len {
		p := index_from(lower, '|', i)
		if p < 0 {
			return ''
		}
		mut k := p + 1
		for k < lower.len && (lower[k] == ` ` || lower[k] == `\t`) {
			k++
		}
		if k >= lower.len || !lower[k..].starts_with(key) {
			i = p + 1
			continue
		}
		mut j := k + key.len
		for j < lower.len && (lower[j] == ` ` || lower[j] == `\t`) {
			j++
		}
		if j >= lower.len || lower[j] != `=` {
			i = p + 1
			continue
		}
		val_start := j + 1
		mut end := lower.len
		mut m := val_start
		for m < lower.len {
			pipe := index_from(lower, '|', m)
			if pipe < 0 {
				break
			}
			n := pipe + 1
			mut name_start := n
			for name_start < lower.len && (lower[name_start] == ` ` || lower[name_start] == `\t`) {
				name_start++
			}
			mut name_end := name_start
			for name_end < lower.len && (lower[name_end].is_letter()
				|| lower[name_end] == ` ` || lower[name_end] == `+`
				|| lower[name_end].is_digit()) {
				name_end++
			}
			mut o := name_end
			for o < lower.len && (lower[o] == ` ` || lower[o] == `\t`) {
				o++
			}
			if o < lower.len && lower[o] == `=` {
				end = pipe
				break
			}
			m = pipe + 1
		}
		return block[val_start..end].trim_space()
	}
	return ''
}

fn strip_templates(s string) string {
	mut out := s
	for {
		p := out.index('{{') or { break }
		q := index_from(out, '}}', p)
		if q < 0 {
			out = out[..p] + out[p + 2..]
			continue
		}
		out = out[..p] + out[q + 2..]
	}
	return out
}

fn clean_markup(s string) string {
	mut out := s
	for {
		p := out.index('<!--') or { break }
		q := index_from(out, '-->', p)
		if q < 0 {
			break
		}
		out = out[..p] + out[q + 3..]
	}
	for {
		p := out.index('[[') or { break }
		q := index_from(out, ']]', p)
		if q < 0 {
			break
		}
		inner := out[p + 2..q]
		mut replacement := ''
		if !inner.to_lower().starts_with('file:') {
			replacement = if bar := inner.index('|') { inner[bar + 1..] } else { inner }
		}
		out = out[..p] + replacement + out[q + 2..]
	}
	out = strip_templates(out)
	out = out.replace('<br/>', '\n').replace('<br />', '\n').replace('<br>', '\n')
	out = out.replace("''", '')
	// strip remaining html tags (gallery/div/ref...); malformed infoboxes can
	// swallow block markup into a field value
	for {
		p := out.index('<') or { break }
		q := index_from(out, '>', p)
		if q < 0 {
			break
		}
		out = out[..p] + out[q + 1..]
	}
	// strip invisible unicode that some infobox fields carry (bidi marks,
	// non-breaking spaces) which would otherwise poison filenames
	out = out.replace('\u200e', '').replace('\u200f', '').replace('\u00a0', ' ')
	out = out.trim_space()
	for out.ends_with('}}') {
		out = out[..out.len - 2].trim_space()
	}
	return out
}

// split an effect field into individual bullet texts
fn effect_bullets(raw string) []string {
	cleaned := raw.replace('<br/>', '\n').replace('<br />', '\n').replace('<br>', '\n')
	mut bullets := []string{}
	for line in cleaned.split('\n') {
		mut l := line.trim_space()
		for l.starts_with('*') {
			l = l[1..].trim_left(' ').trim_space()
		}
		l = clean_markup(l)
		if l != '' {
			bullets << l
		}
	}
	return bullets
}

const month_names = ['january', 'february', 'march', 'april', 'may', 'june', 'july', 'august',
	'september', 'october', 'november', 'december']

// parse a wiki release date like "April 29th, 2026", "18 December 2014",
// "August 1, 2014"; the epoch (unix 0, the app's unknown-date sentinel)
// when unrecognized
fn parse_wiki_date(s string) time.Time {
	lower := s.to_lower()
	mut month := 0
	mut mp := 0
	for i, m in month_names {
		if p := lower.index(m) {
			month = i + 1
			mp = p
			break
		}
	}
	if month == 0 {
		return time.unix(0)
	}
	mut day := 0
	mut year := 0
	mut b := mp - 1
	for b >= 0 && (lower[b] == ` ` || lower[b] == `\t` || lower[b] == `,`) {
		b--
	}
	mut be := b + 1
	for b >= 0 && (lower[b].is_digit() || lower[b] == `t` || lower[b] == `h`
		|| lower[b] == `s` || lower[b] == `n` || lower[b] == `d` || lower[b] == `r`) {
		b--
	}
	day_str := lower[b + 1..be].trim('th').trim('st').trim('nd').trim('rd').trim_space()
	day = day_str.int()
	mut f := mp + month_names[month - 1].len
	for f < lower.len {
		if lower[f].is_digit() {
			mut fe := f
			for fe < lower.len && lower[fe].is_digit() {
				fe++
			}
			year = lower[f..fe].int()
			break
		}
		f++
	}
	if day == 0 || year < 1970 {
		return time.unix(0)
	}
	return time.parse('${year}-${month:02}-${day:02} 00:00:00') or { time.unix(0) }
}

// parse the infobox grade ("S", "s+", "C", ...); none when absent/unknown
fn parse_grade(s string) ?models.Grade {
	mut g := clean_markup(s).to_lower().trim_space()
	g = g.replace('+', '_plus').replace(' ', '_')
	return models.Grade.from(g) or { none }
}

// best-effort structured effect metadata: first single number -> value;
// unit from % / seconds keywords
fn effect_value_unit(text string) (f32, models.EffectUnit) {
	lower := text.to_lower()
	unit := if lower.contains('%') {
		models.EffectUnit.percent
	} else if lower.contains('sec') {
		models.EffectUnit.second
	} else {
		models.EffectUnit.flat
	}
	mut nums := []f32{}
	mut i := 0
	for i < text.len {
		if text[i].is_digit() || (text[i] == `.` && i + 1 < text.len && text[i + 1].is_digit()) {
			mut j := i
			for j < text.len && (text[j].is_digit() || text[j] == `.`) {
				j++
			}
			nums << text[i..j].f32()
			i = j
		} else {
			i++
		}
	}
	if nums.len == 1 {
		return nums[0], unit
	}
	return f32(0), unit
}

// parse_section returns the cleaned text of a `== Name ==` section
fn parse_section(wt string, name string) string {
	lower := wt.to_lower()
	needle := '== ' + name.to_lower() + ' =='
	p := lower.index(needle) or { return '' }
	start := p + needle.len
	q := index_from(lower, '\n==', start)
	body := if q < 0 { wt[start..] } else { wt[start..q] }
	return clean_markup(body).trim_space()
}

// insert_treasure_row writes one treasure row plus its translation and
// normal-state effects; returns the new id
fn insert_treasure_row(db sqlite.DB, mut effect_ids map[string]int, pt ParsedTreasure, base_id int) !int {
	tr := models.Treasure{
		image:            if pt.image == '' { none } else { pt.image }
		grade:            if g := pt.grade { int(g) } else { none }
		base_treasure_id: if base_id > 0 { base_id } else { none }
		is_evolved:       pt.is_evolved
		release_date:     pt.release_date
	}
	tid := sql db {
		insert tr into models.Treasure
	}!
	ttr := models.TreasureTranslation{
		treasure_id: tid
		lang:        'en'
		name:        pt.title
		description: pt.description
	}
	sql db {
		insert ttr into models.TreasureTranslation
	}!
	mut seen := map[int]bool{}
	for text in pt.normal_effects {
		eid := effect_for(db, mut effect_ids, text) or { panic(err) }
		// a treasure may list the same effect twice; the schema's unique
		// (treasure_id, effect_id) pair forbids duplicates
		if eid in seen {
			continue
		}
		seen[eid] = true
		val, unit := effect_value_unit(text)
		te := models.TreasureEffect{
			treasure_id: tid
			effect_id:   eid
			value:       if val == 0 { none } else { val }
			unit:        unit
		}
		sql db {
			insert te into models.TreasureEffect
		}!
	}
	return tid
}

// insert_blessed_effects links the blessed-state effect texts to the
// treasure, skipping duplicates (the schema's unique (treasure_id, effect_id)
// pair)
fn insert_blessed_effects(db sqlite.DB, mut effect_ids map[string]int, tid int, texts []string) ! {
	mut seen := map[int]bool{}
	for text in texts {
		eid := effect_for(db, mut effect_ids, text) or { panic(err) }
		if eid in seen {
			continue
		}
		seen[eid] = true
		val, unit := effect_value_unit(text)
		te := models.TreasureBlessedEffect{
			treasure_id: tid
			effect_id:   eid
			value:       if val == 0 { none } else { val }
			unit:        unit
		}
		sql db {
			insert te into models.TreasureBlessedEffect
		}!
	}
}

// effect_for returns the effect id for `text`, creating the effect +
// translation rows on first sight
fn effect_for(db sqlite.DB, mut effect_ids map[string]int, text string) !int {
	if id := effect_ids[text] {
		return id
	}
	eff := models.Effect{}
	id := sql db {
		insert eff into models.Effect
	}!
	tr := models.EffectTranslation{
		effect_id:   id
		lang:        'en'
		name:        text
		description: text
	}
	sql db {
		insert tr into models.EffectTranslation
	}!
	effect_ids[text] = id
	return id
}

fn main() {
	db := database.initialize('sqlite.db') or { panic(err) }

	// re-runnable: clear treasure data from previous runs (FK-safe order)
	for q in [
		'DELETE FROM treasure_blessed_effect',
		'DELETE FROM treasure_effect',
		'DELETE FROM effect_translation',
		'DELETE FROM effect',
		'DELETE FROM treasure_translation',
		'DELETE FROM treasure',
	] {
		rc := db.exec_none(q)
		if rc != sqlite.sqlite_ok && rc != sqlite.sqlite_done {
			panic('truncate failed (rc=${rc}): ${q}')
		}
	}

	normal := list_page_titles('List_of_Treasures_(Classic)')!
	evolved := list_page_titles('List_of_Evolve_Treasures')!
	println('normal pages: ${normal.len}, evolved pages: ${evolved.len}')

	mut all := normal.clone()
	for e in evolved {
		if e !in all {
			all << e
		}
	}

	// fetch + parse every page
	mut parsed := []ParsedTreasure{}
	mut seen_pages := map[int]bool{}
	for start := 0; start < all.len; start += 20 {
		end := if start + 20 < all.len { start + 20 } else { all.len }
		batch := all[start..end].clone()
		body := api_get({
			'action':        'query'
			'prop':          'revisions'
			'rvprop':        'content'
			'rvslots':       'main'
			'redirects':     '1' // list pages link to redirect targets; resolve them
			'titles':        batch.join('|')
			'format':        'json'
			'formatversion': '2'
		})!
		resp := json2.decode[RevResp](body)!
		for page in resp.query.pages {
			if page.missing || page.revisions.len == 0 {
				continue
			}
			if page.pageid in seen_pages {
				continue
			}
			seen_pages[page.pageid] = true
			wt := page.revisions[0].slots.main.content
			is_evo := wt.contains('{{EvoInfobox')
			if !is_evo && !wt.contains('{{TreasureInfobox') {
				println('  skip (no infobox): ${page.title}')
				continue
			}
			marker := if is_evo { '{{EvoInfobox' } else { '{{TreasureInfobox' }
			block := extract_infobox(wt, marker)
			if block == '' {
				continue
			}
			mut title := clean_markup(infobox_field(block, 'title'))
			// malformed infoboxes can pile gallery content onto the title; keep
			// only the first line
			if nl := title.index('\n') {
				title = title[..nl].trim_space()
			}
			if title == '' {
				title = page.title
			}
			// some infobox image fields carry percent-encoding ("Santa_Sock%27s_...");
			// decode so the stored name matches the actual file
			image := (urllib.query_unescape(clean_markup(infobox_field(block, 'image'))) or { '' })
			date := parse_wiki_date(infobox_field(block, 'release date'))
			desc := parse_section(wt, 'Description')
			mut pt := ParsedTreasure{
				page:         page.title
				title:        title
				image:        image
				grade:        parse_grade(infobox_field(block, 'grade'))
				release_date: date
				description:  desc
				is_evolved:   is_evo
			}
			if is_evo {
				pt.base = clean_markup(infobox_field(block, 'treasure'))
				pt.normal_effects = effect_bullets(infobox_field(block, 'normal effect'))
				pt.blessed_effects = effect_bullets(infobox_field(block, 'blessed effect'))
			} else {
				pt.normal_effects = effect_bullets(infobox_field(block, 'skill'))
			}
			parsed << pt
		}
	}
	println('parsed treasures: ${parsed.len}')

	mut effect_ids := map[string]int{}
	mut title_ids := map[string]int{}
	mut inserted := 0

	// pass 1: normals first so evolved rows can reference their base id
	for pt in parsed {
		if pt.is_evolved {
			continue
		}
		tid := insert_treasure_row(db, mut effect_ids, pt, 0)!
		title_ids[pt.title] = tid
		title_ids[pt.page] = tid
		inserted++
	}

	// some EvoInfobox |Treasure= values are redirect aliases of the base
	// ("Brave Cookie's 3rd Skull Button" -> "3rd Skull Button"); resolve those
	mut resolved_bases := map[string]int{}
	for pt in parsed {
		if !pt.is_evolved || pt.base in title_ids || pt.base in resolved_bases {
			continue
		}
		body := api_get({
			'action':        'query'
			'prop':          'revisions'
			'rvprop':        'content'
			'rvslots':       'main'
			'redirects':     '1'
			'titles':        pt.base
			'format':        'json'
			'formatversion': '2'
		}) or { continue }
		resp := json2.decode[RevResp](body) or { continue }
		for page in resp.query.pages {
			if page.missing || page.revisions.len == 0 {
				continue
			}
			if id := title_ids[page.title] {
				resolved_bases[pt.base] = id
			}
		}
	}

	// pass 2: evolved treasures — one row with its normal-state effects, and
	// the blessed-state effects in treasure_blessed_effect, both keyed to it
	for pt in parsed {
		if !pt.is_evolved {
			continue
		}
		base_id := if b := title_ids[pt.base] {
			b
		} else {
			resolved_bases[pt.base] or { 0 }
		}
		if base_id == 0 {
			println('  no base match: ${pt.title} (base ${pt.base})')
		}
		tid := insert_treasure_row(db, mut effect_ids, pt, base_id)!
		insert_blessed_effects(db, mut effect_ids, tid, pt.blessed_effects)!
		inserted++
	}
	println('treasure rows inserted: ${inserted}, distinct effects: ${effect_ids.len}')
}
