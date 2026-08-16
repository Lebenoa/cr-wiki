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
		ctx.noindex = true
		return $veb.html()
	} else if ctx.req.method == .post {
		if !verify_turnstile(ctx, 'login') {
			ctx.res.set_status(.forbidden)
			return submit_error(veb.tr(ctx.lang, 'turnstile_form_failed'), mut ctx)
		}

		username := ctx.form['username']
		password := ctx.form['password']

		if username == '' || password == '' {
			ctx.res.set_status(.bad_request)
			return submit_error(invalid_credentials, mut ctx)
		}

		user := sql wapp.db {
			select from models.User where username == username limit 1
		} or {
			ctx.res.set_status(.internal_server_error)
			println('Failed to query user for login: ${err}')
			return submit_error('Unexpected Error', mut ctx)
		}

		if user.len == 0 {
			// artificial hash compare
			generate := argon2.generate_from_password(password.bytes()) or {
				ctx.res.set_status(.not_found)
				return submit_error(invalid_credentials, mut ctx)
			}
			argon2.compare_hash_and_password(password.bytes(), generate.bytes()) or {}
			ctx.res.set_status(.not_found)
			return submit_error(invalid_credentials, mut ctx)
		}

		first_user := user.first()
		argon2.compare_hash_and_password(password.bytes(), first_user.password.bytes()) or {
			ctx.res.set_status(.not_found)
			return submit_error(invalid_credentials, mut ctx)
		}

		session_key := rand.uuid_v4()
		wapp.session_mu.lock()
		wapp.sessions[session_key] = first_user
		wapp.session_mu.unlock()
		ctx.set_cookie(
			name:  session_cookie_key
			value: session_key
			// value: (crand.bytes(32) or { return ctx.not_found() })[..32].bytestr()
			path:  '/'
			http_only: true
			same_site: .same_site_lax_mode
		)
		return submit_success(mut ctx, '/')
	}
	return ctx.not_found()
}
