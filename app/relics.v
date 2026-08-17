module app

import veb
import database
import database.models

// relics renders the relic catalog: episode relic sets (grouped under their
// owning episode) followed by the event relics, all from the Phase A relic
// table seeded from cookierundb.
pub fn (mut wapp App) relics(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('relics_page_title')
	ctx.set_translate_desc('relics_page_description')
	groups := database.select_relics(wapp.db, ctx.lang)
	return $veb.html()
}

// episode_kind_label localizes an episode kind code (story/special/event).
pub fn (ctx &Context) episode_kind_label(kind string) string {
	return veb.tr(ctx.lang, 'episode_kind_${kind}')
}

// stars_label renders an episode's difficulty as repeated stars ('' when the
// episode has no star rating).
pub fn (ctx &Context) stars_label(stars ?int) string {
	if s := stars {
		return '★'.repeat(s)
	}
	return ''
}

// grade_label_int renders a stored grade value (models.Grade) as its label
// ('S+' for s_plus), or '' when the row has no grade.
pub fn (ctx &Context) grade_label_int(grade ?int) string {
	if g := grade {
		gr := models.Grade.from(g) or { return '' }
		return ctx.grade_label(gr.str())
	}
	return ''
}

// odds_label renders a disclosed percentage or '—' when the page shows none.
pub fn (ctx &Context) odds_label(odds ?f64) string {
	if o := odds {
		return '${o}%'
	}
	return '—'
}
