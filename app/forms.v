module app

import time
import api
import veb
import database
import database.models

// parse_release_date reads the 'release_date' form field as YYYY-MM-DD,
// defaulting to now when the field is empty.
fn parse_release_date(mut ctx Context) !time.Time {
	date_str := ctx.form['release_date']
	if date_str == '' {
		return time.now()
	}
	return time.parse('${date_str} 00:00:00') or {
		return error('Invalid release date, expected YYYY-MM-DD')
	}
}

// CookieForm carries the form state shared by the cookie create/edit pages.
// Fields are pub mut — the failed-submit path re-renders the form and
// overlays the submitted values onto the state.
pub struct CookieForm {
pub mut:
	edit_mode         bool
	id                int
	name              string
	abilities         string
	description       string
	power_plus        string
	power_plus_requirement string
	unlock_goal       string
	grade             string
	release_date      string
	lang              string
	image             ?string
	unlock_treasure_id int // 0 = none; the treasure this cookie unlocks at max level
	treasures         []database.IdNameOption
	combis            []database.CombiEditRow // this cookie's combo bonuses (edit mode)
	partners          []database.IdNameOption // pickable pets for a new combo
	partner_kind      string                  // 'pet' — sprite dir for partner images
	effect_names      []string                // shared effect-name suggestions
	form_error        string                  // validation error shown in the error banner (failed submit)
}

// PetForm carries the form state shared by the pet create/edit pages.
// Fields are pub mut — the failed-submit path re-renders the form and
// overlays the submitted values onto the state.
pub struct PetForm {
pub mut:
	edit_mode         bool
	id                int
	name              string
	abilities         string
	description       string
	grade             string
	release_date      string
	lang              string
	image             ?string
	unlock_treasure_id int // 0 = none; the treasure this pet unlocks at max level
	treasures         []database.IdNameOption
	combis            []database.CombiEditRow // this pet's combo bonuses (edit mode)
	partners          []database.IdNameOption // pickable cookies for a new combo
	partner_kind      string                  // 'cookie' — sprite dir for partner images
	effect_names      []string                // shared effect-name suggestions
	form_error        string                  // validation error shown in the error banner (failed submit)
}

// EffectRow carries one effect row in the treasure form's structured editor.
pub struct EffectRow {
pub:
	name  string
	value string // display text: "12%", "2-3%", "3s", "500000", '' = none
}


// effect_rows_from_db maps the treasure's stored effect rows to form rows,
// using the stored value string directly for the editor.
fn effect_rows_from_db(rows []database.EffectRowData) []EffectRow {
	mut out := []EffectRow{}
	for r in rows {
		out << EffectRow{
			name:  r.name
			value: r.value
		}
	}
	return out
}

// parse_effect_value validates an effect's value text and returns it
// normalized (trimmed): a single value ("12%", "3s", "500000", "-2"), a
// min/max range ("2-3%", "-2--3%", "0.3-0.8s"), or "" for no value. The
// unit suffix (%, s) stays part of the string. Anything else is rejected
// here at submit time.
fn parse_effect_value(raw string) !string {
	mut s := raw.trim_space()
	if s == '' {
		return ''
	}
	mut suffix := ''
	if s.ends_with('%') {
		suffix = '%'
		s = s[..s.len - 1]
	} else if s.ends_with('s') {
		suffix = 's'
		s = s[..s.len - 1]
	}
	if s == '' {
		return error('expected a number or range like 12% or 2-3%')
	}
	// scan signed numbers: '12' | '0.3-0.8' | '-2' | '-2--3' (one or two
	// values, decimals allowed). A single '-' after a number is the range
	// separator; the next number consumes its own optional sign, so '-2--3'
	// means min -2, max -3.
	mut nums := []string{}
	mut i := 0
	for i < s.len {
		// one number: optional sign then digits and a single decimal point
		mut j := i
		if s[j] == `-` {
			j++
		}
		if j >= s.len || !s[j].is_digit() {
			return error('expected a number or range like 12% or 2-3%')
		}
		mut dots := 0
		for j < s.len && (s[j].is_digit() || s[j] == `.`) {
			if s[j] == `.` {
				dots++
			}
			if dots > 1 {
				return error('expected a number or range like 12% or 2-3%')
			}
			j++
		}
		nums << s[i..j]
		i = j
		// separator: exactly one '-' before the next number
		if i < s.len {
			if s[i] != `-` {
				return error('expected a number or range like 12% or 2-3%')
			}
			i++
			if i >= s.len {
				return error('expected a number or range like 12% or 2-3%')
			}
		}
	}
	// any count of values is legal (some effects carry several, e.g. "1-2-3%");
	// the stored string keeps them all, and the +0/+9 columns render the
	// first/last endpoints
	return nums.join('-') + suffix
}

