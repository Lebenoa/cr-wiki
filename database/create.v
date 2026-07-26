module database

import db.sqlite
import crypto.argon2
import models
import time

pub fn create_user(conn sqlite.DB, username string, password string) !int {
	new_user := models.User{
		username: username
		password: argon2.generate_from_password(password.bytes())!
		created_at: time.now()
	}

	id := sql conn {
		insert new_user into models.User
	}!

	return id
}
