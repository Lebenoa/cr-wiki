module app

import database
import veb

// sitemap serves /sitemap.xml: static pages plus every cookie/pet/treasure
// detail URL. Query-filtered build URLs are excluded on purpose — crawlers
// should reach builds through /builds.
@['/sitemap.xml']
pub fn (wapp &App) sitemap(mut ctx Context) veb.Result {
	base := ctx.site_url()
	mut xml := '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
	for path in ['/', '/cookies', '/pets', '/treasures', '/builds'] {
		xml += '<url><loc>${base}${path}</loc></url>\n'
	}
	entries := database.sitemap_entries(wapp.db) or { [] }
	for e in entries {
		xml += '<url><loc>${base}/${e.kind}s/${e.id}</loc></url>\n'
	}
	xml += '</urlset>\n'
	ctx.set_content_type('application/xml')
	return ctx.text(xml)
}

// robots_txt serves /robots.txt: crawl everything except form/auth pages,
// and points crawlers at the sitemap.
@['/robots.txt']
pub fn (wapp &App) robots_txt(mut ctx Context) veb.Result {
	body := 'User-agent: *\nDisallow: /new\nDisallow: /login\nDisallow: /register\nDisallow: /search\nDisallow: /*/edit\n\nSitemap: ${ctx.site_url()}/sitemap.xml\n'
	ctx.set_content_type('text/plain')
	return ctx.text(body)
}
