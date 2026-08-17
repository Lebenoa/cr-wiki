module database

import db.sqlite
import crypto.argon2
import models
import time
import app.util

pub fn create_user(conn sqlite.DB, username string, password string) !int {
	new_user := models.User{
		username:   username
		password:   argon2.generate_from_password(password.bytes())!
		created_at: time.now()
	}

	id := sql conn {
		insert new_user into models.User
	}!

	return id
}

pub struct CreateCookieParams {
pub:
	lang               string
	name               string
	abilities          string
	description        string
	grade              models.Grade
	image              ?string
	power_plus         string
	power_plus_requirement string
	unlock_goal        string
	release_date       time.Time
	unlock_treasure_id ?int // treasure this cookie unlocks at max level
	new_treasure_name  string // non-empty: create this treasure, then link it
	combi_keep         map[int]CombiRowUpdate // existing combi rows to keep (id -> effect/hidden)
	combi_new          []CombiNewRow          // brand-new combi rows to create
}

// CombiRowUpdate is the submitted state of one existing combi row on the
// cookie/pet edit form: the effect name (in the form's language) and whether
// it is hidden from... nothing — wiki readers see everything, the flag just
// marks the row for the badge.
pub struct CombiRowUpdate {
pub:
	effect    string
	is_hidden bool
}

// CombiNewRow is one new combi row added on the cookie/pet form: the partner
// id (the other half of the pair) plus effect name and hidden flag.
pub struct CombiNewRow {
pub:
	partner_id int
	effect     string
	is_hidden  bool
}

// combi_effect_id resolves a combi effect name into the shared effect table
// (creating the effect when new); 0 for an empty name (no effect).
fn combi_effect_id(conn sqlite.DB, lang string, name string) !int {
	n := name.trim_space()
	if n == '' {
		return 0
	}
	// {value}-placeholder names need a per-link value (substituted at render),
	// which a combo bonus has no field for — reject so the raw token can never
	// reach readers
	if n.contains('{value}') {
		return error('combi effect "${n}" uses a {value} placeholder, which needs a per-link value a combo bonus does not carry')
	}
	return find_or_create_effect(conn, lang, n)
}

// add_combi_rows inserts new combi rows where one side is fixed (the cookie
// or pet being edited, `cookie_side`/`pet_side`; exactly one is non-zero) and
// the other comes from the form. Duplicate pairs are skipped.
fn add_combi_rows(conn sqlite.DB, cookie_side int, pet_side int, rows []CombiNewRow, lang string) ! {
	for nr in rows {
		pid := nr.partner_id
		if pid <= 0 {
			continue
		}
		cid := if cookie_side > 0 { cookie_side } else { pid }
		pid2 := if pet_side > 0 { pet_side } else { pid }
		dup := sql conn {
			select from models.CombiBonus where cookie_id == cid && pet_id == pid2
		}!
		if dup.len > 0 {
			continue
		}
		eid := combi_effect_id(conn, lang, nr.effect)!
		new_row := models.CombiBonus{
			cookie_id: cid
			pet_id:    pid2
			effect_id: if eid > 0 {
				eid
			} else {
				none
			}
			is_hidden: nr.is_hidden
		}
		sql conn {
			insert new_row into models.CombiBonus
		}!
	}
}

