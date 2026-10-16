//! Conditional GET: the two validators a generated document is served with,
//! and the 304 that answers a client whose copy is still current.
//!
//! The two documents this app serves conditionally — the news feed and the
//! sitemap — are generated per request and cached by the client for ten
//! minutes, so a polling reader or a crawler re-downloads the whole thing on
//! every revalidation whether or not a byte of it changed. Revalidating is
//! what the client is *supposed* to do; answering `304 Not Modified` is what
//! makes it cheap.
//!
//! Two validators, because they answer different questions and RFC 9110 gives
//! them different strengths:
//!
//! - the `ETag` is exact by construction: it is a digest of the very bytes
//!   about to be served, so a document that differs anywhere — a new post, an
//!   edited body, a catalog row that added a sitemap url, a channel title from
//!   a translation file — cannot carry the tag of the previous version. It is
//!   compared under the weak rule, as `If-None-Match` requires.
//! - the `Last-Modified` is the newest moment the document's content
//!   reflects. It is the timestamp a client that only sends
//!   `If-Modified-Since` revalidates with, and the shape a human reads in a
//!   response, which is why the caller passes a timestamp rather than an opaque
//!   stamp. Each document works its own out — the feed from its posts and the
//!   news change stamp, the sitemap from the catalog's stamps and the newest
//!   publication (`db::stamp`, `news::newest_publication`) — and may pass
//!   `None` when there is nothing to date it by at all, in which case only the
//!   tag can yield a 304. It is coarser than the tag by nature: a stamp dates a
//!   *table*, so a write to one row moves the date of a document the row is not
//!   in, which costs a re-fetch and never a stale answer.
//!
//! Precedence follows RFC 9110 §13.1.3 exactly: when a request carries
//! `If-None-Match`, that header decides on its own and `If-Modified-Since` is
//! ignored — otherwise a client that had already received a *different*
//! representation could be told "not modified" on the strength of a date.
//!
//! A 304 repeats `Cache-Control`: revalidation is what refreshes the client's
//! freshness window, so a 304 that dropped it would shorten the cache the 200
//! established. It carries no `Content-Type`, since it carries no body.

use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;

/// The freshness window both conditional documents are advertised with, and
/// the one a 304 repeats. Ten minutes: long enough that a poll costs nothing,
/// short enough that a new post reaches a subscriber's reader promptly.
///
/// A client that revalidates inside the window is answered from its own copy
/// without asking, and one that revalidates past it costs a stamp read and the
/// document's own reads — the sitemap's url list is cached against that stamp
/// (`db::sitemap_entries`), so the eight table scans behind it are paid once
/// per change rather than once per crawler.
pub const CACHE_CONTROL: &str = "public, max-age=600";

/// Serves `body` as a 200 with its validators, or answers the request with a
/// 304 when the validators prove the client's copy is current.
///
/// `last_modified` is the newest content timestamp the document reflects
/// (`None` when there is nothing to date it by, in which case no
/// `Last-Modified` is sent and only the tag can yield a 304).
pub fn document(
    request: &HeaderMap,
    content_type: &str,
    body: String,
    last_modified: Option<i64>,
) -> Response {
    let etag = tag(&body);
    if is_current(request, &etag, last_modified) {
        return not_modified(&etag, last_modified);
    }
    let mut res = Response::new(Body::from(body));
    *res.status_mut() = StatusCode::OK;
    insert(&mut res, header::CONTENT_TYPE, content_type);
    validators(&mut res, &etag, last_modified);
    res
}

/// Whether the request already holds this representation. See the module doc
/// for where the precedence rule comes from.
fn is_current(request: &HeaderMap, etag: &str, last_modified: Option<i64>) -> bool {
    if let Some(raw) = request
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        return none_match_hits(raw, etag);
    }
    let Some(since) = request
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
        .and_then(crate::time::parse_http_date)
    else {
        return false;
    };
    // the document is current when it was last changed at or before the date
    // the client already holds
    last_modified.is_some_and(|ts| ts <= since)
}

/// An `If-None-Match` value: `*`, or a comma-separated list of entity-tags,
/// any one of which matching means the client is current. The comparison is
/// the weak one, `If-None-Match`'s rule, so a `W/"…"` client tag matches the
/// strong tag we sent for the same bytes.
fn none_match_hits(raw: &str, etag: &str) -> bool {
    let ours = weak(etag);
    raw.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || weak(candidate) == ours
    })
}

