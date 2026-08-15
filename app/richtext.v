module app

import db.sqlite
import database
import encoding.html
import strings
import veb

// render_rich_text converts the admin-authored wiki markup in prose fields
// (descriptions, power+ text) into safe HTML:
//
//	[[cookie:1]]          -> link to cookie #1 (ids are stable and language-
//	                        independent; the display name is resolved per
//	                        language at render time)
//	[[pet:1]]             -> link to pet #1
//	[[treasure:1]]        -> link to treasure #1 (a bare [[1]] defaults to
//	                        cookie, mirroring the old [[Name]] form)
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
		// link: [[id]] or [[kind:id]] (kind: cookie|pet|treasure)
		if s[i] == `[` && i + 1 < s.len && s[i + 1] == `[` {				if end := s.index_after(']]', i + 2) {
			kind, eid, ok := split_link_ref(s[i + 2 .. end])
			if ok {
				display := database.entity_name_by_id(conn, kind, eid, lang)
				if display != '' {
					href := match kind {
						'pet' { '/pets/${eid}' }
						'treasure' { '/treasures/${eid}' }
						else { '/cookies/${eid}' }
					}
					out.write_string('<a href="${href}" class="font-bold underline decoration-primary decoration-2 underline-offset-4 hover:text-primary transition-colors">')
					out.write_string(html.escape(display))
					out.write_string('</a>')
					i = end + 2
					continue
				}
			}
			// unresolvable ref: fall through to literal text
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

// split_link_ref splits a [[...]] inner text into its link kind and entity
// id: "pet:5" -> ('pet', 5, true). Text with no known kind prefix is a cookie
// link when it is purely numeric (the bare [[1]] form, mirroring the old bare
// [[Name]] cookie default). Everything else — names, unknown prefixes, empty
// or non-numeric refs — returns ok=false and renders literally, because
// authored markup must be language-independent and ids are the only stable
// key. The prefix is case-insensitive and the id is trimmed.
fn split_link_ref(inner string) (string, int, bool) {
	if colon := inner.index(':') {
		if colon > 0 {
			prefix := inner[..colon].trim_space().to_lower()
			rest := inner[colon + 1..].trim_space()
			if prefix in ['cookie', 'pet', 'treasure'] && all_digits(rest) {
				return prefix, rest.int(), true
			}
		}
	}
	trimmed := inner.trim_space()
	if all_digits(trimmed) {
		return 'cookie', trimmed.int(), true
	}
	return '', 0, false
}

// all_digits reports whether s is non-empty and consists only of digits — the
// id form of a rich-text link reference.
fn all_digits(s string) bool {
	if s.len == 0 {
		return false
	}
	for c in s {
		if !c.is_digit() {
			return false
		}
	}
	return true
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
