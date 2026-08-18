module database

import db.sqlite
import log

// SitemapEntry is one indexable detail URL: section is the route segment the
// page lives under, so the caller never has to pluralize (jelly -> jellies).
pub struct SitemapEntry {
pub:
	section string
	id      int
}

// sitemap_entries lists every detail-page id for the sitemap: one row per
// entity that has a detail route. Kinds without one (skins, relics — both
// render inside their list page) are absent on purpose, as are builds, which
// expire.
pub fn sitemap_entries(conn sqlite.DB) ![]SitemapEntry {
	mut rows := []SitemapEntry{}
	// route segment -> (id column prefix, table)
	for section, entity in {
		'cookies':     'cookie'
		'pets':        'pet'
		'treasures':   'treasure'
		'episodes':    'episode'
		'ingredients': 'ingredient'
		'jellies':     'jelly'
	} {
		results := conn.exec('SELECT ${entity}_id AS id FROM ${entity} ORDER BY ${entity}_id') or {
			log.warn('sitemap: ${entity} query failed: ${err}')
			continue
		}
		for row in results {
			id := row.get_int('id')
			if id > 0 {
				rows << SitemapEntry{section, id}
			}
		}
	}
	return rows
}
