//! The RSS document: the one place this app assembles XML, kept apart from the
//! HTTP surface so the escaping rules and the layout are unit-testable without
//! a router or a database.
//!
//! RSS 2.0, with the two namespaces a real reader expects to find: `atom` for
//! the feed's own address (`rel="self"`), and `dc` for the author — RSS's own
//! `<author>` element demands an email address, and these posts are written by
//! a wiki account rather than a mailbox.
//!
//! Every value an admin can type goes through `escape`. `&`, `<` and `>` are
//! the ones that silently break a document; quotes and apostrophes are escaped
//! too because the same helper covers attribute values and a future element
//! that carries one. A description may carry a post's whole rendered article:
//! entity-escaped HTML is how RSS 2.0 ships content in a description, and a
//! reader turns the entities back into the markup it renders. This module knows
//! nothing about news: it takes strings, one site to resolve links against and
//! one clock, which is what keeps the feed's shape testable in isolation.

/// The channel: the feed's identity, its own absolute address, the origin
/// relative links inside an entry are resolved against, and the clock to date
/// the document with when it carries no entries.
pub struct Channel<'a> {
    pub title: &'a str,
    pub link: &'a str,
    pub description: &'a str,
    pub language: &'a str,
    pub self_url: &'a str,
    /// the site the feed belongs to, e.g. `https://example.test`; empty when
    /// there is nothing to resolve against, and then nothing is rewritten
    pub site: &'a str,
    pub now: i64,
}

/// One entry. Links must be absolute: a feed reader has no page to resolve a
/// relative one against — the same holds for every link and image inside a
/// description, which is what `absolutize` is for.
pub struct Item {
    pub title: String,
    pub link: String,
    /// The entry's description: plain text, or a post's rendered article HTML.
    /// Both go through `escape` alike, so neither can close the element.
    pub description: String,
    pub published: i64,
    pub author: String,
}

/// The whole document. `lastBuildDate` is the newest entry's date rather than
/// the wall clock, so a reader that polls on a schedule does not see the feed
/// change under it when nothing was published; a feed with no entries at all
/// falls back to `Channel::now`.
pub fn rss(channel: &Channel<'_>, items: &[Item]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(
        "<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n",
    );
    out.push_str("<channel>\n");
    elem(&mut out, "title", channel.title);
    elem(&mut out, "link", channel.link);
    elem(&mut out, "description", channel.description);
    elem(&mut out, "language", channel.language);
    let newest = items
        .iter()
        .map(|i| i.published)
        .max()
        .filter(|t| *t > 0)
        .unwrap_or(channel.now);
    elem(&mut out, "lastBuildDate", &crate::time::format_rfc2822(newest));
    out.push_str("<atom:link rel=\"self\" type=\"application/rss+xml\" href=\"");
    out.push_str(&escape(channel.self_url));
    out.push_str("\"/>\n");
    for item in items {
        out.push_str("<item>\n");
        elem(&mut out, "title", &item.title);
        elem(&mut out, "link", &item.link);
        // the permalink doubles as the identity: a reader that has seen this
        // url and title once must not show the post twice
        out.push_str("<guid isPermaLink=\"true\">");
        out.push_str(&escape(&item.link));
        out.push_str("</guid>\n");
        elem(&mut out, "pubDate", &crate::time::format_rfc2822(item.published));
        // an absent description means an absent element, exactly as the list
        // cards hide a missing summary: an empty `<description/>` would only
        // spend an entry reader's attention on nothing
        if !item.description.is_empty() {
            elem(
                &mut out,
                "description",
                &absolutize(&item.description, channel.site),
            );
        }
        if !item.author.is_empty() {
            elem(&mut out, "dc:creator", &item.author);
        }
        out.push_str("</item>\n");
    }
    out.push_str("</channel>\n</rss>\n");
    out
}

