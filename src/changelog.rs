//! The git history, captured by build.rs at compile time.
//!
//! Same reasoning as app/changelog.v: `git log`'s default layout is parsed
//! rather than a --pretty format, because a format carries % placeholders and
//! the V version shelled through cmd on Windows, which would expand `%h%` as
//! a variable. Rust's Command execs directly, so the hazard is gone, but the
//! parser is kept identical so both ports agree on what a commit looks like.
//! build.rs runs the capture and drops the raw text into the build output, so a
//! release tarball needs no `.git` directory — and no git at all — at runtime.

use std::sync::OnceLock;

/// Raw `git log` output baked in by build.rs (which caps the capture, keeping
/// its own `--max-count` in sync with what this page paginates well); empty
/// when git was unavailable or the build ran outside a checkout, and the page
/// then shows its empty state rather than failing.
const LOG_TEXT: &str = include_str!(concat!(env!("OUT_DIR"), "/changelog.txt"));

#[derive(Debug, Clone)]
pub struct ChangeEntry {
    /// kept for parity with the V struct; the page shows `short`
    #[allow(dead_code)]
    pub hash: String,
    pub short: String,
    pub date: String,
    pub kind: String,
    pub scope: String,
    pub subject: String,
    pub body: Vec<String>,
}

static ENTRIES: OnceLock<Vec<ChangeEntry>> = OnceLock::new();

pub fn entries() -> &'static Vec<ChangeEntry> {
    ENTRIES.get_or_init(|| parse(LOG_TEXT))
}

fn parse(text: &str) -> Vec<ChangeEntry> {
    let mut entries = Vec::new();
    let mut hash = String::new();
    let mut date = String::new();
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        if let Some(rest) = raw.strip_prefix("commit ") {
            if !hash.is_empty() {
                entries.push(build_entry(&hash, &date, &lines));
            }
            hash = rest.trim().to_string();
            date.clear();
            lines.clear();
            continue;
        }
        if hash.is_empty() {
            continue;
        }
        if let Some(rest) = raw.strip_prefix("Date:") {
            date = rest.trim().to_string();
            continue;
        }
        // Author:, Merge: and any other header line is not shown
        if !raw.is_empty() && !raw.starts_with("    ") {
            continue;
        }
        lines.push(raw.trim_end().to_string());
    }
    if !hash.is_empty() {
        entries.push(build_entry(&hash, &date, &lines));
    }
    entries
}

fn build_entry(hash: &str, date: &str, lines: &[String]) -> ChangeEntry {
    let mut msg: Vec<&str> = Vec::new();
    for line in lines {
        let text = line.strip_prefix("    ").unwrap_or(line);
        if text.starts_with("Co-Authored-By:") || text.starts_with("Claude-Session:") {
            continue;
        }
        msg.push(text);
    }
    while msg.first().is_some_and(|l| l.trim().is_empty()) {
        msg.remove(0);
    }
    while msg.last().is_some_and(|l| l.trim().is_empty()) {
        msg.pop();
    }

    let subject = msg.first().copied().unwrap_or("");
    let (kind, scope, title) = split_subject(subject);

    let mut body = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    for line in msg.iter().skip(1) {
        if line.trim().is_empty() {
            if !para.is_empty() {
                body.push(para.join(" "));
                para.clear();
            }
            continue;
        }
        para.push(line);
    }
    if !para.is_empty() {
        body.push(para.join(" "));
    }

    ChangeEntry {
        hash: hash.to_string(),
        short: hash.chars().take(7).collect(),
        date: date.to_string(),
        kind,
        scope,
        subject: title,
        body,
    }
}

/// "feat(theme): text" -> ("feat", "theme", "text"). Any other shape comes
/// back whole with no type, so the page renders it without a badge instead of
/// mangling it.
fn split_subject(subject: &str) -> (String, String, String) {
    let Some(colon) = subject.find(':') else {
        return (String::new(), String::new(), subject.to_string());
    };
    let (mut head, rest) = subject.split_at(colon);
    let Some(rest) = rest.get(1..) else {
        return (String::new(), String::new(), subject.to_string());
    };
    let rest = rest.trim();
    if head.is_empty() || rest.is_empty() {
        return (String::new(), String::new(), subject.to_string());
    }
    let mut scope = String::new();
    if let Some(open) = head.find('(') {
        if head.ends_with(')') {
            // the parens are ASCII, so both cuts are char boundaries by
            // construction; the subslices cannot be None
            if let (Some(inner), Some(before)) =
                (head.get(open.saturating_add(1)..), head.get(..open))
            {
                let inner = inner.strip_suffix(')').unwrap_or(inner);
                scope = inner.to_string();
                head = before;
            }
        }
    }
    if head.is_empty() || !head.bytes().all(|c| c.is_ascii_lowercase()) {
        return (String::new(), String::new(), subject.to_string());
    }
    (head.to_string(), scope, rest.to_string())
}

/// Badge class for a commit type; unknown types get the neutral pill so a new
/// type in the history still renders.
pub fn kind_class(kind: &str) -> &'static str {
    match kind {
        "feat" => "pill-accent",
        "fix" => "pill-primary",
        _ => "text-[10px] font-bold uppercase text-foreground-muted border border-secondary/40 rounded-full px-2 py-0.5",
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// git log parses into conventional-commit parts, and the trailers the V
    /// version drops are dropped here too.
    #[test]
    fn changelog_parses_history() {
        let entries = entries();
        assert!(!entries.is_empty(), "no history parsed");
        assert!(entries.iter().any(|e| e.kind == "feat"));
        assert!(entries.iter().any(|e| e.kind == "fix"));
        for e in entries {
            for para in &e.body {
                assert!(!para.starts_with("Co-Authored-By:"));
                assert!(!para.starts_with("Claude-Session:"));
            }
        }
    }

    /// The capture build.rs bakes in — not the live repo — is what parses, so
    /// an empty capture (no git on the build machine) means an empty page.
    #[test]
    fn empty_capture_yields_no_entries() {
        assert!(parse("").is_empty());
        assert!(parse("fatal: not a git repository").is_empty());
    }

    #[test]
    fn parse_splits_subject_and_body() {
        // concat!, not \-continuations: those strip the leading indentation
        // the parser keys on
        let text = concat!(
            "commit abc1234def5678\n",
            "Author: someone\n",
            "Date:   2026-09-29\n",
            "\n",
            "    feat(seed): add cookies\n",
            "\n",
            "    First paragraph\n",
            "    continues here.\n",
            "\n",
            "    Second paragraph.\n",
            "\n",
            "    Co-Authored-By: bot <bot@example.com>\n",
            "commit 0000000\n",
            "Date:   2026-09-28\n",
            "\n",
            "    subject without a colon\n",
        );
        let entries = parse(text);
        assert_eq!(entries.len(), 2);

        let first = &entries[0];
        assert_eq!(first.short, "abc1234");
        assert_eq!(first.date, "2026-09-29");
        assert_eq!(first.kind, "feat");
        assert_eq!(first.scope, "seed");
        assert_eq!(first.subject, "add cookies");
        assert_eq!(first.body, vec!["First paragraph continues here.", "Second paragraph."]);

        let second = &entries[1];
        assert_eq!(second.kind, "");
        assert_eq!(second.subject, "subject without a colon");
        assert!(second.body.is_empty());
    }
}