fn weak(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

/// The 304: the two validators, the freshness window, and no body.
fn not_modified(etag: &str, last_modified: Option<i64>) -> Response {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::NOT_MODIFIED;
    validators(&mut res, etag, last_modified);
    res
}

fn validators(res: &mut Response, etag: &str, last_modified: Option<i64>) {
    insert(res, header::ETAG, etag);
    if let Some(ts) = last_modified {
        insert(res, header::LAST_MODIFIED, &crate::time::format_http_date(ts));
    }
    insert(res, header::CACHE_CONTROL, CACHE_CONTROL);
}

/// A header, dropped rather than panicking on a value that cannot be one: an
/// unrepresentable header is a missing header, never a failed response.
fn insert(res: &mut Response, name: header::HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        res.headers_mut().insert(name, value);
    }
}

/// The document's entity-tag: its length and a digest of it, so two versions
/// can only share a tag by colliding in both. `len` is carried alongside the
/// hash because a length is free to check and closes the trivial collisions a
/// 64-bit digest admits.
fn tag(body: &str) -> String {
    format!("\"{}-{:016x}\"", body.len(), digest(body.as_bytes()))
}

/// FNV-1a, 64-bit — chosen for being six lines with no dependency, not for
/// being a cryptographic hash, which an entity-tag does not need to be: it
/// has to tell two versions of a document apart, and whoever can forge a
/// collision can already edit the document itself.
fn digest(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// 2026-10-16T00:00:00Z
    const NEWEST: i64 = 1_792_108_800;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                header::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                HeaderValue::from_str(value).expect("value"),
            );
        }
        map
    }

    /// One header as a string, read before the body is consumed.
    fn head(res: &Response, name: header::HeaderName) -> String {
        res.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    async fn body_of(res: Response) -> String {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8_lossy(&bytes).to_string()
    }

    /// The plain case: a first fetch is a 200 carrying the document, the tag
    /// that identifies it and the date its content reflects.
    #[tokio::test]
    async fn a_first_fetch_is_a_200_carrying_both_validators() {
        let res = document(&HeaderMap::new(), "application/xml", "<xml/>".into(), Some(NEWEST));
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(head(&res, header::CONTENT_TYPE), "application/xml");
        assert_eq!(
            head(&res, header::LAST_MODIFIED),
            "Fri, 16 Oct 2026 00:00:00 GMT",
            "an HTTP-date, not the feed's numeric offset"
        );
        assert_eq!(head(&res, header::CACHE_CONTROL), CACHE_CONTROL);
        assert!(head(&res, header::ETAG).starts_with('"'));
        assert_eq!(body_of(res).await, "<xml/>");
    }

    /// The tag is a function of the bytes: the same document revalidates, a
    /// document that changed anywhere does not — and the same tag comes back
    /// no matter which body produced it first.
    #[test]
    fn the_tag_follows_the_bytes() {
        assert_eq!(tag("<xml/>"), tag("<xml/>"));
        assert_ne!(tag("<xml/>"), tag("<xml />"));
        assert_ne!(tag("<xml/>"), tag("<xml/>\n"));
        // the algorithm itself, pinned: FNV-1a 64's published vectors
        assert_eq!(digest(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(digest(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(digest(b"foobar"), 0x8594_4171_f739_67e8);
        // and the length rides along, so equal-length collisions are not the
        // only thing standing between two documents and one tag
        assert!(tag("abc").starts_with("\"3-"), "{}", tag("abc"));
    }

    /// A client that sends back the tag it was given gets a 304 with no body,
    /// no content type, and the freshness window it asked to refresh.
    #[tokio::test]
    async fn a_matching_tag_answers_304_without_a_body() {
        let first = document(&HeaderMap::new(), "application/xml", "<xml/>".into(), Some(NEWEST));
        let etag = head(&first, header::ETAG);

        let again = document(
            &headers(&[("if-none-match", &etag)]),
            "application/xml",
            "<xml/>".into(),
            Some(NEWEST),
        );
        assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
        assert!(again.headers().get(header::CONTENT_TYPE).is_none(), "a 304 has no representation");
        assert_eq!(head(&again, header::ETAG), etag);
        assert_eq!(head(&again, header::CACHE_CONTROL), CACHE_CONTROL);
        assert_eq!(head(&again, header::LAST_MODIFIED), "Fri, 16 Oct 2026 00:00:00 GMT");
        assert_eq!(body_of(again).await, "");

        // a tag for a different document re-fetches, however new the date is
        let changed = document(
            &headers(&[("if-none-match", "\"1234-0000000000000000\"")]),
            "application/xml",
            "<xml/>".into(),
            Some(NEWEST),
        );
        assert_eq!(changed.status(), StatusCode::OK);
        assert_eq!(body_of(changed).await, "<xml/>");
    }

    /// `If-None-Match` accepts `*`, a list, and a weak tag for the same bytes
    /// — and, when it is present and does not match, it overrules
    /// `If-Modified-Since`, which is the rule that keeps a client from being
    /// told "not modified" after it was already sent something else.
    #[test]
    fn the_tag_header_outranks_the_date_header() {
        let etag = tag("<xml/>");
        let fresh = |inm: &str| {
            let res = document(
                &headers(&[
                    ("if-none-match", inm),
                    ("if-modified-since", "Fri, 01 Jan 2100 00:00:00 GMT"),
                ]),
                "application/xml",
                "<xml/>".into(),
                Some(NEWEST),
            );
            res.status()
        };
        assert_eq!(fresh("*"), StatusCode::NOT_MODIFIED);
        assert_eq!(fresh(&format!("\"other\", {etag}")), StatusCode::NOT_MODIFIED);
        assert_eq!(fresh(&format!("W/{etag}")), StatusCode::NOT_MODIFIED);
        assert_eq!(fresh("\"other\""), StatusCode::OK, "the date must not rescue it");
    }

    /// The date path: a client whose copy is at least as new as the document's
    /// last change revalidates, one holding an older copy re-downloads, and an
    /// unreadable date is not a 304.
    #[test]
    fn the_date_header_decides_when_it_is_the_only_one() {
        let at = |date: &str| {
            document(
                &headers(&[("if-modified-since", date)]),
                "application/xml",
                "<xml/>".into(),
                Some(NEWEST),
            )
            .status()
        };
        assert_eq!(at("Fri, 16 Oct 2026 00:00:00 GMT"), StatusCode::NOT_MODIFIED);
        assert_eq!(at("Sat, 17 Oct 2026 00:00:00 GMT"), StatusCode::NOT_MODIFIED);
        assert_eq!(at("Thu, 15 Oct 2026 23:59:59 GMT"), StatusCode::OK);
        assert_eq!(at("garbage"), StatusCode::OK);
        assert_eq!(at("Fri, 16 Oct 2026 00:00:00 GMT+9"), StatusCode::OK);
    }

    /// A document with nothing to date it by still revalidates on its tag; no
    /// date is sent, and a date-only client is answered with the document
    /// rather than against a stamp we never advertised.
    #[test]
    fn a_document_without_a_date_revalidates_on_its_tag_alone() {
        let first = document(&HeaderMap::new(), "application/xml", "<xml/>".into(), None);
        assert!(first.headers().get(header::LAST_MODIFIED).is_none(), "no date to send");
        let etag = first.headers().get(header::ETAG).unwrap().to_str().unwrap().to_string();

        let tagged = document(
            &headers(&[("if-none-match", &etag)]),
            "application/xml",
            "<xml/>".into(),
            None,
        );
        assert_eq!(tagged.status(), StatusCode::NOT_MODIFIED);
        assert!(tagged.headers().get(header::LAST_MODIFIED).is_none());

        let dated = document(
            &headers(&[("if-modified-since", "Fri, 16 Oct 2026 00:00:00 GMT")]),
            "application/xml",
            "<xml/>".into(),
            None,
        );
        assert_eq!(dated.status(), StatusCode::OK);
    }

    /// No conditional header at all is a plain 200 — and an empty document is
    /// still tagged, so the next poll of an empty feed can revalidate too.
    #[tokio::test]
    async fn an_unconditional_request_gets_the_document() {
        let res = document(&HeaderMap::new(), "application/xml", String::new(), None);
        assert_eq!(res.status(), StatusCode::OK);
        assert!(head(&res, header::ETAG).starts_with('"'), "even an empty document is tagged");
        assert_eq!(body_of(res).await, "");
    }
}
