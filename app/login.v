module app

import veb
import database.models
import crypto.argon2
import rand

const invalid_credentials = "Invalid username/password"

@[get; post]
pub fn (mut wapp App) login(mut ctx Context) veb.Result {
	if ctx.user != none {
		return ctx.redirect("/")
	}

	if ctx.req.method == .get {
		ctx.set_translate_title("login_page_title")
		return $veb.html()
	} else if ctx.req.method == .post {
		username := ctx.form['username']
		password := ctx.form['password']

		if username == '' || password == '' {
			ctx.res.set_status(.bad_request)
			return ctx.text(invalid_credentials)
		}

		user := sql wapp.db {
			select from models.User where username == username limit 1
		} or {
			ctx.res.set_status(.internal_server_error)
			println('Failed to query user for login: ${err}')
			return ctx.text("Unexpected Error")
		}

		if user.len == 0 {
			// artificial hash compare
			generate := argon2.generate_from_password(password.bytes()) or {
				ctx.res.set_status(.not_found)
				return ctx.text(invalid_credentials)
			}
			argon2.compare_hash_and_password(password.bytes(), generate.bytes()) or {}
			ctx.res.set_status(.not_found)
			return ctx.text(invalid_credentials)
		}

		first_user := user.first()
		argon2.compare_hash_and_password(password.bytes(), first_user.password.bytes()) or {
			ctx.res.set_status(.not_found)
			return ctx.text(invalid_credentials)
		}

		session_key := rand.uuid_v4()
		sessions[session_key] = first_user
		ctx.set_cookie(
			name:  session_cookie_key
			value: session_key
			// value: (crand.bytes(32) or { return ctx.not_found() })[..32].bytestr()
			path:  '/'
			http_only: true
			same_site: .same_site_lax_mode
		)
		return ctx.redirect("/")
	}
	return ctx.not_found()
}
