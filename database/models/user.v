module models

import time

pub struct User {
	id ?int @[primary; serial]
	username string
	password string
	created_at time.Time
}