pub fn create_cookie(conn sqlite.DB, params CreateCookieParams) !int {
	if params.lang == '' {
		return error('cookie lang is required')
	}
	if params.name == '' {
		return error('cookie name is required')
	}
	if params.abilities == '' {
		return error('cookie abilities are required')
	}

	new_cookie := models.Cookie{
		grade:        params.grade
		image:        params.image
		release_date: params.release_date
	}

	cookie_id := sql conn {
		insert new_cookie into models.Cookie
	}!

	new_translation := models.CookieTranslation{
		owner_id:                cookie_id
		lang:                    params.lang
		name:                    params.name
		abilities:               params.abilities
		description:             params.description
		power_plus:              params.power_plus
		power_plus_requirement:  params.power_plus_requirement
		unlock_goal:             params.unlock_goal
	}

	sql conn {
		insert new_translation into models.CookieTranslation
	}!

	// link the chosen treasure to this cookie (the unlock lives on the treasure);
	// a "new treasure" name creates the treasure first, then links it
	if params.new_treasure_name != '' {
		tid := create_treasure(conn, CreateTreasureParams{
			lang:         params.lang
			name:         params.new_treasure_name
			release_date: params.release_date // treasure released together with its cookie
		})!
		sql conn {
			update models.Treasure set unlock_cookie_id = cookie_id where treasure_id == tid
		}!
	} else if utid := params.unlock_treasure_id {
		sql conn {
			update models.Treasure set unlock_cookie_id = cookie_id where treasure_id == utid
		}!
	}

	if params.combi_new.len > 0 {
		add_combi_rows(conn, cookie_id, 0, params.combi_new, params.lang)!
	}

	return cookie_id
}

pub struct CreatePetParams {
pub:
	lang               string
	name               string
	abilities          string
	description        string
	grade              models.Grade
	image              ?string
	release_date       time.Time
	unlock_treasure_id ?int // treasure this pet unlocks at max level
	new_treasure_name  string // non-empty: create this treasure, then link it
	combi_keep         map[int]CombiRowUpdate
	combi_new          []CombiNewRow
}

pub fn create_pet(conn sqlite.DB, params CreatePetParams) !int {
	if params.lang == '' {
		return error('pet lang is required')
	}
	if params.name == '' {
		return error('pet name is required')
	}
	if params.abilities == '' {
		return error('pet abilities are required')
	}

	new_pet := models.Pet{
		grade:        params.grade
		image:        params.image
		release_date: params.release_date
	}

	pet_id := sql conn {
		insert new_pet into models.Pet
	}!

	new_translation := models.PetTranslation{
		pet_id:      pet_id
		lang:        params.lang
		name:        params.name
		abilities:   params.abilities
		description: params.description
	}

	sql conn {
		insert new_translation into models.PetTranslation
	}!

	// link the chosen treasure to this pet (the unlock lives on the treasure);
	// a "new treasure" name creates the treasure first, then links it
	if params.new_treasure_name != '' {
		tid := create_treasure(conn, CreateTreasureParams{
			lang:         params.lang
			name:         params.new_treasure_name
			release_date: params.release_date // treasure released together with its pet
		})!
		sql conn {
			update models.Treasure set unlock_pet_id = pet_id where treasure_id == tid
		}!
	} else if utid := params.unlock_treasure_id {
		sql conn {
			update models.Treasure set unlock_pet_id = pet_id where treasure_id == utid
		}!
	}

	if params.combi_new.len > 0 {
		add_combi_rows(conn, 0, pet_id, params.combi_new, params.lang)!
	}

	return pet_id
}

// EffectInput is one effect entered in the treasure form: a display name plus
// an optional value string. The value is the display text for the +0..+9
// columns — a single value ("12%", "3s", "500000"), a min/max range
// ("2-3%"), or absent (legacy names carry their own numbers); its format is
// validated at submit.
pub struct EffectInput {
pub:
	name  string
	value string
}

pub struct CreateTreasureParams {
pub:
	lang            string
	name            string
	description     string
	image           ?string
	grade           ?int // models.Grade value; none = no wiki grade
	is_evolved      bool
	is_power_plus   bool // POWER+ treasure (friendly-run bonus) cannot be equipped
	base_treasure_id ?int // evolved rows link back to their normal base treasure
	release_date    time.Time
	effects         []EffectInput
	blessed_effects []EffectInput
	unlock_cookie_id ?int // cookie whose max-level upgrade unlocks this treasure
	unlock_pet_id    ?int // pet whose max-level upgrade unlocks this treasure
}

// validate_evolved_link enforces the base-treasure invariants shared by
// create and update: a base link requires is_evolved, and cannot point at
// the treasure itself (create passes 0 — a fresh row cannot reference its
// own not-yet-assigned id).
fn validate_evolved_link(id int, params CreateTreasureParams) ! {
	if bid := params.base_treasure_id {
		if !params.is_evolved {
			return error('base_treasure_id requires is_evolved')
		}
		if bid == id {
			return error('base_treasure_id cannot point at the treasure itself')
		}
	}
}

