module util

import database.models
import encoding.html
import strings
import veb

// EffectView is one rendered treasure effect: the +0/+9 column values and the
// display text (numbers stripped / {value} handled). diff0/diff9 and name_html
// are the blessed-tab-only deltas vs the normal state. Pure presentation —
// lives in util, not the database module.
pub struct EffectView {
pub:
	effect_id int
	name      string // effect text (numbers stripped / {value} handled)
	value0    string // +0 column ("6%", "30"); empty when the effect carries no value
	value9    string // +9 column ("11%", "75")
	diff0     string // blessed tab only: delta vs the normal state ("+2%")
	diff9     string // blessed tab only: delta vs the normal state ("+10")
	name_html veb.RawHtml // blessed tab only: word diff vs the normal text
}

// fmt_f64 renders a value without a trailing .0 on whole numbers
// (5.0 -> "5", 0.3 -> "0.3", 7000.0 -> "7000").
pub fn fmt_f64(n f64) string {
	if n == f64(int(n)) {
		return '${int(n)}'
	}
	return '${n}'
}

// format_effect_value renders the numeric value with its unit suffix
// ("12%", "3s", "5000", "0.3-0.8s"); empty when the effect has no value.
pub fn format_effect_value(value ?f64, value_min ?f64, value_max ?f64, unit models.EffectUnit) string {
	suffix := match unit {
		.percent { '%' }
		.second { 's' }
		.flat { '' }
	}
	if mn := value_min {
		if mx := value_max {
			return '${fmt_f64(mn)}-${fmt_f64(mx)}${suffix}'
		}
		return '${fmt_f64(mn)}${suffix}'
	}
	if v := value {
		return '${fmt_f64(v)}${suffix}'
	}
	return ''
}

// format_effect_bare_value renders the bare number/range without a unit
// suffix ("5", "2-3") for substitution into {value} placeholders — the unit
// word/symbol lives in each language's translation text.
pub fn format_effect_bare_value(value ?f64, value_min ?f64, value_max ?f64) string {
	if mn := value_min {
		if mx := value_max {
			return '${fmt_f64(mn)}-${fmt_f64(mx)}'
		}
		return '${fmt_f64(mn)}'
	}
	if v := value {
		return '${fmt_f64(v)}'
	}
	return ''
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

// blessed_diffs pairs each blessed-state effect with the normal-state effect
// at the same position (the wiki lists both states in the same order) and
// fills the blessed rows' diff0/diff9 columns with the per-column delta. Rows
// without a normal counterpart (or with unparseable values) carry no diff.
// When the blessed text differs from the normal one (same effect, different
// wording), name_html carries a word-level diff of the change.
pub fn blessed_diffs(normal []EffectView, blessed []EffectView) []EffectView {
	mut out := []EffectView{}
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
		out << EffectView{
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

// effect_name_tokens returns the number-like tokens of an effect name:
// "get 5-15 extra points" -> ["5-15"], "6-11% higher" -> ["6-11%"],
// "get 1.5-3.3 extra seconds" -> ["1.5-3.3"]. Both '-' and '~' separate
// ranges. Scanning is byte-wise but only advances through ASCII digits and
// separators, so multi-byte runes (Thai) are never sliced mid-rune.
fn effect_name_tokens(s string) []string {
	mut out := []string{}
	mut i := 0
	for i < s.len {
		if !s[i].is_digit() {
			i++
			continue
		}
		mut j := i
		for j < s.len && (s[j].is_digit() || s[j] == `.`) {
			j++
		}
		// optional range separator then a second number
		if j < s.len && (s[j] == `-` || s[j] == `~`) && j + 1 < s.len && s[j + 1].is_digit() {
			j++
			for j < s.len && (s[j].is_digit() || s[j] == `.`) {
				j++
			}
		}
		// optional unit symbol
		if j < s.len && (s[j] == `%` || s[j] == `s`) {
			j++
		}
		out << s[i..j]
		i = j
	}
	return out
}

// split_token splits one token into low, high (empty for a single) and the
// unit symbol ("30-75%" -> 30, 75, "%"; "6" -> 6, "", "").
fn split_token(t string) (string, string, string) {
	mut i := 0
	for i < t.len && (t[i].is_digit() || t[i] == `.`) {
		i++
	}
	low := t[..i]
	mut rest := t[i..]
	mut high := ''
	if rest.len > 0 && (rest[0] == `-` || rest[0] == `~`) {
		mut j := 1
		for j < rest.len && (rest[j].is_digit() || rest[j] == `.`) {
			j++
		}
		high = rest[1..j]
		rest = rest[j..]
	}
	return low, high, rest
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

// strip_tokens removes the given tokens from s and collapses leftover
// whitespace ("get 5-15 extra points" -> "get extra points").
fn strip_tokens(s string, tokens []string) string {
	mut out := s
	for t in tokens {
		out = out.replace(t, ' ')
	}
	return collapse_spaces(out)
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

// split_structured_value renders the +0/+9 columns from the link's stored
// value (range -> endpoints, single -> repeated for both levels) and the
// display text with the {value} placeholder stripped when it reads cleanly.
fn split_structured_value(link models.TreasureEffect, name string) (string, string, string) {
	suffix := match link.unit {
		.percent { '%' }
		.second { 's' }
		.flat { '' }
	}
	mut v0 := ''
	mut v9 := ''
	mut bare := ''
	if mn := link.value_min {
		if mx := link.value_max {
			v0 = '${fmt_f64(mn)}${suffix}'
			v9 = '${fmt_f64(mx)}${suffix}'
			bare = '${fmt_f64(mn)}-${fmt_f64(mx)}'
		} else {
			v0 = '${fmt_f64(mn)}${suffix}'
			v9 = v0
			bare = '${fmt_f64(mn)}'
		}
	} else if v := link.value {
		v0 = '${fmt_f64(v)}${suffix}'
		v9 = v0
		bare = '${fmt_f64(v)}'
	}
	mut text := name
	if name.contains('{value}') {
		if bare == '' {
			// legacy placeholder with no structured value: never leak the token
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
	return v0, v9, text
}

// split_baked_value derives the +0/+9 columns from the number baked into the
// translation name (the wiki recorded the range/single inline): the first
// range token splits into its endpoints, a lone single token repeats for both
// levels. Unrelated extra tokens stay in the text untouched.
fn split_baked_value(name string) (string, string, string) {
	toks := effect_name_tokens(name)
	mut v0 := ''
	mut v9 := ''
	mut strip := []string{}
	for t in toks {
		low, high, suffix := split_token(t)
		if high != '' {
			v0 = low + suffix
			v9 = high + suffix
			strip = [t]
			break
		}
	}
	if v0 == '' && toks.len == 1 {
		low, _, suffix := split_token(toks[0])
		v0 = low + suffix
		v9 = v0
		strip = [toks[0]]
	}
	mut text := if strip.len > 0 { strip_tokens(name, strip) } else { name }
	// "increased by X-Y" phrases would dangle without their number — keep the
	// inline form then (the columns stay filled; mild redundancy is fine)
	if strip.len > 0 && ends_dangling(text) {
		text = name
	}
	return v0, v9, text
}

// split_effect_value derives the +0/+9 column values and the display text for
// one treasure-effect link: columns come from the link's structured value
// when present, else from the range/single number baked into the name.
pub fn split_effect_value(link models.TreasureEffect, name string) (string, string, string) {
	if link.value != none || link.value_min != none || link.value_max != none {
		return split_structured_value(link, name)
	}
	return split_baked_value(name)
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
