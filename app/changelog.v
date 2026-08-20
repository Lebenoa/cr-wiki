module app

import os
import veb

// The changelog is the git history, read once at startup rather than per
// request: it only changes when the binary is rebuilt anyway. `git log`'s
// default layout is parsed instead of a --pretty format string because a
// format carries `%` placeholders, and os.execute goes through cmd on
// Windows, which would expand `%h%` as a variable before git ever saw it.
const changelog_limit = 200

// Newest first, and empty when git is not on PATH or the binary runs outside
// a checkout — the page then says so rather than failing.
const changelog_entries = load_changelog()

// ChangeEntry is one commit as the page shows it: the conventional-commit
// type and scope split off the subject line so they can be badged, and the
// body as paragraphs with the trailers dropped.
pub struct ChangeEntry {
pub:
	hash    string
	short   string
	date    string
	kind    string // 'feat', 'fix', ... ; empty when the subject is unconventional
	scope   string // the (parenthesised) part, empty when there is none
	subject string
	body    []string
}

fn load_changelog() []ChangeEntry {
	res := os.execute('git log --no-color --date=short --max-count=${changelog_limit}')
	if res.exit_code != 0 {
		return []
	}
	mut out := []ChangeEntry{}
	mut hash := ''
	mut date := ''
	mut lines := []string{}
	for raw in res.output.split_into_lines() {
		if raw.starts_with('commit ') {
			if hash != '' {
				out << build_entry(hash, date, lines)
			}
			hash = raw[7..].trim_space()
			date = ''
			lines = []
			continue
		}
		if hash == '' {
			continue
		}
		if raw.starts_with('Date:') {
			date = raw[5..].trim_space()
			continue
		}
		// Author:, Merge: and any other header line is not shown
		if raw.len > 0 && !raw.starts_with('    ') {
			continue
		}
		lines << raw.trim_right(' \t')
	}
	if hash != '' {
		out << build_entry(hash, date, lines)
	}
	return out
}

// build_entry turns one commit's indented message block into an entry. The
// Co-Authored-By / Claude-Session trailers are dropped: they are provenance,
// not something a reader of the changelog wants.
fn build_entry(hash string, date string, lines []string) ChangeEntry {
	mut msg := []string{}
	for line in lines {
		text := if line.starts_with('    ') { line[4..] } else { line }
		if text.starts_with('Co-Authored-By:') || text.starts_with('Claude-Session:') {
			continue
		}
		msg << text
	}
	// leading/trailing blanks come from the blank line git puts after the
	// headers and from the dropped trailers
	for msg.len > 0 && msg[0].trim_space() == '' {
		msg.delete(0)
	}
	for msg.len > 0 && msg[msg.len - 1].trim_space() == '' {
		msg.delete(msg.len - 1)
	}
	subject := if msg.len > 0 { msg[0] } else { '' }
	kind, scope, title := split_subject(subject)
	mut body := []string{}
	mut para := []string{}
	for i := 1; i < msg.len; i++ {
		if msg[i].trim_space() == '' {
			if para.len > 0 {
				body << para.join(' ')
				para = []
			}
			continue
		}
		para << msg[i]
	}
	if para.len > 0 {
		body << para.join(' ')
	}
	return ChangeEntry{
		hash:    hash
		short:   if hash.len > 7 { hash[..7] } else { hash }
		date:    date
		kind:    kind
		scope:   scope
		subject: title
		body:    body
	}
}

// split_subject pulls "feat(theme): text" apart into its type, scope and the
// text. A subject in any other shape is returned whole, with no type — the
// page then shows it without a badge instead of mangling it.
fn split_subject(subject string) (string, string, string) {
	colon := subject.index(':') or { return '', '', subject }
	mut head := subject[..colon]
	rest := subject[colon + 1..].trim_space()
	if head == '' || rest == '' {
		return '', '', subject
	}
	mut scope := ''
	if open := head.index('(') {
		if head.ends_with(')') {
			scope = head[open + 1..head.len - 1]
			head = head[..open]
		}
	}
	if head == '' || !is_lower_word(head) {
		return '', '', subject
	}
	return head, scope, rest
}

// is_lower_word reports whether s is a bare lowercase word, which is what a
// conventional-commit type looks like. Anything else (a sentence, a path, a
// capitalised word) means the subject was not conventional after all.
fn is_lower_word(s string) bool {
	for c in s {
		if c < `a` || c > `z` {
			return false
		}
	}
	return s.len > 0
}

// changelog_kind_cls badges a commit type. Unknown types get the neutral
// pill, so a new type in the history still renders.
pub fn (ctx &Context) changelog_kind_cls(kind string) string {
	return match kind {
		'feat' { 'pill-accent' }
		'fix' { 'pill-primary' }
		else { 'text-[10px] font-bold uppercase text-foreground-muted border border-secondary/40 rounded-full px-2 py-0.5' }
	}
}

// changelog_page_size matches the catalog lists. The whole history with its
// bodies is a third of a megabyte in one response, so it scrolls in.
const changelog_page_size = 30

@['/changelog']
pub fn (mut wapp App) changelog_page(mut ctx Context) veb.Result {
	if !wapp.rate_limit_ok(mut ctx) {
		return rate_limited_response(mut ctx)
	}
	ctx.set_translate_title('changelog_page_title')
	ctx.set_translate_desc('changelog_page_description')
	mut page := (ctx.query['page'] or { '1' }).int()
	if page < 1 {
		page = 1
	}
	all := changelog_entries
	start := (page - 1) * changelog_page_size
	mut end := start + changelog_page_size
	if end > all.len {
		end = all.len
	}
	entries := if start < all.len { all[start..end] } else { []ChangeEntry{} }
	next_url := if end < all.len { '/changelog?page=${page + 1}' } else { '' }
	if ctx.is_htmx_request() && !ctx.is_boosted_request() {
		return $veb.html('./templates/components/changelog_entries.html')
	}
	return $veb.html('./templates/changelog.html')
}
