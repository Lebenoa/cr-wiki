//! The admin-authored wiki markup in prose fields, rendered to safe HTML.
//!
//!   [[12]] / [[cookie:12]] / [[pet:5]] / [[treasure:9]]
//!       a link to that entity, with its sprite ahead of the name. Ids are
//!       stable and language-independent; the name resolves per request.
//!   {color:red}text{/color}
//!       a coloured span, the value restricted to characters that cannot
//!       break out of the attribute.
//!
//! Everything else is escaped, so stray brackets or pasted markup render as
//! text. Ported from app/richtext.v.

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
        return rest.trim().parse::<i64>().ok().filter(|n| *n > 0).map(|n| (kind, n));
    }
    inner.trim().parse::<i64>().ok().filter(|n| *n > 0).map(|n| ("cookie", n))
}

fn section_of(kind: &str) -> &'static str {
    match kind {
        "pet" => "pets",
        "treasure" => "treasures",
        _ => "cookies",
    }
}

/// Renders one prose field. Names are resolved sequentially — the memo map
/// keeps a description linking the same entity three times to one query.
pub async fn render(db: &Db, lang: &str, raw: &str) -> String {
    // the overwhelming majority of fields carry no markup at all
    if !raw.contains("[[") && !raw.contains("{color:") {
        return escape(raw);
    }
    let mut cache: HashMap<String, Option<(String, Option<String>)>> = HashMap::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len() + 64);
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] == '[' && i + 1 < chars.len() && chars[i + 1] == '[' {
            if let Some(end) = find(&chars, i + 2, "]]") {
                let inner: String = chars[i + 2..end].iter().collect();
                if let Some((kind, id)) = split_ref(&inner) {
                    let key = format!("{kind}:{id}");
                    let hit = if cache.contains_key(&key) {
                        cache[&key].clone()
                    } else {
                        let v = db::entity_link(db, lang, kind, id).await;
                        cache.insert(key.clone(), v.clone());
                        v
                    };
                    if let Some((name, image)) = hit {
                        let dir = section_of(kind);
                        // the anchor stays a plain inline box: inline-flex
                        // would make it an atomic inline that cannot break
                        // across lines, so a long name in a narrow column
                        // would overflow instead of wrapping
                        out.push_str(&format!(
                            "<a href=\"/{dir}/{id}\" class=\"font-bold hover:text-primary transition-colors\">"
                        ));
                        if let Some(img) = image.filter(|s| !s.is_empty()) {
                            out.push_str(&format!(
                                "<img src=\"/img/{dir}/{}\" alt=\"\" loading=\"lazy\" class=\"inline-block size-5 mr-1 object-contain align-text-bottom\" />",
                                escape(&img)
                            ));
                        }
                        // the underline lives on the text, not the anchor: a
                        // decoration on the anchor is drawn across the sprite
                        out.push_str("<span class=\"underline decoration-primary decoration-2 underline-offset-4\">");
                        out.push_str(&escape(&name));
                        out.push_str("</span></a>");
                        i = end + 2;
                        continue;
                    }
                }
            }
        }
        if chars[i] == '{' && starts_at(&chars, i, "{color:") {
            if let Some(close) = find(&chars, i + 7, "}") {
                let value: String = chars[i + 7..close].iter().collect();
                if valid_color(&value) {
                    if let Some(end) = find(&chars, close + 1, "{/color}") {
                        let inner: String = chars[close + 1..end].iter().collect();
                        out.push_str(&format!("<span style=\"color:{value}\">"));
                        out.push_str(&escape(&inner));
                        out.push_str("</span>");
                        i = end + "{/color}".len();
                        continue;
                    }
                }
            }
        }
        out.push_str(&escape(&chars[i].to_string()));
        i += 1;
    }
    out
}

fn starts_at(chars: &[char], at: usize, needle: &str) -> bool {
    needle.chars().enumerate().all(|(k, c)| chars.get(at + k) == Some(&c))
}

fn find(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    (from..chars.len().saturating_sub(n.len() - 1)).find(|&i| chars[i..i + n.len()] == n[..])
}