/// One element, escaped. Every value in the document goes through here or
/// through `escape` directly, which is what keeps an admin's pasted text from
/// closing a tag.
fn elem(out: &mut String, name: &str, value: &str) {
    out.push('<');
    out.push_str(name);
    out.push('>');
    out.push_str(&escape(value));
    out.push_str("</");
    out.push_str(name);
    out.push_str(">\n");
}

/// Root-relative links and images inside an entry's description, made absolute
/// against the site they came from. A feed is read on a reader's own host, so
/// `/cookies/12` and `/static/img/cookies/x.png` would resolve to the reader
/// and render as a dead anchor and a broken sprite; only an absolute URL
/// survives the trip.
///
/// The rewrite is deliberately literal, and safe because of a property of the
/// article renderer rather than of a parser: it emits double-quoted `href="/…"`
/// and `src="/…"` attributes and escapes every quote in author text (`"`
/// becomes `&quot;`), so a `="/` inside a rendered description can only be an
/// attribute that renderer itself wrote. Rewriting anything else would need a
/// real HTML parser, and this is the only form the markup takes.
fn absolutize(description: &str, site: &str) -> String {
    let site = site.trim_end_matches('/');
    if site.is_empty() || !description.contains("=\"/") {
        return description.to_string();
    }
    description
        .replace("href=\"/", &format!("href=\"{site}/"))
        .replace("src=\"/", &format!("src=\"{site}/"))
}

