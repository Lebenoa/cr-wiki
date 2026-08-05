module app

import veb
import db.sqlite

@[heap]
pub struct Admin {
	veb.Middleware[Context]

	db &sqlite.DB
}

pub fn (wapp &Admin) before_request(mut ctx Context) bool {
	ctx.user = if user_cookie := ctx.req.cookie(session_cookie_key) {
		rlock sessions {
			if user := sessions[user_cookie.value] {
				user
			} else {
				none
			}
		}
	} else {
		none
	}

	if user := ctx.user {
		if !user.is_admin  {
			ctx.not_found()
			return false
		} else {
			return true
		}
	} else {
		ctx.not_found()
		return false
	}
}

pub fn (wapp &Admin) index(mut ctx Context) veb.Result {
	return ctx.text("from admin!")
}
