module app

import time
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
pub struct CookieForm {
pub:
	edit_mode         bool
	id                int
	name              string
	abilities         string
	description       string
	power_plus        string
	grade             string
	release_date      string
	lang              string
	image             ?string
	unlock_treasure_id int // 0 = none; the treasure this cookie unlocks at max level
	treasures         []database.IdNameOption
}

// PetForm carries the form state shared by the pet create/edit pages.
pub struct PetForm {
pub:
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
}

// EffectRow carries one effect row in the treasure form's structured editor.
pub struct EffectRow {
pub:
	name  string
	value string // display text: "12%", "2-3%", "3s", "500000", '' = none
}

// effect_rows_from_db maps the treasure's stored effect rows to form rows,
// formatting the numeric value with its unit suffix for the editor.
fn effect_rows_from_db(rows []database.EffectRowData) []EffectRow {
	mut out := []EffectRow{}
	for r in rows {
		out << EffectRow{
			name:  r.name
			value: database.format_effect_value(r.value, r.value_min, r.value_max, r.unit)
		}
	}
	return out
}

// EffectValueParts is the parsed form of an effect's value text.
pub struct EffectValueParts {
pub:
	value     ?int
	value_min ?int
	value_max ?int
	unit      models.EffectUnit
}

// parse_effect_value splits the effect value text into its numeric parts and
// the unit derived from the suffix: "12%" -> value 12 percent, "2-3%" ->
// min 2 max 3 percent, "3s" -> value 3 second, "500000" -> value 500000
// flat, "" -> no value. Anything else is rejected here at submit time.
fn parse_effect_value(raw string) !EffectValueParts {
	mut s := raw.trim_space()
	if s == '' {
		return EffectValueParts{
			unit: models.EffectUnit.flat
		}
	}
	mut unit := models.EffectUnit.flat
	if s.ends_with('%') {
		unit = models.EffectUnit.percent
		s = s[..s.len - 1]
	} else if s.ends_with('s') {
		unit = models.EffectUnit.second
		s = s[..s.len - 1]
	}
	parts := s.split('-')
	if parts.len > 2 {
		return error('expected a number or range like 12% or 2-3%')
	}
	mut nums := []int{}
	for part in parts {
		p := part.trim_space()
		if p == '' {
			return error('expected a number or range like 12% or 2-3%')
		}
		for c in p {
			if !c.is_digit() {
				return error('expected a number or range like 12% or 2-3%')
			}
		}
		nums << p.int()
	}
	if nums.len == 1 {
		return EffectValueParts{
			value: nums[0]
			unit:  unit
		}
	}
	return EffectValueParts{
		value_min: nums[0]
		value_max: nums[1]
		unit:      unit
	}
}

// TreasureForm carries the form state shared by the treasure create/edit pages.
pub struct TreasureForm {
pub:
	edit_mode       bool
	id              int
	name            string
	description     string
	grade           string // '' = no wiki grade
	effects         []EffectRow
	blessed_effects []EffectRow
	effect_names    []string // suggestions for the effect name input
	is_evolved      bool
	release_date    string
	lang            string
	image           ?string
	unlock_cookie_id int // 0 = none; cookie whose max-level upgrade unlocks this treasure
	unlock_pet_id    int // 0 = none; pet whose max-level upgrade unlocks this treasure
	cookies         []database.IdNameOption
	pets            []database.IdNameOption
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
		power_plus:         ctx.form['power_plus']
		release_date:       parse_release_date(mut ctx)!
		unlock_treasure_id: choice.treasure_id
		new_treasure_name:  choice.new_name
	}
}

fn parse_pet_form(mut ctx Context) !database.CreatePetParams {
	image := upload_image(mut ctx, 'pets')!
	grade := models.Grade.from(ctx.form['grade']) or {
		return error('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
	}
	choice := parse_unlock_treasure(mut ctx)!
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
		effects << database.EffectInput{
			name:      name
			value:     parts.value
			value_min: parts.value_min
			value_max: parts.value_max
			unit:      parts.unit
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
		return UnlockTreasureChoice{}
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
		release_date:     parse_release_date(mut ctx)!
		effects:          parse_effect_inputs(mut ctx, 'effects')!
		blessed_effects:  parse_effect_inputs(mut ctx, 'blessed_effects')!
		unlock_cookie_id: parse_optional_id(mut ctx, 'unlock_cookie_id')
		unlock_pet_id:    parse_optional_id(mut ctx, 'unlock_pet_id')
	}
}
