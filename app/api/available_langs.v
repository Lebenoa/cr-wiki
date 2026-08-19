module api

import os
import i18n

// The `.tr` files ship with the binary and veb verifies their keys at compile
// time, so a running process can never gain or lose a locale — scan the
// translations directory once at startup instead of on every call. This sits
// on the hot path: templates/layout/head.html emits an hreflang <link> per
// locale on every rendered page, before_request checks `?lang=` against it,
// and /sitemap.xml reads it per fetch.
const langs = scan_langs()

fn scan_langs() []string {
	files := os.walk_ext(i18n.default_translations_dir, '.tr')
	mut out := []string{}
	for file in files {
		filename := os.file_name(file)
		if filename == 'lang_map.tr' {
			continue
		}
		out << filename.all_before_last('.tr')
	}
	return out
}

// available_lang returns the locales the site is served in. Callers must treat
// the result as read-only: it is the shared startup scan, not a fresh copy.
@[inline]
pub fn available_lang() []string {
	return langs
}
