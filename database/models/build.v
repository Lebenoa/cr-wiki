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
	combi_bonus_id ?int // id into combi_bonus when cookie+pet have a combo bonus; none = no combo
	treasure1_id int @[required; index]
	treasure2_id int @[required; index]
	treasure3_id int @[required; index]
	treasure1_blessed int // 1 when slot 1 treasure is blessed (evolved treasures only)
	treasure2_blessed int
	treasure3_blessed int
	treasure1_level int // equipped level 0-9; max by default
	treasure2_level int
	treasure3_level int
	ep           int @[index] // tier 1-7; 0 when the build is a special EP build
	ep_special   int @[index] // special tier 1-3; 0 for regular EP builds
	tag          string // comma-separated: 'score,coin' etc.
	boosts       string // comma-separated run boosts: 'energy,item_time,fast_start'
	boost        string // purchased pre-run boost key ('' = none; run_boost_keys)
	power_effects string // comma-separated owned Power+ effect keys (power_effect_keys)
	score        u64 // run result: total score
	coin         u64 // run result: coins collected
	time         u64 // run result: run duration in milliseconds (0 = not set)
	boxes        u64 // run result: boxes opened
	description  string // optional; may contain plain text/URLs
	youtube_url  string // optional; empty when the build has no video
	author       string
	user_id      ?int // null = anonymous submit
	created_at   time.Time
	expires_at   ?time.Time // null = permanent (logged-in submitter)
}