pub fn create_treasure(conn sqlite.DB, params CreateTreasureParams) !int {
	if params.lang == '' {
		return error('treasure lang is required')
	}
	if params.name == '' {
		return error('treasure name is required')
	}
	validate_evolved_link(0, params)!

	new_treasure := models.Treasure{
		image:            params.image
		grade:            params.grade
		is_evolved:       params.is_evolved
		is_power_plus:    params.is_power_plus
		base_treasure_id: params.base_treasure_id
		release_date:     params.release_date
		unlock_cookie_id: params.unlock_cookie_id
		unlock_pet_id:    params.unlock_pet_id
	}

	treasure_id := sql conn {
		insert new_treasure into models.Treasure
	}!

	new_translation := models.TreasureTranslation{
		treasure_id: treasure_id
		lang:        params.lang
		name:        params.name
		description: params.description
	}

	sql conn {
		insert new_translation into models.TreasureTranslation
	}!

	replace_effects(conn, treasure_id, params.lang, params.effects, models.EffectState.normal)!
	replace_effects(conn, treasure_id, params.lang, params.blessed_effects,
		models.EffectState.blessed)!

	return treasure_id
}

// replace_effects rewrites the treasure's effect links AND their per-level
// values for one state: existing links and treasure_level rows are dropped,
// then each submitted effect is re-created (so the saved order always matches
// the form). The +0/+9 column values live in treasure_level: the form edits
// one range string per effect ("2-4%" -> level 0 "2%", level 9 "4%"), while
// the catalog seed carries the full per-level rows. Effect names are matched
// in the form's language so re-saving an edit reuses the same effect row
// (translations are per-lang). The delete + re-insert runs in one transaction,
// so a failed insert cannot leave the treasure with its effects half-removed.
pub fn replace_effects(conn sqlite.DB, treasure_id int, lang string, effects []EffectInput, state models.EffectState) ! {
	conn.begin()!
	defer {
		conn.rollback() or {}
	}

	existing := sql conn {
		select from models.TreasureEffect where treasure_id == treasure_id && state == state
	}!
	for link in existing {
		sql conn {
			delete from models.TreasureEffect where treasure_effect_id == link.treasure_effect_id
		}!
	}
	old_levels := sql conn {
		select from models.TreasureLevel where treasure_id == treasure_id && state == state
	}!
	for tl in old_levels {
		sql conn {
			delete from models.TreasureLevel where treasure_level_id == tl.treasure_level_id
		}!
	}

	// duplicate effect names resolve to the same effect row, and the link
	// table's unique key is (treasure_id, effect_id, state); keep the first
	// occurrence so a repeated name can't violate the constraint
	mut linked := map[int]bool{}
	for e in effects {
		effect_id := find_or_create_effect(conn, lang, e.name)!
		if effect_id in linked {
			continue
		}
		linked[effect_id] = true
		new_link := models.TreasureEffect{
			treasure_id: treasure_id
			effect_id:   effect_id
			state:       state
		}
		sql conn {
			insert new_link into models.TreasureEffect
		}!
		if e.value != '' {
			v0, v9, _ := util.split_value_text(e.value)
			lv0 := models.TreasureLevel{
				treasure_id: treasure_id
				level:       0
				effect_id:   effect_id
				state:       state
				values:      v0
			}
			lv9 := models.TreasureLevel{
				treasure_id: treasure_id
				level:       9
				effect_id:   effect_id
				state:       state
				values:      v9
			}
			sql conn {
				insert lv0 into models.TreasureLevel
			}!
			sql conn {
				insert lv9 into models.TreasureLevel
			}!
		}
	}

	conn.commit()!
}

