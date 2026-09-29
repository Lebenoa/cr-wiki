//! The admin-authored wiki markup in prose fields, rendered to safe HTML.
//!
//!   `[[12]]` / `[[cookie:12]]` / `[[pet:5]]` / `[[treasure:9]]`
//!       a link to that entity, with its sprite ahead of the name. Ids are
//!       stable and language-independent; the name resolves per request.
//!   `{color:red}text{/color}`
//!       a coloured span, the value restricted to characters that cannot
//!       break out of the attribute.
//!
//! Everything else is escaped, so stray brackets or pasted markup render as
//! text. Ported from `app/richtext.v`.

use std::collections::HashMap;

use crate::db::{self, Db};

/// HTML-escapes text. askama does this for `{{ }}`, but rich text is handed
/// to the template pre-rendered, so it escapes its own input.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// A colour value safe to drop into a style attribute: letters, digits and
/// the punctuation real CSS colours use. Quotes, semicolons and angle
/// brackets are rejected, so `{color:red" onclick="x}` renders literally.
fn valid_color(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 32
        && v.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '#' | '(' | ')' | ',' | '%' | '.' | '-' | ' ')
        })
}

/// "pet:5" -> ("pet", 5). A bare "12" is a cookie, mirroring the original
/// bare-name form. Anything else is not a link and renders as text.
fn split_ref(inner: &str) -> Option<(&'static str, i64)> {
    if let Some((prefix, rest)) = inner.split_once(':') {
        let kind = match prefix.trim().to_ascii_lowercase().as_str() {
            "cookie" => "cookie",
            "pet" => "pet",
            "treasure" => "treasure",
            _ => return None,
        };
        return rest
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|n| *n > 0)
            .map(|n| (kind, n));
    }
    inner
        .trim()
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .map(|n| ("cookie", n))
}

fn section_of(kind: &str) -> &'static str {
    match kind {
        "pet" => "pets",
        "treasure" => "treasures",
        _ => "cookies",
    }
}

/// A clamped char-window over the raw text: indexes are always in bounds,
/// so the scanner cannot panic on truncated markup.
fn slice(chars: &[char], from: usize, to: usize) -> String {
    let from = from.min(chars.len());
    let to = to.min(chars.len()).max(from);
    let window = chars.get(from..to).unwrap_or(&[]);
    window.iter().collect()
}

/// Resolved entity links keyed `kind:id`, shared across every prose field of
/// one page so the same entity is looked up once.
pub type LinkCache = HashMap<String, Option<(String, Option<String>)>>;

/// Renders one prose field through the caller's cache. Names are resolved
/// sequentially — the detail page shares one cache across its five fields,
/// so an entity named in abilities and description costs one query.
/// `self_ref` is the entity the prose belongs to: a link to it renders as a
/// bold highlight instead of an anchor, because a page linking to itself
/// reads as noise (and SEO-wise a self-link is a dead weight).
pub async fn render_with(
    db: &Db,
    lang: &str,
    raw: &str,
    cache: &mut LinkCache,
    self_ref: Option<(&'static str, i64)>,
) -> String {
    // the overwhelming majority of fields carry no markup at all
    if !raw.contains("[[") && !raw.contains("{color:") {
        return escape(raw);
    }
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len().saturating_add(64));
    let mut i = 0usize;

    while i < chars.len() {
        if chars.get(i) == Some(&'[') && chars.get(i.saturating_add(1)) == Some(&'[') {
            if let Some(end) = find(&chars, i.saturating_add(2), "]]") {
                let inner = slice(&chars, i.saturating_add(2), end);
                if let Some((kind, id)) = split_ref(&inner) {
                    let key = format!("{kind}:{id}");
                    let hit = if let Some(hit) = cache.get(&key) {
                        hit.clone()
                    } else {
                        let v = db::entity_link(db, lang, kind, id).await;
                        cache.insert(key, v.clone());
                        v
                    };
                    if let Some((name, image)) = hit {
                        // a reference to the page's own entity: bold, no
                        // anchor, no sprite — it is this page
                        if self_ref.is_some_and(|(k, n)| k == kind && n == id) {
                            out.push_str("<span class=\"font-bold text-primary\">");
                            out.push_str(&escape(&name));
                            out.push_str("</span>");
                            i = end.saturating_add(2);
                            continue;
                        }
                        let dir = section_of(kind);
                        // the anchor stays a plain inline box: inline-flex
                        // would make it an atomic inline that cannot break
                        // across lines, so a long name in a narrow column
                        // would overflow instead of wrapping
                        out.push_str("<a href=\"/");
                        out.push_str(dir);
                        out.push('/');
                        out.push_str(&id.to_string());
                        out.push_str(
                            "\" class=\"font-bold hover:text-primary transition-colors\">",
                        );
                        if let Some(img) = image.filter(|s| !s.is_empty()) {
                            out.push_str("<img src=\"/static/img/");
                            out.push_str(dir);
                            out.push('/');
                            out.push_str(&escape(&img));
                            out.push_str("\" alt=\"\" loading=\"lazy\" class=\"inline-block size-5 mr-1 object-contain align-text-bottom\" />");
                        }
                        // the underline lives on the text, not the anchor: a
                        // decoration on the anchor is drawn across the sprite
                        out.push_str("<span class=\"underline decoration-primary decoration-2 underline-offset-4\">");
                        out.push_str(&escape(&name));
                        out.push_str("</span></a>");
                        i = end.saturating_add(2);
                        continue;
                    }
                }
            }
        }
        if chars.get(i) == Some(&'{') && starts_at(&chars, i, "{color:") {
            if let Some(close) = find(&chars, i.saturating_add(7), "}") {
                let value = slice(&chars, i.saturating_add(7), close);
                if valid_color(&value) {
                    if let Some(end) = find(&chars, close.saturating_add(1), "{/color}") {
                        let inner = slice(&chars, close.saturating_add(1), end);
                        out.push_str("<span style=\"color:");
                        out.push_str(&value);
                        out.push_str("\">");
                        out.push_str(&escape(&inner));
                        out.push_str("</span>");
                        i = end.saturating_add("{/color}".chars().count());
                        continue;
                    }
                }
            }
        }
        // plain characters go straight into the output: routing each one
        // through `escape` would allocate a fresh String per character
        match chars.get(i).copied() {
            Some('&') => out.push_str("&amp;"),
            Some('<') => out.push_str("&lt;"),
            Some('>') => out.push_str("&gt;"),
            Some('"') => out.push_str("&quot;"),
            Some('\'') => out.push_str("&#39;"),
            Some(c) => out.push(c),
            None => {}
        }
        i = i.saturating_add(1);
    }
    out
}

fn starts_at(chars: &[char], at: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(k, c)| chars.get(at.saturating_add(k)) == Some(&c))
}

fn find(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    let len = n.len();
    (from..chars.len().saturating_sub(len.saturating_sub(1)))
        .find(|&i| chars.get(i..i.saturating_add(len)) == Some(&n[..]))
}
