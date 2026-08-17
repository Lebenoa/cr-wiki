module util

import encoding.html
import strings
import veb

// EffectView is one rendered treasure effect: the +0/+9 level values and the
// display text (numbers stripped / {value} handled). `values` carries every
// level's value (index = level, '' where the level has none) so the detail
// page's level slider can switch without a round-trip. diff0/diff9/diffs and
// name_html are the blessed-tab-only deltas vs the normal state. Pure
// presentation — lives in util, not the database module.
pub struct EffectView {
pub:
	effect_id int
	name      string // effect text (numbers stripped / {value} handled)
	value0    string // level-0 value ("6%", "30"); empty when the effect carries no value
	value9    string // level-9 value ("11%", "75")
	values    []string // every level's value, index = level ('' where the level has none)
	diff0     string // blessed tab only: delta vs the normal state at level 0 ("+2%")
	diff9     string // blessed tab only: delta vs the normal state at level 9 ("+10")
	diffs     []string // blessed tab only: per-level deltas vs normal, index = level
	name_html veb.RawHtml // blessed tab only: word diff vs the normal text
}

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

// level_at returns an effect view's value at the given level, '' when the
// view carries no per-level values for it.
fn level_at(e EffectView, l int) string {
	if l >= 0 && l < e.values.len {
		return e.values[l]
	}
	return ''
}

// blessed_diffs pairs each blessed-state effect with the normal-state effect
// at the same position (the wiki lists both states in the same order) and
// fills the blessed rows' diff0/diff9/diffs with the per-level delta. Rows
// without a normal counterpart (or with unparseable values) carry no diff.
// When the blessed text differs from the normal one (same effect, different
// wording), name_html carries a word-level diff of the change.
pub fn blessed_diffs(normal []EffectView, blessed []EffectView) []EffectView {
	mut out := []EffectView{}
	for i, e in blessed {
		mut d0 := ''
		mut d9 := ''
		mut diffs := []string{}
		mut name_html := veb.RawHtml('')
		if i < normal.len {
			d0 = value_delta(normal[i].value0, e.value0)
			d9 = value_delta(normal[i].value9, e.value9)
			for l in 0 .. 10 {
				diffs << value_delta(level_at(normal[i], l), level_at(e, l))
			}
			if normal[i].name != e.name {
				name_html = veb.RawHtml(word_diff(normal[i].name, e.name))
			}
		}
		out << EffectView{
			effect_id: e.effect_id
			name:      e.name
			value0:    e.value0
			value9:    e.value9
			values:    e.values
			diff0:     d0
			diff9:     d9
			diffs:     diffs
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

// collapse_spaces trims s and collapses runs of whitespace into one space,
// byte-wise so multi-byte runes (Thai) survive untouched.
fn collapse_spaces(s string) string {
	mut b := strings.Builder{}
	mut prev_space := false
	for c in s {
		if c == ` ` {
			if !prev_space {
				b.write_byte(c)
			}
			prev_space = true
		} else {
			b.write_byte(c)
			prev_space = false
		}
	}
	return b.str().trim_space()
}

// strip_value_placeholder removes the {value} placeholder (and an attached
// unit symbol) from a translation name, collapsing the leftover whitespace.
fn strip_value_placeholder(s string) string {
	mut t := s.replace('{value}%', ' ')
	t = t.replace('{value}s', ' ')
	t = t.replace('{value}', ' ')
	return collapse_spaces(t)
}

// ends_dangling reports whether stripping the value from a {value} name left
// a dangling word ("Base speed increased by") — the substituted form reads
// better then.
fn ends_dangling(s string) bool {
	t := s.trim_space()
	if t == '' {
		return true
	}
	for w in [' by', ' with', ' for', ' to', ' of', ' from', ' an', ' a'] {
		if t.ends_with(w) {
			return true
		}
	}
	return false
}

// scan_value_num consumes one signed decimal number ("-2", "0.3", "5000")
// starting at i and returns its text plus the index just past it; the second
// return equals i when no number starts there.
fn scan_value_num(s string, i int) (string, int) {
	mut j := i
	if j < s.len && s[j] == `-` {
		j++
	}
	start := j
	mut dots := 0
	for j < s.len && (s[j].is_digit() || s[j] == `.`) {
		if s[j] == `.` {
			dots++
		}
		j++
	}
	if j == start || dots > 1 {
		return '', i
	}
	return s[start..j], j
}

// split_value_text parses a stored effect value string into its +0/+9
// columns and the bare number list: "6-11%" -> "6%", "11%", "6-11"; "12%"
// -> "12%", "12%", "12"; "-2--3%" -> "-2%", "-3%", "-2--3" (min -2, max -3);
// a multi-value effect ("1-2-3%") renders its first/last endpoints in the
// columns and keeps the whole list as the bare value; "" -> all empty. The
// unit suffix (%, s) rides on the string.
pub fn split_value_text(value string) (string, string, string) {
	if value == '' {
		return '', '', ''
	}
	mut s := value
	mut suffix := ''
	if s.ends_with('%') {
		suffix = '%'
		s = s[..s.len - 1]
	} else if s.ends_with('s') {
		suffix = 's'
		s = s[..s.len - 1]
	}
	// one or more signed decimal numbers joined by '-' ("12", "0.3-0.8",
	// "1-2-3"); anything else is not a stored value
	mut nums := []string{}
	mut i := 0
	for i < s.len {
		num, j := scan_value_num(s, i)
		if j == i {
			return '', '', ''
		}
		nums << num
		i = j
		if i < s.len {
			if s[i] != `-` {
				return '', '', ''
			}
			i++
			if i >= s.len {
				return '', '', ''
			}
		}
	}
	if nums.len == 1 {
		return nums[0] + suffix, nums[0] + suffix, nums[0]
	}
	return nums[0] + suffix, nums[nums.len - 1] + suffix, nums.join('-')
}

// split_effect_value renders one treasure effect from its +0/+9 column
// values (read from the treasure_level rows at levels 0 and 9) and the
// effect's localized name: the columns pass through unchanged, and a {value}
// placeholder in the name is substituted with the level-0 value's numbers
// ("15/25" -> "15", "0.3-0.8s" -> "0.3-0.8") so the text reads inline; when
// the substituted form would dangle ("Base speed increased by") the
// placeholder is stripped instead and the columns carry the numbers.
pub fn split_effect_value(value0 string, value9 string, name string) (string, string, string) {
	mut text := name
	if name.contains('{value}') {
		_, _, bare := split_value_text(value0)
		if bare == '' {
			// placeholder with no structured value at level 0: never leak the token
			text = strip_value_placeholder(name)
		} else if bare.contains('-') {
			// a range reads naturally inline; keep the substituted form
			text = name.replace('{value}', bare)
		} else {
			stripped := strip_value_placeholder(name)
			if !ends_dangling(stripped) {
				text = stripped
			} else {
				text = name.replace('{value}', bare)
			}
		}
	}
	return value0, value9, text
}

// compact_effect_value joins the +0/+9 column values into one picker line:
// empty when the effect has no value, the single value when both levels match
// ("12%"), or a range with the shared unit hoisted ("6-11%", "0.3-0.8s").
pub fn compact_effect_value(v0 string, v9 string) string {
	if v0 == '' {
		return ''
	}
	if v0 == v9 {
		return v0
	}
	if v0.len > 1 && v9.len > 1 && v0[v0.len - 1] == v9[v9.len - 1] && (v0[v0.len - 1] == `%` || v0[v0.len - 1] == `s`) {
		return v0[..v0.len - 1] + '-' + v9
	}
	return v0 + '-' + v9
}
