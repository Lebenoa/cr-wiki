module database

import db.sqlite
import log

// SitemapEntry is one indexable detail URL (kind maps to /{kind}s/:id).
pub struct SitemapEntry {
pub:
	kind string
	id   int
}

// sitemap_entries lists every cookie/pet/treasure id for the sitemap.
pub fn sitemap_entries(conn sqlite.DB) ![]SitemapEntry {
	mut rows := []SitemapEntry{}
	for kind, table in {
		'cookie':   'cookie'
		'pet':      'pet'
		'treasure': 'treasure'
	} {
		results := conn.exec('SELECT ${kind}_id AS id FROM ${table} ORDER BY ${kind}_id') or {
			log.warn('sitemap: ${table} query failed: ${err}')
			continue
		}
		for row in results {
			id := row.get_int('id')
			if id > 0 {
				rows << SitemapEntry{kind, id}
			}
		}
	}
	return rows
}