// find_or_create_effect returns the effect id whose translation in `lang` has
// the given name, creating the effect row and its translation when new.
fn find_or_create_effect(conn sqlite.DB, lang string, name string) !int {
	plang := lang
	pname := name
	existing := sql conn {
		select from models.EffectTranslation where lang == plang && name == pname
	}!
	if existing.len > 0 {
		return existing.first().effect_id
	}

	new_effect := models.Effect{}
	effect_id := sql conn {
		insert new_effect into models.Effect
	}!
	new_tr := models.EffectTranslation{
		effect_id: effect_id
		lang:      lang
		name:      name
	}
	sql conn {
		insert new_tr into models.EffectTranslation
	}!
	return effect_id
}


// create_build stores one community-submitted loadout. `expires_at` is set
// for anonymous submitters so the build is dropped from the list after the
// TTL; logged-in submitters pass `none` for a permanent build. `ep` is the
// tier 1-7 (0 for special builds) and `ep_special` the special tier 1-3;
// `tags` is the score/coin/autofarm list, stored comma-separated.
// `description`/`youtube_url` are optional and may be empty; the run-result
// stats (score/coin/time_ms/boxes) are unsigned and 0 when not submitted.
pub fn create_build(conn sqlite.DB, cookie_id int, cookie2_id int, pet_id int, treasure1_id int, treasure2_id int, treasure3_id int, treasure1_blessed int, treasure2_blessed int, treasure3_blessed int, treasure1_level int, treasure2_level int, treasure3_level int, ep int, ep_special int, tags []string, boosts []string, boost string, power_effects []string, score u64, coin u64, time_ms u64, boxes u64, description string, youtube_url string, author string, user_id ?int, expires_at ?time.Time) !int {
	combi_id := combi_bonus_id_for(conn, cookie_id, pet_id)!
	build := models.Build{
		cookie_id:       cookie_id
		cookie2_id:      cookie2_id
		pet_id:          pet_id
		combi_bonus_id:  if combi_id > 0 {
			combi_id
		} else {
			none
		}
		treasure1_id:    treasure1_id
		treasure2_id:    treasure2_id
		treasure3_id:    treasure3_id
		treasure1_blessed: sanitize_blessed(conn, treasure1_id, treasure1_blessed)
		treasure2_blessed: sanitize_blessed(conn, treasure2_id, treasure2_blessed)
		treasure3_blessed: sanitize_blessed(conn, treasure3_id, treasure3_blessed)
		treasure1_level: clamp_level(treasure1_level)
		treasure2_level: clamp_level(treasure2_level)
		treasure3_level: clamp_level(treasure3_level)
		ep:           ep
		ep_special:   ep_special
		tag:          tags.join(',')
		boosts:       boosts.join(',')
		boost:        boost
		power_effects: power_effects.join(',')
		score:        score
		coin:         coin
		time:         time_ms
		boxes:        boxes
		description:  description
		youtube_url:  youtube_url
		author:       author
		user_id:      user_id
		created_at:   time.now()
		expires_at:   expires_at
	}
	return sql conn {
		insert build into models.Build
	}!
}

// upsert_build_review records (or overwrites) one user's verdict on a build:
// verified = the loadout works, otherwise a reported issue with a reason.
// The UNIQUE(build_id, user_id) key means re-submitting flips the previous
// verdict instead of stacking duplicates. `reason` is only meaningful for
// reported issues (verified=false); verified reviews pass ''.
pub fn upsert_build_review(conn sqlite.DB, build_id int, user_id int, verified bool, reason string) ! {
	existing := sql conn {
		select from models.BuildReview where build_id == build_id && user_id == user_id limit 1
	}!
	if existing.len > 0 {
		review_id := existing.first().build_review_id or { return }
		now := time.now()
		sql conn {
			update models.BuildReview set verified = verified, reason = reason, updated_at = now where build_review_id == review_id
		}!
		return
	}
	now := time.now()
	new_review := models.BuildReview{
		build_id:   build_id
		user_id:    user_id
		verified:   verified
		reason:     reason
		created_at: now
		updated_at: now
	}
	sql conn {
		insert new_review into models.BuildReview
	}!
}
