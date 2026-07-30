module models

import time

pub struct User {
	pub:
	user_id ?int @[primary; serial]
	username string @[unique]
	password string
	is_admin bool
	created_at time.Time
}
