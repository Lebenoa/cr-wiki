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
	edit_mode    bool
	id           int
	name         string
	abilities    string
	description  string
	power_plus   string
	grade        string
	release_date string
	lang         string
	image        ?string
}

// PetForm carries the form state shared by the pet create/edit pages.
pub struct PetForm {
pub:
	edit_mode    bool
	id           int
	name         string
	abilities    string
	description  string
	grade        string
	release_date string
	lang         string
	image        ?string
}

// TreasureForm carries the form state shared by the treasure create/edit pages.
pub struct TreasureForm {
pub:
	edit_mode       bool
	id              int
	name            string
	description     string
	grade           string // '' = no wiki grade
	effects         string // one "Name | 12%" per line
	blessed_effects string
	is_evolved      bool
	release_date    string
	lang            string
	image           ?string
}

fn parse_cookie_form(mut ctx Context) !database.CreateCookieParams {
	image := upload_image(mut ctx, 'cookies')!
	grade := models.Grade.from(ctx.form['grade']) or {
		return error('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
	}
	return database.CreateCookieParams{
		lang:         ctx.form['lang'] or { ctx.lang }
		name:         ctx.form['name']
		abilities:    ctx.form['abilities']
		description:  ctx.form['description']
		grade:        grade
		image:        if image == '' {
			none
		} else {
			image
		}
		power_plus:   ctx.form['power_plus']
		release_date: parse_release_date(mut ctx)!
	}
}

fn parse_pet_form(mut ctx Context) !database.CreatePetParams {
	image := upload_image(mut ctx, 'pets')!
	grade := models.Grade.from(ctx.form['grade']) or {
		return error('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
	}
	return database.CreatePetParams{
		lang:         ctx.form['lang'] or { ctx.lang }
		name:         ctx.form['name']
		abilities:    ctx.form['abilities']
		description:  ctx.form['description']
		grade:        grade
		image:        if image == '' {
			none
		} else {
			image
		}
		release_date: parse_release_date(mut ctx)!
	}
}

// parse_effect_inputs parses the treasure 'effects' textarea: one effect per
// line, "Name | 12%". The value is optional; a trailing % or s suffix picks
// the unit (percent/second), anything else is treated as a plain number.
fn parse_effect_inputs(s string) ![]database.EffectInput {
	mut effects := []database.EffectInput{}
	for line in s.split_into_lines() {
		l := line.trim_space()
		if l == '' {
			continue
		}
		mut name := l
		mut value := ?f32(none)
		mut unit := models.EffectUnit.flat
		idx := l.last_index('|') or { -1 }
		if idx >= 0 {
			name = l[..idx].trim_space()
			val_str := l[idx + 1..].trim_space()
			if val_str != '' {
				mut num_str := val_str
				if val_str.ends_with('%') {
					unit = models.EffectUnit.percent
					num_str = val_str[..val_str.len - 1]
				} else if val_str.ends_with('s') {
					unit = models.EffectUnit.second
					num_str = val_str[..val_str.len - 1]
				}
				value = f32(strconv.atof64(num_str, strconv.AtoF64Param{}) or {
					return error('Invalid effect value: "${val_str}" (expected e.g. 12%, 3s, or a plain number)')
				})
			}
		}
		if name == '' {
			return error('Invalid effect line: "${l}" (missing effect name)')
		}
		effects << database.EffectInput{
			name:  name
			value: value
			unit:  unit
		}
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
		lang:            ctx.form['lang'] or { ctx.lang }
		name:            ctx.form['name']
		description:     ctx.form['description']
		image:           if image == '' {
			none
		} else {
			image
		}
		grade:           grade
		is_evolved:      ctx.form['is_evolved'] == 'true'
		release_date:    parse_release_date(mut ctx)!
		effects:         parse_effect_inputs(ctx.form['effects'])!
		blessed_effects: parse_effect_inputs(ctx.form['blessed_effects'])!
	}
}
