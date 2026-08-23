//! The git history, read once at startup.
//!
//! Same reasoning as app/changelog.v: `git log`'s default layout is parsed
//! rather than a --pretty format, because a format carries % placeholders and
//! the V version shelled through cmd on Windows, which would expand `%h%` as
//! a variable. Rust's Command execs directly, so the hazard is gone, but the
//! parser is kept identical so both ports agree on what a commit looks like.

use std::process::Command;
use std::sync::OnceLock;

pub const LIMIT: usize = 200;

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
    ENTRIES.get_or_init(load)
}

fn load() -> Vec<ChangeEntry> {
    let out = Command::new("git")
        .args(["log", "--no-color", "--date=short", &format!("--max-count={LIMIT}")])
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o.stdout,
        // no git on PATH, or not a checkout: the page says so rather than failing
        _ => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&out);

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
    let rest = rest[1..].trim();
    if head.is_empty() || rest.is_empty() {
        return (String::new(), String::new(), subject.to_string());
    }
    let mut scope = String::new();
    if let Some(open) = head.find('(') {
        if head.ends_with(')') {
            scope = head[open + 1..head.len() - 1].to_string();
            head = &head[..open];
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
