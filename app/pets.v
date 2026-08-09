module app

import veb
import time
import api
import database
import database.models

pub fn (wapp &App) pets(mut ctx Context) veb.Result {
	ctx.set_translate_title("pets_page_title")
	page_size := 30
	grades := models.grade_values

	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	pets := database.select_pets(wapp.db, ctx.lang, page_size, (page - 1) * page_size) or {
		println(err)
		return ctx.html("Something went wrong")
	}
	next_page := if pets.len == page_size {
		page + 1
	} else {
		0
	}

	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html("./templates/partials/pet_cards.html")
	}

	return $veb.html()
}

@['/pets/:id']
pub fn (wapp &App) pet_info(mut ctx Context, id int) veb.Result {
	pet := database.get_pet(wapp.db, ctx.lang, id) or { return ctx.not_found() }
	ctx.page_title = '${pet.name} | Classic Fan/Wiki'
	return $veb.html("./templates/views/pet.html")
}

@['/pets/new'; get; post]
pub fn (wapp &App) new_pet(mut ctx Context) veb.Result {
	cur_user := ctx.user or { return ctx.not_found() }
	if !cur_user.is_admin {
		return ctx.not_found()
	}

	if ctx.req.method == .get {
		ctx.page_title = 'New Pet | Classic/FanWiki'
		languages := api.available_lang()
		grades := models.grade_values
		return $veb.html("./templates/admin/new_pet.html")
	} else if ctx.req.method == .post {
		image := upload_image(mut ctx, 'pets') or {
			ctx.res.set_status(.bad_request)
			return ctx.text(err.msg())
		}

		grade := match ctx.form['grade'] {
			'e' {
				models.Grade.e
			}
			'c' {
				models.Grade.c
			}
			'b' {
				models.Grade.b
			}
			'a' {
				models.Grade.a
			}
			's' {
				models.Grade.s
			}
			's_plus' {
				models.Grade.s_plus
			}
			'l' {
				models.Grade.l
			}
			else {
				ctx.res.set_status(.bad_request)
				return ctx.text('Invalid grade: expected one of e, c, b, a, s, s_plus, l')
			}
		}

		date_str := ctx.form['release_date']
		release_date := if date_str != '' {
			time.parse('${date_str} 00:00:00') or {
				ctx.res.set_status(.bad_request)
				return ctx.text('Invalid release date, expected YYYY-MM-DD')
			}
		} else {
			time.now()
		}

		lang_str := ctx.form['lang'] or { ctx.lang }
		database.create_pet(wapp.db, database.CreatePetParams{
			lang:         lang_str
			name:         ctx.form['name']
			abilities:    ctx.form['abilities']
			description:  ctx.form['description']
			grade:        grade
			image:        if image == '' {
				none
			} else {
				image
			}
			release_date: release_date
		}) or {
			ctx.res.set_status(.bad_request)
			return ctx.text('Failed to create pet: ${err}')
		}

		return ctx.redirect('/pets')
	}

	return ctx.not_found()
}