// TreasureForm carries the form state shared by the treasure create/edit pages.
// Fields are pub mut — the failed-submit path re-renders the form and
// overlays the submitted values onto the state.
pub struct TreasureForm {
pub mut:
	edit_mode       bool
	id              int
	name            string
	description     string
	grade           string // '' = no wiki grade
	effects         []EffectRow
	blessed_effects []EffectRow
	effect_names []string  // suggestions for the effect name input
	empty_effect EffectRow // blank row for the clone template loop
	is_evolved      bool
	is_power_plus   bool // POWER+ treasure (friendly-run bonus) cannot be equipped
	base_treasure_id int // 0 = none; the normal treasure this evolved row evolves from
	bases           []database.IdNameOption // pickable normal treasures for the base link
	release_date    string
	lang            string
	image           ?string
	unlock_cookie_id int // 0 = none; cookie whose max-level upgrade unlocks this treasure
	unlock_pet_id    int // 0 = none; pet whose max-level upgrade unlocks this treasure
	cookies         []database.IdNameOption
	pets            []database.IdNameOption
	form_error      string // validation error shown in the error banner (failed submit)
}

// parse_combi_editor reads the combi-bonus section of the cookie/pet form:
// existing rows are identified by a `combi_id_<id>` marker (their effect text
// + hidden flag come from `combi_effect_<id>` / `combi_hidden_<id>`); new rows
// come from the renumbered `new_combi_partner_<i>` / `new_combi_effect_<i>` /
// `new_combi_hidden_<i>` inputs. Rows whose marker is missing are deleted
// server-side (the form's remove button just drops the row from the DOM).
fn parse_combi_editor(mut ctx Context) !(map[int]database.CombiRowUpdate, []database.CombiNewRow) {
	mut keep := map[int]database.CombiRowUpdate{}
	for k, _ in ctx.form {
		if !k.starts_with('combi_id_') {
			continue
		}
		id := k.all_after('combi_id_').int()
		if id <= 0 {
			continue
		}
		keep[id] = database.CombiRowUpdate{
			effect:    ctx.form['combi_effect_${id}'] or { '' }
			is_hidden: (ctx.form['combi_hidden_${id}'] or { '' }) == 'true'
		}
	}
	// new rows: collect by index from the form (gap-tolerant — a removed row
	// can leave a hole if the editor's JS didn't renumber), then emit in index
	// order; rows without a picked partner are skipped.
	mut new_by_index := map[int]database.CombiNewRow{}
	for k, v in ctx.form {
		if !k.starts_with('new_combi_partner_') {
			continue
		}
		idx := k.all_after('new_combi_partner_').int()
		if idx < 0 {
			continue
		}
		new_by_index[idx] = database.CombiNewRow{
			partner_id: v.int()
			effect:     ctx.form['new_combi_effect_${idx}'] or { '' }
			is_hidden:  (ctx.form['new_combi_hidden_${idx}'] or { '' }) == 'true'
		}
	}
	mut new_rows := []database.CombiNewRow{}
	mut indices := new_by_index.keys()
	indices.sort()
	for idx in indices {
		nr := new_by_index[idx]
		if nr.partner_id <= 0 {
			continue
		}
		new_rows << nr
	}
	return keep, new_rows
}

