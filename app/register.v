module app

import veb
import database
import database.models
import rand

@[get; post]
pub fn (mut wapp App) register(mut ctx Context) veb.Result {
	if ctx.user != none {
		return ctx.redirect("/")
	}

	if ctx.req.method == .post {
		username := ctx.form['username'] or {
			ctx.res.set_status(.bad_request)
			return ctx.text("Username is required")
		}
		password := ctx.form['password'] or {
			ctx.res.set_status(.bad_request)
			return ctx.text("Password is required")
		}
		confirm_password := ctx.form['confirm_password'] or {
			ctx.res.set_status(.bad_request)
			return ctx.text("Confirm password is required")
		}

		if password != confirm_password {
			ctx.res.set_status(.bad_request)
			return ctx.text("Passwords do not match")
		}

		new_user_id := database.create_user(wapp.db, username, password) or {
			eprintln(err)
			ctx.res.set_status(.internal_server_error)
			return ctx.text("Failed to create user")
		}

		session_id := rand.uuid_v4()
		wapp.sessions[session_id] = sql wapp.db {
			select from models.User where user_id == new_user_id
		} or { panic("Somehow user disappeared: ${err}") }.first()
		ctx.set_cookie(
			name: session_cookie_key
			value: session_id
			path: "/"
			http_only: true
			same_site: .same_site_lax_mode
		)
		return ctx.redirect("/")
	} else if ctx.req.method == .get {
		ctx.set_translate_title("register_page_title")
		return $veb.html()
	}

	return ctx.not_found()
}