/// XML text escaping. A control byte is not legal XML at all — and XML 1.0
/// admits no way to escape one — so the choices are to drop it or to ship a
/// document every parser rejects; tab and newline are legal and meaningful, so
/// they stay.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    fn channel() -> Channel<'static> {
        // 2024-02-29T12:34:05Z, an arbitrary but fixed clock
        Channel {
            title: "CookieRun: Classic/FanWiki — News",
            link: "https://example.test/news",
            description: "Announcements, updates and community news.",
            language: "en",
            self_url: "https://example.test/news/feed.xml",
            site: "https://example.test",
            now: 1_709_210_045,
        }
    }

    fn item(id: i64, title: &str, description: &str, author: &str) -> Item {
        Item {
            title: title.to_string(),
            link: format!("https://example.test/news/{id}?lang=th&utm=x"),
            description: description.to_string(),
            published: 1_792_108_800,
            author: author.to_string(),
        }
    }

    /// Admin text is escaped rather than trusted, the identity is the
    /// permalink, and the header carries both namespaces a reader looks for.
    #[test]
    fn the_document_escapes_admin_text() {
        let doc = rss(
            &channel(),
            &[item(
                7,
                "<script>alert('x') & \"y\"</script>",
                "Tea & crumpets > everything",
                "Ginger & Brave",
            )],
        );
        assert!(doc.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
        assert!(doc.contains("xmlns:atom=\"http://www.w3.org/2005/Atom\""), "{doc}");
        assert!(doc.contains("xmlns:dc=\"http://purl.org/dc/elements/1.1/\""), "{doc}");
        assert!(
            doc.contains("<title>&lt;script&gt;alert(&apos;x&apos;) &amp; &quot;y&quot;&lt;/script&gt;</title>"),
            "{doc}"
        );
        assert!(doc.contains("<description>Tea &amp; crumpets &gt; everything</description>"), "{doc}");
        assert!(doc.contains("<dc:creator>Ginger &amp; Brave</dc:creator>"), "{doc}");
        // the query separator is escaped in both the link and its guid
        assert!(
            doc.contains("<link>https://example.test/news/7?lang=th&amp;utm=x</link>"),
            "{doc}"
        );
        assert!(
            doc.contains("<guid isPermaLink=\"true\">https://example.test/news/7?lang=th&amp;utm=x</guid>"),
            "{doc}"
        );
        assert!(!doc.contains("<script>"), "{doc}");
    }

    /// The feed keeps the newest-first order its query handed it, and dates
    /// the document with the newest entry rather than the clock.
    #[test]
    fn entries_keep_their_order_and_date_the_document() {
        let mut newer = item(9, "Newer", "s", "a");
        newer.published = 1_792_195_200; // 2026-10-17
        let older = item(8, "Older", "s", "a");
        let doc = rss(&channel(), &[newer, older]);
        let first = doc.find("Newer").unwrap();
        let second = doc.find("Older").unwrap();
        assert!(first < second, "{doc}");
        assert!(doc.contains("<lastBuildDate>Sat, 17 Oct 2026 00:00:00 +0000</lastBuildDate>"), "{doc}");
    }

    /// The whole point of the description change: a subscriber reads the post
    /// itself, not a teaser. The article arrives entity-escaped — the RSS 2.0
    /// way of carrying HTML in a description — so the document stays valid and
    /// the reader is what turns the entities back into markup.
    #[test]
    fn a_description_carries_the_escaped_article() {
        let post = item(
            5,
            "Launch",
            "<p class=\"mt-4\">See <a href=\"/cookies/12\">GingerBrave</a>.</p>",
            "a",
        );
        let doc = rss(&channel(), &[post]);
        assert!(
            doc.contains(
                "<description>&lt;p class=&quot;mt-4&quot;&gt;See &lt;a \
                 href=&quot;https://example.test/cookies/12&quot;&gt;GingerBrave&lt;/a&gt;.&lt;/p&gt;</description>"
            ),
            "{doc}"
        );
        // nothing in the article escapes the element it lives in
        assert!(!doc.contains("<p class="), "{doc}");
        assert_eq!(doc.matches("<description>").count(), 2, "{doc}");
    }

    /// A reader resolves a root-relative link against its own host, so every
    /// one in a description is made absolute; an already-absolute link and a
    /// feed with no site to resolve against are left alone.
    #[test]
    fn relative_links_in_a_description_become_absolute() {
        let html = "<a href=\"/cookies/12\">x</a><img src=\"/static/img/cookies/x.png\"/>";
        assert_eq!(
            absolutize(html, "https://wiki.test/"),
            "<a href=\"https://wiki.test/cookies/12\">x</a><img \
             src=\"https://wiki.test/static/img/cookies/x.png\"/>"
        );
        let absolute = "<a href=\"https://other.test/x\">y</a>";
        assert_eq!(absolutize(absolute, "https://wiki.test"), absolute);
        assert_eq!(absolutize(html, ""), html, "no site, nothing to rewrite");
        assert_eq!(absolutize("plain text", "https://wiki.test"), "plain text");
    }

    /// A feed with nothing in it is still a valid document — and an entry
    /// without a description at all (no article, no summary) produces no
    /// element rather than an empty one, so the only `<description>` left is
    /// the channel's.
    #[test]
    fn an_empty_feed_and_a_summaryless_entry_stay_valid() {
        let empty = rss(&channel(), &[]);
        assert!(empty.contains("<channel>"), "{empty}");
        assert!(!empty.contains("<item>"), "{empty}");
        assert!(
            empty.contains("<lastBuildDate>Thu, 29 Feb 2024 12:34:05 +0000</lastBuildDate>"),
            "an empty feed dates itself with the clock: {empty}"
        );
        assert!(empty.ends_with("</channel>\n</rss>\n"), "{empty}");

        let bare = rss(&channel(), &[item(1, "No summary", "", "")]);
        assert_eq!(bare.matches("<description>").count(), 1, "{bare}");
        assert_eq!(bare.matches("<dc:creator>").count(), 0, "{bare}");
        assert_eq!(bare.matches("<item>").count(), 1, "{bare}");
    }

    /// The metacharacters a document cannot survive, plus the illegal control
    /// bytes that would make a parser reject the whole feed.
    #[test]
    fn escape_covers_metacharacters_and_drops_control_bytes() {
        assert_eq!(escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&apos;");
        assert_eq!(escape("keep\ttab\nand newline"), "keep\ttab\nand newline");
        assert_eq!(escape("drop\u{0}this\u{7}"), "dropthis");
        assert_eq!(escape(""), "");
    }
}
