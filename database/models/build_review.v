module models

import time

// BuildReview is one authenticated user's verdict on a community build:
// verified (tried the loadout, it works) or a reported issue with a reason.
// One review per user per build — re-submitting overwrites the previous
// verdict (the UNIQUE(build_id, user_id) key drives the upsert).
@[table: 'build_review']
@[unique_key: 'build_id, user_id']
pub struct BuildReview {
pub:
	build_review_id ?int @[primary; serial]
	build_id        int @[required; references: 'build(build_id)']
	user_id         int @[required; references: 'user(user_id)']
	verified        bool // true = works, false = reported issue
	reason          string // issue reason when verified=false; '' when verified
	created_at      time.Time
	updated_at      time.Time
}
