module models

import time

// Build is one community-submitted loadout: a cookie, a pet and three
// treasures plus the submitter's score (EP). Anonymous submissions get an
// expiry time so they don't linger forever; logged-in ones are permanent.
@[table: 'build']
pub struct Build {
pub:
	build_id     ?int @[primary; serial]
	cookie_id    int @[required; index]
	cookie2_id   int @[index] // relay cookie; 0 = none
	pet_id       int @[required; index]
	treasure1_id int @[required]
	treasure2_id int @[required]
	treasure3_id int @[required]
	ep           int @[index] // tier 1-7; 0 when the build is a special EP build
	ep_special   int @[index] // special tier 1-3; 0 for regular EP builds
	tag          string // 'score' | 'coin' | 'autofarm'
	author       string
	user_id      ?int // null = anonymous submit
	created_at   time.Time
	expires_at   ?time.Time // null = permanent (logged-in submitter)
}
