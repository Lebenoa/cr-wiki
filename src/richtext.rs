//! The admin-authored wiki markup in prose fields, rendered to safe HTML.
//!
//!   `[[cookie:12]]` / `[[pet:5]]` / `[[treasure:9]]`
//!       a link to that entity, with its sprite ahead of the name. Ids are
//!       stable and language-independent; the name resolves per request.
//!   `![alt](https://…)`
//!       an image; http(s) sources only, both parts required.
//!   `[text](https://…)`
//!       a link; http(s) targets only, both parts required, and the label
//!       renders its own inline markup (entity links, bold, colour).
//!   `{color:red}text{/color}`
//!       a coloured span, the value restricted to characters that cannot
//!       break out of the attribute.
//!
//! Everything else is escaped, so stray brackets or pasted markup render as
//! text. Ported from `app/richtext.v`.

use std::collections::HashMap;
use std::future::Future;

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

/// "cookie:12" / "pet:5" / "treasure:9" -> (kind, id). Anything else — an
/// untyped id included — is not a link and renders as text.
fn split_ref(inner: &str) -> Option<(&'static str, i64)> {
    let (prefix, rest) = inner.split_once(':')?;
    let kind = match prefix.trim().to_ascii_lowercase().as_str() {
        "cookie" => "cookie",
        "pet" => "pet",
        "treasure" => "treasure",
        _ => return None,
    };
    rest.trim()
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .map(|n| (kind, n))
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

/// Renders one prose field's INLINE markup through the caller's cache. Names
/// are resolved sequentially — the detail page shares one cache across its
/// five fields, so an entity named in abilities and description costs one
/// query. `self_ref` is the entity the prose belongs to: a link to it renders
/// as a bold highlight instead of an anchor, because a page linking to itself
/// reads as noise (and SEO-wise a self-link is a dead weight).
pub async fn render_with(
    db: &Db,
    lang: &str,
    raw: &str,
    cache: &mut LinkCache,
    self_ref: Option<(&'static str, i64)>,
) -> String {
    render_inline(db, lang, raw, cache, self_ref).await
}

/// The scanner behind [`render_with`]. Private so the article renderer below
/// reuses it per block without exposing a second inline entry point.
async fn render_inline(
    db: &Db,
    lang: &str,
    raw: &str,
    cache: &mut LinkCache,
    self_ref: Option<(&'static str, i64)>,
) -> String {
    render_inline_inner(db, lang, raw, cache, self_ref).await
}

/// The body of [`render_inline`], boxed so the emphasis recursion (bold
/// containing italic containing…, each at most a marker's width deeper)
/// compiles: a recursive async fn has an infinitely sized future otherwise.
fn render_inline_inner<'a>(
    db: &'a Db,
    lang: &'a str,
    raw: &'a str,
    cache: &'a mut LinkCache,
    self_ref: Option<(&'static str, i64)>,
) -> std::pin::Pin<Box<dyn Future<Output = String> + Send + 'a>> {
    Box::pin(async move { render_inline_body(db, lang, raw, cache, self_ref).await })
}

async fn render_inline_body(
    db: &Db,
    lang: &str,
    raw: &str,
    cache: &mut LinkCache,
    self_ref: Option<(&'static str, i64)>,
) -> String {
    // the overwhelming majority of fields carry no markup at all
    if !raw.contains("[[")
        && !raw.contains("{color:")
        && !raw.contains('*')
        && !raw.contains("![")
        && !raw.contains("](")
    {
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
        if chars.get(i) == Some(&'!') && chars.get(i.saturating_add(1)) == Some(&'[') {
            if let Some((close, src, alt)) = image_at(&chars, i) {
                out.push_str("<img src=\"");
                out.push_str(&escape(&src));
                out.push_str("\" alt=\"");
                out.push_str(&escape(&alt));
                out.push_str(
                    "\" loading=\"lazy\" class=\"mt-4 rounded-2xl border border-border\" />",
                );
                i = close;
                continue;
            }
        }
        if chars.get(i) == Some(&'[') {
            // a plain link never starts `[[` (that is the entity form, tried
            // above and fallen through when it does not resolve); the label
            // renders recursively, so entity links, bold and colour nest in it
            if let Some((close, href, label)) = link_at(&chars, i) {
                out.push_str("<a href=\"");
                out.push_str(&escape(&href));
                out.push_str(
                    "\" class=\"text-primary underline decoration-primary decoration-2 underline-offset-4 hover:text-accent transition-colors\" target=\"_blank\" rel=\"noopener noreferrer\">",
                );
                out.push_str(&render_inline(db, lang, &label, cache, self_ref).await);
                out.push_str("</a>");
                i = close;
                continue;
            }
        }
        if chars.get(i) == Some(&'*') {
            // **strong**: scanned before *em* so the longer marker wins
            if chars.get(i.saturating_add(1)) == Some(&'*') {
                if let Some(end) = find(&chars, i.saturating_add(2), "**") {
                    let inner = slice(&chars, i.saturating_add(2), end);
                    if !inner.is_empty() {
                        out.push_str("<strong>");
                        out.push_str(&render_inline(db, lang, &inner, cache, self_ref).await);
                        out.push_str("</strong>");
                        i = end.saturating_add(2);
                        continue;
                    }
                }
            } else {
                // *em*: the closing marker must not be another '*' (that is
                // the start of a ** pair, not this run's end)
                if let Some(end) = find(&chars, i.saturating_add(1), "*") {
                    let inner = slice(&chars, i.saturating_add(1), end);
                    if !inner.is_empty()
                        && !inner.starts_with('*')
                        && chars.get(end.saturating_add(1)) != Some(&'*')
                    {
                        out.push_str("<em>");
                        out.push_str(&render_inline(db, lang, &inner, cache, self_ref).await);
                        out.push_str("</em>");
                        i = end.saturating_add(1);
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

/// One block of an article body, before inline rendering. News prose is the
/// one place the wiki markup gets structure: the catalog's short fields are
/// single paragraphs, while a news post wants headings, lists and paragraphs
/// the author can write without touching HTML.
enum Block {
    /// the heading's tag number (2..=4; h1 is the article title itself)
    Heading(u8, String),
    Bullets(Vec<String>),
    Numbers(Vec<String>),
    Paragraph(String),
}

/// Renders a news article body: blank-line-separated blocks, each a heading,
/// a bullet/numbered list or a paragraph, with the same inline markup as
/// every other prose field. Everything is escaped exactly like
/// [`render_with`], so an article cannot inject HTML any more than a
/// description can.
pub async fn render_article_with(db: &Db, lang: &str, raw: &str, cache: &mut LinkCache) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(raw.len().saturating_add(64));
    for block in split_blocks(raw) {
        match block {
            Block::Heading(level, text) => {
                let class = if level <= 2 {
                    "mt-8 mb-3 font-headline text-2xl text-accent"
                } else {
                    "mt-6 mb-2 font-headline text-xl text-accent"
                };
                out.push_str("<h");
                out.push_str(&level.to_string());
                out.push_str(" class=\"");
                out.push_str(class);
                out.push_str("\">");
                out.push_str(&render_inline(db, lang, &text, cache, None).await);
                out.push_str("</h");
                out.push_str(&level.to_string());
                out.push('>');
            }
            Block::Bullets(items) => {
                out.push_str(&render_items(db, lang, &items, cache, "ul", "list-disc").await);
            }
            Block::Numbers(items) => {
                out.push_str(&render_items(db, lang, &items, cache, "ol", "list-decimal").await);
            }
            Block::Paragraph(text) => {
                // single newlines inside a paragraph arrive as real \n (the
                // block splitter keeps them); the inline renderer passes
                // them through, so they must become breaks here — a <br>
                // wraps tight, nothing like a paragraph's own mt-4 gap
                let html = render_inline(db, lang, &text, cache, None).await;
                out.push_str("<p class=\"mt-4 leading-relaxed text-foreground\">");
                out.push_str(&html.replace('\n', "<br />"));
                out.push_str("</p>");
            }
        }
    }
    out
}

/// One list, its items inline-rendered. `tag` is `ul`/`ol` and `marker` the
/// matching list-style utility.
async fn render_items(
    db: &Db,
    lang: &str,
    items: &[String],
    cache: &mut LinkCache,
    tag: &str,
    marker: &str,
) -> String {
    let mut out = String::new();
    out.push('<');
    out.push_str(tag);
    out.push_str(" class=\"mt-4 ");
    out.push_str(marker);
    out.push_str(" list-inside space-y-1 text-foreground\">");
    for item in items {
        out.push_str("<li>");
        out.push_str(&render_inline(db, lang, item, cache, None).await);
        out.push_str("</li>");
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    out
}

/// Splits an article body into blocks on blank lines. Consecutive non-blank
/// lines are one block; within it the first line's shape decides: a single
/// `#`/`##`/`###` line is a heading, a run of `- `/`* ` lines a bullet list,
/// a run of `1. ` lines a numbered one, anything else a paragraph. A
/// paragraph keeps its newlines (the renderer turns them into `<br>`), so a
/// hard wrap in the textarea is a narrow line break on the page while a
/// blank line still means a full paragraph.
fn split_blocks(raw: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut chunk: Vec<String> = Vec::new();
    for line in raw.replace("\r\n", "\n").lines() {
        if line.trim().is_empty() {
            flush_block(&mut chunk, &mut blocks);
        } else {
            chunk.push(line.trim_end().to_string());
        }
    }
    flush_block(&mut chunk, &mut blocks);
    blocks
}

fn flush_block(chunk: &mut Vec<String>, blocks: &mut Vec<Block>) {
    if chunk.is_empty() {
        return;
    }
    if chunk.len() == 1 {
        if let Some(heading) = heading_of(&chunk[0]) {
            blocks.push(heading);
            chunk.clear();
            return;
        }
    }
    if chunk.iter().all(|l| bullet_of(l).is_some()) {
        blocks.push(Block::Bullets(
            chunk.iter().filter_map(|l| bullet_of(l)).collect(),
        ));
    } else if chunk.iter().all(|l| number_of(l).is_some()) {
        blocks.push(Block::Numbers(
            chunk.iter().filter_map(|l| number_of(l)).collect(),
        ));
    } else {
        blocks.push(Block::Paragraph(chunk.join("\n")));
    }
    chunk.clear();
}

fn heading_of(line: &str) -> Option<Block> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let text = line.get(hashes..)?.strip_prefix(' ')?.trim();
    if text.is_empty() {
        return None;
    }
    // # -> h2, ## -> h3, ### -> h4: h1 is the article's own title
    Some(Block::Heading(
        u8::try_from(hashes).ok()?.saturating_add(1),
        text.to_string(),
    ))
}

fn bullet_of(line: &str) -> Option<String> {
    let t = line.trim_start();
    ["- ", "* "]
        .iter()
        .find_map(|m| t.strip_prefix(m).map(|rest| rest.trim().to_string()))
}

fn number_of(line: &str) -> Option<String> {
    let t = line.trim_start();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = t.get(digits..)?.strip_prefix(". ")?;
    Some(rest.trim().to_string())
}

fn starts_at(chars: &[char], at: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(k, c)| chars.get(at.saturating_add(k)) == Some(&c))
}

/// An image `![alt](src)` at `at`: returns the index just past the markup,
/// the source URL and the alt text. http(s) only — the value renders as a
/// live `src` and HTML escaping does not neutralise a `javascript:` URL that
/// carries no markup characters; no control bytes, both parts non-empty.
fn image_at(chars: &[char], at: usize) -> Option<(usize, String, String)> {
    let alt_end = find(chars, at.saturating_add(2), "]")?;
    if chars.get(alt_end.saturating_add(1)) != Some(&'(') {
        return None;
    }
    let close = find(chars, alt_end.saturating_add(2), ")")?;
    let alt = slice(chars, at.saturating_add(2), alt_end);
    let src = slice(chars, alt_end.saturating_add(2), close);
    if alt.is_empty() || src.is_empty() {
        return None;
    }
    let lower = src.to_ascii_lowercase();
    let scheme = if lower.starts_with("https://") {
        8
    } else if lower.starts_with("http://") {
        7
    } else {
        return None;
    };
    // "https://" alone has no host: nothing to load
    if src.len() <= scheme || src.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    if alt.len() > 300 || alt.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    Some((close.saturating_add(1), src, alt))
}

/// A plain link, `[text](https://...)`: the image form without the leading
/// `!`. Guards mirror the build form's video-link validation: http(s)
/// targets only (HTML escaping does not neutralise a `javascript:` href, so
/// the scheme check is the whole defense), ≤200 chars, no control bytes,
/// and a non-empty label. Returns the index past the closing `)`, the
/// target, and the label.
fn link_at(chars: &[char], at: usize) -> Option<(usize, String, String)> {
    let label_end = find(chars, at.saturating_add(1), "]")?;
    if chars.get(label_end.saturating_add(1)) != Some(&'(') {
        return None;
    }
    let close = find(chars, label_end.saturating_add(2), ")")?;
    let label = slice(chars, at.saturating_add(1), label_end);
    let href = slice(chars, label_end.saturating_add(2), close);
    if label.is_empty() || href.is_empty() {
        return None;
    }
    let lower = href.to_ascii_lowercase();
    let scheme = if lower.starts_with("https://") {
        8
    } else if lower.starts_with("http://") {
        7
    } else {
        return None;
    };
    // "https://" alone has no host: nowhere to link
    if href.len() > 200 || href.len() <= scheme || href.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    Some((close.saturating_add(1), href, label))
}

fn find(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    let len = n.len();
    (from..chars.len().saturating_sub(len.saturating_sub(1)))
        .find(|&i| chars.get(i..i.saturating_add(len)) == Some(&n[..]))
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    fn shape(b: &Block) -> &'static str {
        match b {
            Block::Heading(..) => "h",
            Block::Bullets(_) => "ul",
            Block::Numbers(_) => "ol",
            Block::Paragraph(_) => "p",
        }
    }

    /// Paragraphs, headings and both list shapes split on blank lines, and a
    /// multi-line paragraph stays one block that keeps its newlines.
    #[test]
    fn blocks_split_on_blank_lines() {
        let raw = "# Title\n\nFirst line\nsecond line\n\n- a\n- b\n\n1. one\n2. two\n\nTail";
        let blocks = split_blocks(raw);
        assert_eq!(
            blocks.iter().map(shape).collect::<Vec<_>>(),
            vec!["h", "p", "ul", "ol", "p"]
        );
        assert!(matches!(&blocks[0], Block::Heading(2, t) if t == "Title"));
        assert!(matches!(
            &blocks[1],
            Block::Paragraph(t) if t == "First line\nsecond line"
        ));
        assert!(matches!(&blocks[2], Block::Bullets(v) if v == &["a", "b"]));
        assert!(matches!(&blocks[3], Block::Numbers(v) if v == &["one", "two"]));
    }

    /// The escaper and the colour guard are the article renderer's whole
    /// security surface: a pasted tag is text, and a colour value cannot
    /// break out of the attribute.
    #[test]
    fn escaping_and_colour_guard_hold() {
        assert_eq!(
            escape("<img src=x onerror=1>"),
            "&lt;img src=x onerror=1&gt;"
        );
        assert_eq!(
            escape("a & b 'c' \"d\""),
            "a &amp; b &#39;c&#39; &quot;d&quot;"
        );
        assert!(valid_color("#f00"));
        assert!(valid_color("rgb(1, 2, 3)"));
        assert!(!valid_color("red\" onclick=\"x"));
        assert!(!valid_color("red;background:url(x)"));
        assert!(!valid_color(&"a".repeat(33)));
    }

    /// A hashtag-ish line that is not a heading marker stays body text, and
    /// an empty body yields no blocks (so the renderer returns nothing).
    #[test]
    fn near_misses_stay_paragraphs() {
        assert!(split_blocks("   \n\n").is_empty());
        assert!(
            matches!(split_blocks("#nospace").as_slice(), [Block::Paragraph(t)] if t == "#nospace")
        );
        assert!(
            matches!(split_blocks("#### four").as_slice(), [Block::Paragraph(t)] if t == "#### four")
        );
        // a bullet run interrupted by a plain line is one paragraph
        assert!(matches!(
            split_blocks("- a\nplain").as_slice(),
            [Block::Paragraph(_)]
        ));
    }

    /// The emphasis renderer's db handle: entity resolution inside emphasis
    /// needs one, and the same env-gated scratch server the news tests use
    /// serves. None means the tests skip with a notice, the way every
    /// server-backed suite behaves without `CR_SURREAL_URL`.
    async fn scratch() -> Option<crate::db::Db> {
        let url = std::env::var("CR_SURREAL_URL").ok()?;
        let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
        match crate::db::connect_url(
            &url,
            "cookierun_richtext",
            "cookierun_richtext",
            &user,
            &pass,
        )
        .await
        {
            Ok(db) => Some(db),
            Err(e) => {
                eprintln!("skip: CR_SURREAL_URL is set but unusable ({e})");
                None
            }
        }
    }

    /// `**bold**` and `*italic*` render as emphasis and nest; multiplication,
    /// unmatched markers and empty runs stay literal, and pasted markup
    /// inside an emphasis is still escaped.
    #[tokio::test]
    async fn emphasis_renders_and_does_not_leak() {
        let Some(db) = scratch().await else {
            eprintln!("skip: set CR_SURREAL_URL to run the emphasis tests");
            return;
        };
        let db = &db;
        let mut cache = LinkCache::new();
        let cases: &[(&str, &str)] = &[
            (
                "**bold** and *slanted*",
                "<strong>bold</strong> and <em>slanted</em>",
            ),
            // nested: bold containing italic
            (
                "**big *small* end**",
                "<strong>big <em>small</em> end</strong>",
            ),
            // multiplication and near-misses stay literal
            ("2*3=6", "2*3=6"),
            ("a ** b", "a ** b"),
            ("**", "**"),
            ("****", "****"),
            ("* ", "* "),
            // escaping still applies inside emphasis
            ("**<x>**", "<strong>&lt;x&gt;</strong>"),
        ];
        for (src, want) in cases {
            let html = render_inline(db, "en", src, &mut cache, None).await;
            assert_eq!(html, *want, "{src:?}");
        }
    }

    /// `![alt](src)` renders an img; scheme and near-miss guards reject
    /// everything that is not an http(s) image, and a rejected shape stays
    /// literal text (escaped).
    #[tokio::test]
    async fn images_render_and_reject_unsafe_sources() {
        let Some(db) = scratch().await else {
            eprintln!("skip: set CR_SURREAL_URL to run the image tests");
            return;
        };
        let db = &db;
        let mut cache = LinkCache::new();
        let ok = render_inline(
            db,
            "en",
            "![a banner](https://example.com/x.png)",
            &mut cache,
            None,
        )
        .await;
        assert!(ok.contains("<img src=\"https://example.com/x.png\""));
        assert!(ok.contains("alt=\"a banner\""));

        // javascript:, data:, relative and schemeless sources stay literal
        for src in [
            "![x](javascript:alert(1))",
            "![x](data:image/png;base64,AAAA)",
            "![x](/static/img/news/a.png)",
            "![x](ftp://example.com/a.png)",
            "![x](https://)",
            "![x](https://ok.example/a.png",
            "![](https://ok.example/a.png)",
            "![alt only]",
            "![x]()",
        ] {
            let html = render_inline(db, "en", src, &mut cache, None).await;
            assert!(!html.contains("<img"), "{src:?} must not render: {html}");
            assert!(html.contains("&lt;") || html.contains('['), "{src:?}");
        }

        // escaping: quotes and angle brackets in either part cannot escape
        // the attribute
        let sneaky = render_inline(
            db,
            "en",
            "![a\" onerror=\"x](https://e.com/a.png)",
            &mut cache,
            None,
        )
        .await;
        assert!(
            sneaky.contains("alt=\"a&quot; onerror=&quot;x\""),
            "{sneaky}"
        );
    }

    /// `[text](href)` renders an anchor; scheme and near-miss guards reject
    /// everything that is not an http(s) target, a rejected shape stays
    /// literal text (escaped), and the label renders its own markup.
    #[tokio::test]
    async fn links_render_and_reject_unsafe_targets() {
        let Some(db) = scratch().await else {
            eprintln!("skip: set CR_SURREAL_URL to run the link tests");
            return;
        };
        let db = &db;
        let mut cache = LinkCache::new();
        let ok = render_inline(
            db,
            "en",
            "[the wiki](https://lebenoa.com)",
            &mut cache,
            None,
        )
        .await;
        assert_eq!(
            ok,
            "<a href=\"https://lebenoa.com\" class=\"text-primary underline decoration-primary decoration-2 underline-offset-4 hover:text-accent transition-colors\" target=\"_blank\" rel=\"noopener noreferrer\">the wiki</a>"
        );

        // the label renders its own inline markup: emphasis and colour nest
        let nested = render_inline(
            db,
            "en",
            "[**bold** and {color:red}red{/color}](https://e.com)",
            &mut cache,
            None,
        )
        .await;
        assert!(nested.contains("<strong>bold</strong>"), "{nested}");
        assert!(
            nested.contains("<span style=\"color:red\">red</span>"),
            "{nested}"
        );

        let cases: &[(&str, bool)] = &[
            // a rejected shape stays literal text
            ("[text](javascript:alert(1))", false),
            ("[text](data:text/html,x)", false),
            ("[text](/relative)", false),
            ("[text](ftp://e.com)", false),
            ("[text]() empty href", false),
            ("[](https://e.com) empty label", false),
            ("[text](https://)", false),
            // near-misses: unbalanced brackets
            ("[text(https://e.com)", false),
            ("text](https://e.com)", false),
            // a link inside bold, and vice versa
            ("**[l](https://e.com)**", true),
        ];
        for (src, renders) in cases {
            let html = render_inline(db, "en", src, &mut cache, None).await;
            assert_eq!(html.contains("<a href="), *renders, "{src:?} -> {html}");
        }

        // the target is escaped into the attribute: a quote in the href
        // cannot break out
        let sneaky = render_inline(
            db,
            "en",
            "[x](https://e.com/\" onmouseover=\"y)",
            &mut cache,
            None,
        )
        .await;
        assert!(
            sneaky.contains("href=\"https://e.com/&quot; onmouseover=&quot;y\""),
            "{sneaky}"
        );
    }
}