// parse_optional_id reads an optional entity-id form field: empty/absent -> none.
fn parse_optional_id(mut ctx Context, field string) ?int {
	raw := ctx.form[field] or { return none }
	if raw == '' {
		return none
	}
	return raw.int()
}

fn parse_cookie_form(mut ctx Context) !database.CreateCookieParams {
	image := upload_image(mut ctx, 'cookies')!
	grade := models.Grade.from(ctx.form['grade']) or {
		return error('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
	}
	choice := parse_unlock_treasure(mut ctx)!
	combi_keep, combi_new := parse_combi_editor(mut ctx)!
	return database.CreateCookieParams{
		lang:               ctx.form['lang'] or { ctx.lang }
		name:               ctx.form['name']
		abilities:          ctx.form['abilities']
		description:        ctx.form['description']
		grade:              grade
		image:              if image == '' {
			none
		} else {
			image
		}
		power_plus:              ctx.form['power_plus']
		power_plus_requirement:  ctx.form['power_plus_requirement']
		unlock_goal:             ctx.form['unlock_goal']
		release_date:            parse_release_date(mut ctx)!
		unlock_treasure_id:      choice.treasure_id
		new_treasure_name:       choice.new_name
		combi_keep:              combi_keep
		combi_new:               combi_new
	}
}

fn parse_pet_form(mut ctx Context) !database.CreatePetParams {
	image := upload_image(mut ctx, 'pets')!
	grade := models.Grade.from(ctx.form['grade']) or {
		return error('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
	}
	choice := parse_unlock_treasure(mut ctx)!
	combi_keep, combi_new := parse_combi_editor(mut ctx)!
	return database.CreatePetParams{
		lang:               ctx.form['lang'] or { ctx.lang }
		name:               ctx.form['name']
		abilities:          ctx.form['abilities']
		description:        ctx.form['description']
		grade:              grade
		image:              if image == '' {
			none
		} else {
			image
		}
		release_date:       parse_release_date(mut ctx)!
		unlock_treasure_id: choice.treasure_id
		new_treasure_name:  choice.new_name
		combi_keep:         combi_keep
		combi_new:          combi_new
	}
}

// parse_effect_inputs reads the treasure form's structured effect rows for
// one state (indexed fields `${prefix}_name_N` and `${prefix}_value_N`) into
// EffectInputs in submitted order, validating each value at submit. Rows
// with a blank name and value are skipped (the trailing blank "add row"); a
// value without a name is an error.
fn parse_effect_inputs(mut ctx Context, prefix string) ![]database.EffectInput {
	mut effects := []database.EffectInput{}
	mut i := 0
	for {
		mut name := ctx.form['${prefix}_name_${i}'] or { break }
		name = name.trim_space()
		value_str := ctx.form['${prefix}_value_${i}'] or { '' }
		if name == '' {
			if value_str.trim_space() != '' {
				return error('Effect ${i + 1}: name is required')
			}
			i++
			continue
		}
		parts := parse_effect_value(value_str) or {
			return error('Effect ${i + 1}: ${err.msg()}')
		}
		// {value}-placeholder names must carry a value on the link — the page
		// substitutes it into the text, so a placeholder without a value would
		// render as a literal "{value}" token
		if name.contains('{value}') && parts == '' {
			return error('Effect ${i + 1}: name uses a {value} placeholder but no value was entered')
		}
		effects << database.EffectInput{
			name:  name
			value: parts
		}
		i++
	}
	return effects
}

// UnlockTreasureChoice is the parsed treasure combobox value from the
// cookie/pet forms: an existing treasure id, or a name for a new treasure.
pub struct UnlockTreasureChoice {
pub:
	treasure_id ?int
	new_name    string
}

// parse_unlock_treasure reads the cookie/pet form's treasure combobox: an
// existing treasure id, '__new__' (whose name must be non-empty and creates
// the treasure at submit), or no selection.
fn parse_unlock_treasure(mut ctx Context) !UnlockTreasureChoice {
	raw := ctx.form['unlock_treasure_id'] or { '' }
	if raw == '__new__' {
		name := (ctx.form['new_treasure_name'] or { '' }).trim_space()
		if name == '' {
			return error('New treasure name is required')
		}
		return UnlockTreasureChoice{
			new_name: name
		}
	}
	if raw == '' {
		return error('A treasure is required: every cookie/pet gives one at max level')
	}
	return UnlockTreasureChoice{
		treasure_id: raw.int()
	}
}

fn parse_treasure_form(mut ctx Context) !database.CreateTreasureParams {
	image := upload_image(mut ctx, 'treasures')!
	// grade is optional: empty means no wiki grade (no badge on the detail page)
	g := ctx.form['grade'] or { '' }
	mut grade := ?int(none)
	if g != '' {
		grade = int(models.Grade.from(g) or {
			return error('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
		})
	}
	return database.CreateTreasureParams{
		lang:             ctx.form['lang'] or { ctx.lang }
		name:             ctx.form['name']
		description:      ctx.form['description']
		image:            if image == '' {
			none
		} else {
			image
		}
		grade:            grade
		is_evolved:       ctx.form['is_evolved'] == 'true'
		is_power_plus:    ctx.form['is_power_plus'] == 'true'
		base_treasure_id: parse_optional_id(mut ctx, 'base_treasure_id')
		release_date:     parse_release_date(mut ctx)!
		effects:          parse_effect_inputs(mut ctx, 'effects')!
		blessed_effects:  parse_effect_inputs(mut ctx, 'blessed_effects')!
		unlock_cookie_id: parse_optional_id(mut ctx, 'unlock_cookie_id')
		unlock_pet_id:    parse_optional_id(mut ctx, 'unlock_pet_id')
	}
}

// submit_success finishes a successful form POST. htmx requests get an
// HX-Redirect header so the browser navigates to the destination (a fragment
// swap would render it inside the form's target with the URL stuck on the
// form path); plain requests keep the normal 302 redirect.
fn submit_success(mut ctx Context, redirect_url string) veb.Result {
	if ctx.is_htmx_request() {
		ctx.set_custom_header('HX-Redirect', redirect_url) or {}
		return ctx.text('')
	}
	return ctx.redirect(redirect_url)
}

// submit_error returns a failed form submission for forms without a
// re-renderable state (login/register/builds): htmx requests get the OOB
// error banner fragment (the form stays in place, input preserved); plain
// requests keep the bare error text. The caller sets the status first.
// NOTE: `form_error` comes FIRST because this V fork's `$veb.html`
// codegen assumes the Context is the second parameter (params[1]); with
// `mut ctx` first it would emit `veb__Context_html(form_error, ...)`.
fn submit_error(form_error string, mut ctx Context) veb.Result {
	if ctx.is_htmx_request() {
		err_msg := form_error
		return $veb.html('./templates/components/form_error.html')
	}
	return ctx.text(form_error)
}

// cookie_form_state builds the CookieForm for the create page (edit = none)
// or the edit page. Shared by the GET render and the failed-POST re-render.
fn cookie_form_state(wapp &App, ctx &Context, edit ?database.CookieView) CookieForm {
	mut state := CookieForm{
		lang:         ctx.lang
		grade:        'c'
		treasures:    database.treasure_options(wapp.db, ctx.lang, false) or { [] }
		partners:     database.pet_options(wapp.db, ctx.lang) or { [] }
		partner_kind: 'pet'
		effect_names: database.effect_names(wapp.db, ctx.lang) or { [] }
	}
	if c := edit {
		rd := c.release_date
		unlock_tid := if ut := database.get_unlocked_treasure(wapp.db, ctx.lang, 'cookie',
			c.cookie_id) {
			ut.treasure_id
		} else {
			0
		}
		state.edit_mode = true
		state.id = c.cookie_id
		state.name = c.name
		state.abilities = c.abilities
		state.description = c.description
		state.power_plus = c.power_plus
		state.power_plus_requirement = c.power_plus_requirement
		state.unlock_goal = c.unlock_goal
		state.grade = c.grade.str()
		state.release_date = '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
		state.lang = c.lang
		state.image = c.image
		state.unlock_treasure_id = unlock_tid
		state.combis = database.combi_edit_rows(wapp.db, ctx.lang, 'cookie', c.cookie_id) or { [] }
	}
	return state
}

// cookie_form_overlay_post restores the submitted scalar values after a
// failed submission. The dynamic editors (combo rows, treasure combobox)
// cannot be rebuilt from a bare form POST, so they keep their defaults.
fn cookie_form_overlay_post(ctx &Context, mut state CookieForm) {
	state.name = ctx.form['name'] or { state.name }
	state.abilities = ctx.form['abilities'] or { state.abilities }
	state.description = ctx.form['description'] or { state.description }
	state.power_plus = ctx.form['power_plus'] or { state.power_plus }
	state.power_plus_requirement = ctx.form['power_plus_requirement'] or { state.power_plus_requirement }
	state.unlock_goal = ctx.form['unlock_goal'] or { state.unlock_goal }
	state.grade = ctx.form['grade'] or { state.grade }
	state.release_date = ctx.form['release_date'] or { state.release_date }
	if lang := ctx.form['lang'] {
		state.lang = lang
	}
	if raw := ctx.form['unlock_treasure_id'] {
		if raw != '' && raw != '__new__' {
			state.unlock_treasure_id = raw.int()
		}
	}
}

// cookie_submit_error returns a failed cookie submission: htmx requests get
// an OOB error banner (the form stays in place with the user's input intact);
// plain requests get the full form page re-rendered with the banner inline.
fn cookie_submit_error(wapp &App, ctx &Context, edit ?database.CookieView, form_error string) veb.Result {
	if ctx.is_htmx_request() {
		err_msg := form_error
		return $veb.html('./templates/components/form_error.html')
	}
	languages := api.available_lang()
	grades := models.grade_values
	mut state := cookie_form_state(wapp, ctx, edit)
	cookie_form_overlay_post(ctx, mut state)
	state.form_error = form_error
	return $veb.html('./templates/admin/new_cookie.html')
}

// pet_form_state builds the PetForm for the create page (edit = none) or the
// edit page. Shared by the GET render and the failed-POST re-render.
fn pet_form_state(wapp &App, ctx &Context, edit ?database.PetView) PetForm {
	mut state := PetForm{
		lang:         ctx.lang
		grade:        'c'
		treasures:    database.treasure_options(wapp.db, ctx.lang, false) or { [] }
		partners:     database.cookie_options(wapp.db, ctx.lang) or { [] }
		partner_kind: 'cookie'
		effect_names: database.effect_names(wapp.db, ctx.lang) or { [] }
	}
	if p := edit {
		rd := p.release_date
		unlock_tid := if ut := database.get_unlocked_treasure(wapp.db, ctx.lang, 'pet', p.pet_id) {
			ut.treasure_id
		} else {
			0
		}
		state.edit_mode = true
		state.id = p.pet_id
		state.name = p.name
		state.abilities = p.abilities
		state.description = p.description
		state.grade = p.grade.str()
		state.release_date = '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
		state.lang = p.lang
		state.image = p.image
		state.unlock_treasure_id = unlock_tid
		state.combis = database.combi_edit_rows(wapp.db, ctx.lang, 'pet', p.pet_id) or { [] }
	}
	return state
}

// pet_form_overlay_post restores the submitted scalar values after a failed
// submission (the dynamic editors keep their defaults, see cookie overlay).
fn pet_form_overlay_post(ctx &Context, mut state PetForm) {
	state.name = ctx.form['name'] or { state.name }
	state.abilities = ctx.form['abilities'] or { state.abilities }
	state.description = ctx.form['description'] or { state.description }
	state.grade = ctx.form['grade'] or { state.grade }
	state.release_date = ctx.form['release_date'] or { state.release_date }
	if lang := ctx.form['lang'] {
		state.lang = lang
	}
	if raw := ctx.form['unlock_treasure_id'] {
		if raw != '' && raw != '__new__' {
			state.unlock_treasure_id = raw.int()
		}
	}
}

// pet_submit_error returns a failed pet submission: htmx requests get an OOB
// error banner; plain requests get the full form page with the banner inline.
fn pet_submit_error(wapp &App, ctx &Context, edit ?database.PetView, form_error string) veb.Result {
	if ctx.is_htmx_request() {
		err_msg := form_error
		return $veb.html('./templates/components/form_error.html')
	}
	languages := api.available_lang()
	grades := models.grade_values
	mut state := pet_form_state(wapp, ctx, edit)
	pet_form_overlay_post(ctx, mut state)
	state.form_error = form_error
	return $veb.html('./templates/admin/new_pet.html')
}

// treasure_form_state builds the TreasureForm for the create page (edit =
// none) or the edit page. Shared by the GET render and the failed-POST
// re-render.
fn treasure_form_state(wapp &App, ctx &Context, edit ?database.TreasureView) TreasureForm {
	mut state := TreasureForm{
		lang:         ctx.lang
		effect_names: database.effect_names(wapp.db, ctx.lang) or { [] }
		cookies:      database.cookie_options(wapp.db, ctx.lang) or { [] }
		pets:         database.pet_options(wapp.db, ctx.lang) or { [] }
		bases:        database.normal_treasure_options(wapp.db, ctx.lang) or { [] }
	}
	if t := edit {
		rd := t.release_date
		state.edit_mode = true
		state.id = t.treasure_id
		state.name = t.name
		state.description = t.description
		state.grade = if g := t.grade {
			g.str()
		} else {
			''
		}
		state.effects = effect_rows_from_db(database.treasure_effect_rows(wapp.db, ctx.lang,
			t.treasure_id, models.EffectState.normal) or { [] })
		state.blessed_effects = effect_rows_from_db(database.treasure_effect_rows(wapp.db,
			ctx.lang, t.treasure_id, models.EffectState.blessed) or { [] })
		state.is_evolved = t.is_evolved
		state.is_power_plus = t.is_power_plus
		state.base_treasure_id = if bid := t.base_treasure_id {
			bid
		} else {
			0
		}
		state.release_date = '${rd.year:04d}-${int(rd.month):02d}-${rd.day:02d}'
		state.lang = t.lang
		state.image = t.image
		state.unlock_cookie_id = if cid := t.unlock_cookie_id {
			cid
		} else {
			0
		}
		state.unlock_pet_id = if pid := t.unlock_pet_id {
			pid
		} else {
			0
		}
	}
	return state
}

// treasure_form_overlay_post restores the submitted scalar values after a
// failed submission (effect rows and the unlock combobox keep their
// defaults — they cannot be rebuilt from a bare form POST).
fn treasure_form_overlay_post(ctx &Context, mut state TreasureForm) {
	state.name = ctx.form['name'] or { state.name }
	state.description = ctx.form['description'] or { state.description }
	state.grade = ctx.form['grade'] or { state.grade }
	state.release_date = ctx.form['release_date'] or { state.release_date }
	if lang := ctx.form['lang'] {
		state.lang = lang
	}
	state.is_evolved = ctx.form['is_evolved'] == 'true'
	state.is_power_plus = ctx.form['is_power_plus'] == 'true'
	if raw := ctx.form['base_treasure_id'] {
		state.base_treasure_id = raw.int()
	}
	if raw := ctx.form['unlock_cookie_id'] {
		state.unlock_cookie_id = raw.int()
	}
	if raw := ctx.form['unlock_pet_id'] {
		state.unlock_pet_id = raw.int()
	}
}

// treasure_submit_error returns a failed treasure submission: htmx requests
// get an OOB error banner; plain requests get the full form page with the
// banner inline.
fn treasure_submit_error(wapp &App, ctx &Context, edit ?database.TreasureView, form_error string) veb.Result {
	if ctx.is_htmx_request() {
		err_msg := form_error
		return $veb.html('./templates/components/form_error.html')
	}
	languages := api.available_lang()
	grades := models.grade_values
	mut state := treasure_form_state(wapp, ctx, edit)
	treasure_form_overlay_post(ctx, mut state)
	state.form_error = form_error
	return $veb.html('./templates/admin/new_treasure.html')
}
