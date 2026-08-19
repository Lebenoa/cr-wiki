module app

import database

// The cookie/pet/treasure picker lists are identical for every visitor of a
// language and change only when an admin edits the catalog, yet /builds and
// its htmx partials rebuilt all three per request: treasure_options alone
// reads 872 treasures, 1744 translations, 1785 effect links and their
// translations, then sorts the lot. Measured, that made /builds ~105ms of
// server time against ~1ms for every other page. They are built once per
// language here and dropped whenever a cookie, pet or treasure is written.
struct OptionsCache {
mut:
	cookies map[string][]database.IdNameOption
	pets    map[string][]database.IdNameOption
	// equippable treasures (the build pickers) and the full list (admin forms)
	treasures     map[string][]database.IdNameOption
	all_treasures map[string][]database.IdNameOption
}

// cookie_options returns the cached cookie picker list for `lang`. Callers
// must treat the result as read-only: V arrays assign by reference, so this
// is the shared cached list, not a copy.
fn (wapp &App) cookie_options(lang string) []database.IdNameOption {
	rlock wapp.options {
		if v := wapp.options.cookies[lang] {
			return v
		}
	}
	v := database.cookie_options(wapp.db, lang) or { [] }
	lock wapp.options {
		wapp.options.cookies[lang] = v
	}
	return v
}

// pet_options returns the cached pet picker list for `lang`; read-only, see
// cookie_options.
fn (wapp &App) pet_options(lang string) []database.IdNameOption {
	rlock wapp.options {
		if v := wapp.options.pets[lang] {
			return v
		}
	}
	v := database.pet_options(wapp.db, lang) or { [] }
	lock wapp.options {
		wapp.options.pets[lang] = v
	}
	return v
}

// treasure_options returns the cached treasure picker list for `lang`:
// `equippable` drops the Power+ friendly-run items the build form cannot
// pick. Read-only, see cookie_options.
fn (wapp &App) treasure_options(lang string, equippable bool) []database.IdNameOption {
	rlock wapp.options {
		if equippable {
			if v := wapp.options.treasures[lang] {
				return v
			}
		} else {
			if v := wapp.options.all_treasures[lang] {
				return v
			}
		}
	}
	v := database.treasure_options(wapp.db, lang, equippable) or { [] }
	lock wapp.options {
		if equippable {
			wapp.options.treasures[lang] = v
		} else {
			wapp.options.all_treasures[lang] = v
		}
	}
	return v
}

// normal_treasure_options lists the non-evolved equippable treasures for the
// admin form's "base treasure" selector. Filtered off the cached equippable
// list rather than re-queried; the filtered array is fresh, so it is the one
// option list callers may keep.
fn (wapp &App) normal_treasure_options(lang string) []database.IdNameOption {
	all := wapp.treasure_options(lang, true)
	mut out := []database.IdNameOption{cap: all.len}
	for opt in all {
		if !opt.is_evolved {
			out << opt
		}
	}
	return out
}

// invalidate_options drops every cached picker list. Called after any catalog
// write, since a new or renamed cookie/pet/treasure has to show up in the
// pickers on the next request — including the admin's own redirect.
fn (wapp &App) invalidate_options() {
	lock wapp.options {
		wapp.options.cookies.clear()
		wapp.options.pets.clear()
		wapp.options.treasures.clear()
		wapp.options.all_treasures.clear()
	}
}
