module models

import time

pub struct User {
	user_id ?int @[primary; serial]
	username string
	password string
	created_at time.Time
}
