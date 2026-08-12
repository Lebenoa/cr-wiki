module app

import db.sqlite
import database
import encoding.html
import strings
import veb

// render_rich_text converts the admin-authored wiki markup in prose fields
// (descriptions, power+ text) into safe HTML:
//
//	[[Cookie Name]]       -> link to that cookie's page (name resolved per
//	                        language with en fallback; unresolvable names stay
//	                        literal)
//	{color:red}text{/color} -> <span style="color:...">text</span> (color value
//	                        is restricted to alphanumerics + '#')
//
// The whole input is HTML-escaped first, so everything else (including stray
// brackets or malicious markup) renders as plain text. Returns veb.RawHtml so
// the template emits it verbatim instead of re-escaping.
pub fn render_rich_text(conn sqlite.DB, lang string, raw string) veb.RawHtml {
	s := html.escape(raw)
	mut out := strings.new_builder(s.len + 64)
	mut i := 0
	for i < s.len {
		// link: [[Name]]
		if s[i] == `[` && i + 1 < s.len && s[i + 1] == `[` {				if end := s.index_after(']]', i + 2) {
				name := s[i + 2 .. end].trim_space()
				if name != '' {
					cid := database.cookie_id_by_name(conn, lang, html.unescape(name))
					if cid > 0 {
						out.write_string('<a href="/cookies/${cid}" class="font-bold underline decoration-primary decoration-2 underline-offset-4 hover:text-primary transition-colors">')
						out.write_string(name)
						out.write_string('</a>')
						i = end + 2
						continue
					}
				}
				// unresolvable/empty name: fall through to literal text
			}
		}
		// color span: {color:VAL}...{/color}
		if s[i] == `{` && s[i..].starts_with('{color:') {
			if close := s.index_after('}', i + 7) {
				val := s[i + 7 .. close]
				if valid_color(val) {
					if end := s.index_after('{/color}', close + 1) {
						out.write_string('<span style="color:${val}">')
						out.write_string(s[close + 1 .. end])
						out.write_string('</span>')
						i = end + '{/color}'.len
						continue
					}
				}
				// invalid color or missing close tag: fall through to literal
			}
		}
		out.write_u8(s[i])
		i++
	}
	return out.str()
}

// valid_color accepts CSS color values that are safe to embed in a style
// attribute: only letters, digits, and '#' (named colors, hex, rgb()/hsl()
// numbers), capped at 32 chars. Quotes, semicolons, and markup are rejected,
// so an injection like {color:red" onclick="...} renders literally.
fn valid_color(val string) bool {
	if val.len == 0 || val.len > 32 {
		return false
	}
	for c in val {
		if !(c.is_letter() || c.is_digit() || c == `#` || c == `(` || c == `)` || c == `,`
			|| c == `%` || c == `.` || c == `-` || c == ` `) {
			return false
		}
	}
	return true
}
