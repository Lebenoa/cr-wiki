module database

import db.sqlite
import models

pub fn select_cookies(conn sqlite.DB) ![]models.Cookie {
	cookies := sql conn {
		select from models.Cookie limit 30
	}!

	return cookies
}
