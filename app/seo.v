module app

import api
import database
import veb

// sitemap serves /sitemap.xml: static pages plus every detail URL, each
// declaring its locale alternates so both languages get indexed.
// Query-filtered build URLs are excluded on purpose — crawlers
// should reach builds through /builds.
@['/sitemap.xml']
pub fn (wapp &App) sitemap(mut ctx Context) veb.Result {
	base := ctx.site_url()
	mut xml := '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">\n'
	// every list page the navbar exposes; a section missing here is one
	// crawlers only reach by luck
	mut paths := ['/', '/cookies', '/pets', '/treasures', '/builds', '/episodes',
		'/ingredients', '/jellies', '/skins', '/relics', '/gacha']
	entries := database.sitemap_entries(wapp.db) or { [] }
	for e in entries {
		paths << '/${e.section}/${e.id}'
	}
	langs := api.available_lang()
	for path in paths {
		xml += sitemap_url(base, path, langs)
	}
	xml += '</urlset>\n'
	ctx.set_content_type('application/xml')
	return ctx.text(xml)
}

// sitemap_url renders one <url> entry: the default-locale address plus an
// xhtml:link per locale (including itself, which the spec requires), so a
// crawler that finds any one of them learns about the rest.
fn sitemap_url(base string, path string, langs []string) string {
	mut out := '<url><loc>${base}${path}</loc>'
	for lang in langs {
		href := if lang == default_lang { '${base}${path}' } else { '${base}${path}?lang=${lang}' }
		out += '<xhtml:link rel="alternate" hreflang="${lang}" href="${href}"/>'
	}
	out += '<xhtml:link rel="alternate" hreflang="x-default" href="${base}${path}"/></url>\n'
	return out
}

// robots_txt serves /robots.txt: crawl everything except form/auth pages,
// and points crawlers at the sitemap.
@['/robots.txt']
pub fn (wapp &App) robots_txt(mut ctx Context) veb.Result {
	// /api and /builds/preview return fragments, not standalone pages
	body := 'User-agent: *\nDisallow: /new\nDisallow: /login\nDisallow: /register\nDisallow: /search\nDisallow: /*/edit
Disallow: /api/
Disallow: /builds/preview\n\nSitemap: ${ctx.site_url()}/sitemap.xml\n'
	ctx.set_content_type('text/plain')
	return ctx.text(body)
}
