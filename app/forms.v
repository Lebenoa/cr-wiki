module app

import strconv
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
	value string // plain number, '' = no value
	unit  string // 'percent' | 'second' | 'flat'
}

// effect_rows_from_db maps the treasure's stored effect rows to form rows,
// formatting the numeric value for the editor's number input.
fn effect_rows_from_db(rows []database.EffectRowData) []EffectRow {
	mut out := []EffectRow{}
	for r in rows {
		value_str := if v := r.value {
			s := v.str()
			if s.contains('.') {
				s.trim_right('0').trim_right('.')
			} else {
				s
			}
		} else {
			''
		}
		out << EffectRow{
			name:  r.name
			value: value_str
			unit:  r.unit.str()
		}
	}
	return out
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
	return database.CreateCookieParams{
		lang:              ctx.form['lang'] or { ctx.lang }
		name:              ctx.form['name']
		abilities:         ctx.form['abilities']
		description:       ctx.form['description']
		grade:             grade
		image:             if image == '' {
			none
		} else {
			image
		}
		power_plus:        ctx.form['power_plus']
		release_date:      parse_release_date(mut ctx)!
		unlock_treasure_id: parse_optional_id(mut ctx, 'unlock_treasure_id')
	}
}

fn parse_pet_form(mut ctx Context) !database.CreatePetParams {
	image := upload_image(mut ctx, 'pets')!
	grade := models.Grade.from(ctx.form['grade']) or {
		return error('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
	}
	return database.CreatePetParams{
		lang:              ctx.form['lang'] or { ctx.lang }
		name:              ctx.form['name']
		abilities:         ctx.form['abilities']
		description:       ctx.form['description']
		grade:             grade
		image:             if image == '' {
			none
		} else {
			image
		}
		release_date:      parse_release_date(mut ctx)!
		unlock_treasure_id: parse_optional_id(mut ctx, 'unlock_treasure_id')
	}
}

// parse_effect_inputs reads the treasure form's structured effect rows for
// one state (indexed fields `${prefix}_name_N`, `${prefix}_value_N`,
// `${prefix}_unit_N`) into EffectInputs in submitted order. Rows with a blank
// name and value are skipped (the trailing blank "add row"); a value without
// a name is an error.
fn parse_effect_inputs(mut ctx Context, prefix string) ![]database.EffectInput {
	mut effects := []database.EffectInput{}
	mut i := 0
	for {
		mut name := ctx.form['${prefix}_name_${i}'] or { break }
		name = name.trim_space()
		value_str := ctx.form['${prefix}_value_${i}'] or { '' }
		unit_str := ctx.form['${prefix}_unit_${i}'] or { 'flat' }
		if name == '' {
			if value_str != '' {
				return error('Effect ${i + 1}: name is required')
			}
			i++
			continue
		}
		mut value := ?f32(none)
		if value_str != '' {
			value = f32(strconv.atof64(value_str, strconv.AtoF64Param{}) or {
				return error('Effect ${i + 1}: invalid value "${value_str}" (expected a number)')
			})
		}
		unit := models.EffectUnit.from(unit_str) or {
			return error('Effect ${i + 1}: invalid unit "${unit_str}"')
		}
		effects << database.EffectInput{
			name:  name
			value: value
			unit:  unit
		}
		i++
	}
	return effects
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
