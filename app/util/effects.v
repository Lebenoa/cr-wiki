module util

import database
import encoding.html
import strings
import veb

// parse_value_cell splits one +0/+9 column value ("6%", "30", "1.5s") into
// its number and unit suffix; ok=false when the cell is empty or not a plain
// number ("???").
fn parse_value_cell(s string) (f64, string, bool) {
	if s == '' {
		return 0, '', false
	}
	mut num := s
	mut suffix := ''
	for u in ['%', 's'] {
		if s.ends_with(u) {
			num = s[..s.len - 1]
			suffix = u
			break
		}
	}
	if num.len == 0 {
		return 0, '', false
	}
	mut i := 0
	if num[0] == `-` {
		i = 1
	}
	if i >= num.len {
		return 0, '', false
	}
	mut dots := 0
	for i < num.len {
		if num[i] == `.` {
			dots++
			if dots > 1 {
				return 0, '', false
			}
		} else if !num[i].is_digit() {
			return 0, '', false
		}
		i++
	}
	return num.f64(), suffix, true
}

// value_delta renders the signed difference between a normal and a blessed
// column value ("6%" vs "8%" -> "+2%", "30" vs "40" -> "+10"); empty when
// either side is empty or non-numeric (no diff to show).
fn value_delta(normal string, blessed string) string {
	nn, nunit, nok := parse_value_cell(normal)
	bn, bunit, bok := parse_value_cell(blessed)
	if !nok || !bok {
		return ''
	}
	d := bn - nn
	// snap binary-float noise to 3 decimals: 1.2 - 1.0 computes as
	// 0.19999999999999996 and V prints the full expansion, so the rounded
	// value is formatted instead (the data carries at most 2 dp)
	mut d2 := d * 1000
	d2 = if d2 >= 0 {
		f64(int(d2 + 0.5))
	} else {
		f64(int(d2 - 0.5))
	}
	d2 /= 1000
	unit := if nunit != '' { nunit } else { bunit }
	mut txt := ''
	if d2 == f64(int(d2)) {
		txt = '${int(d2)}'
	} else {
		txt = '${d2}'
	}
	if d2 > 0 {
		txt = '+' + txt
	}
	return txt + unit
}

// blessed_diffs pairs each blessed-state effect with the normal-state effect
// at the same position (the wiki lists both states in the same order) and
// fills the blessed rows' diff0/diff9 columns with the per-column delta. Rows
// without a normal counterpart (or with unparseable values) carry no diff.
// When the blessed text differs from the normal one (same effect, different
// wording), name_html carries a word-level diff of the change.
pub fn blessed_diffs(normal []database.EffectView, blessed []database.EffectView) []database.EffectView {
	mut out := []database.EffectView{}
	for i, e in blessed {
		mut d0 := ''
		mut d9 := ''
		mut name_html := veb.RawHtml('')
		if i < normal.len {
			d0 = value_delta(normal[i].value0, e.value0)
			d9 = value_delta(normal[i].value9, e.value9)
			if normal[i].name != e.name {
				name_html = veb.RawHtml(word_diff(normal[i].name, e.name))
			}
		}
		out << database.EffectView{
			effect_id: e.effect_id
			name:      e.name
			value0:    e.value0
			value9:    e.value9
			diff0:     d0
			diff9:     d9
			name_html: name_html
		}
	}
	return out
}

// word_diff renders the word-level change between two effect texts as safe
// HTML: the common prefix/suffix stay plain, the normal-only words are struck
// through, and the blessed-only words are highlighted. Each word is escaped,
// so stored text can't inject markup.
fn word_diff(normal string, blessed string) string {
	nw := normal.split(' ')
	bw := blessed.split(' ')
	// longest common prefix
	mut p := 0
	for p < nw.len && p < bw.len && nw[p] == bw[p] {
		p++
	}
	// longest common suffix
	mut s := 0
	for s < nw.len - p && s < bw.len - p && nw[nw.len - 1 - s] == bw[bw.len - 1 - s] {
		s++
	}
	removed := nw[p .. nw.len - s]
	added := bw[p .. bw.len - s]
	mut b := strings.new_builder(normal.len + blessed.len + 64)
	for i in 0 .. p {
		if i > 0 {
			b.write_byte(` `)
		}
		b.write_string(html.escape(nw[i]))
	}
	if removed.len > 0 {
		b.write_string(' <del class="text-foreground-muted">')
		for i, w in removed {
			if i > 0 {
				b.write_byte(` `)
			}
			b.write_string(html.escape(w))
		}
		b.write_string('</del>')
	}
	if added.len > 0 {
		b.write_string(' <ins class="text-accent font-bold">')
		for i, w in added {
			if i > 0 {
				b.write_byte(` `)
			}
			b.write_string(html.escape(w))
		}
		b.write_string('</ins>')
	}
	for i in 0 .. s {
		b.write_byte(` `)
		b.write_string(html.escape(nw[nw.len - s + i]))
	}
	return b.str().trim_space()
}
