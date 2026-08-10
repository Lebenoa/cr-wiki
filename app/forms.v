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

fn parse_treasure_form(mut ctx Context) !database.CreateTreasureParams {
	image := upload_image(mut ctx, 'treasures')!
	return database.CreateTreasureParams{
		lang:         ctx.form['lang'] or { ctx.lang }
		name:         ctx.form['name']
		description:  ctx.form['description']
		image:        if image == '' {
			none
		} else {
			image
		}
		is_evolved:   ctx.form['is_evolved'] == 'true'
		release_date: parse_release_date(mut ctx)!
	}
}
