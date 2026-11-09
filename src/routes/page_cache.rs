use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

use axum::{
    body::{Body, Bytes},
    http::{header, HeaderMap, HeaderValue, Response, StatusCode},
};

use crate::ctx::Ctx;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Key {
    url: String,
    lang: String,
    site_url: String,
    htmx: bool,
    boosted: bool,
    user_id: Option<String>,
    username: Option<String>,
    user_is_admin: bool,
    is_admin: bool,
    tz_offset: i64,
    google_login: bool,
}

impl Key {
    pub fn new(ctx: &Ctx, url: &str) -> Self {
        Self {
            url: url.to_owned(),
            lang: ctx.lang.clone(),
            site_url: ctx.site_url.clone(),
            htmx: ctx.htmx,
            boosted: ctx.boosted,
            user_id: ctx.user.as_ref().map(|u| u.id.clone()),
            username: ctx.user.as_ref().map(|u| u.username.clone()),
            user_is_admin: ctx.user.as_ref().is_some_and(|u| u.is_admin),
            is_admin: ctx.is_admin(),
            tz_offset: ctx.tz_offset,
            google_login: ctx.google_login,
        }
    }
}

#[derive(Debug)]
pub struct Page {
    bytes: Bytes,
    etag: String,
}

impl Page {
    pub fn new(html: String) -> Self {
        let etag = crate::conditional::tag(&html);
        Self {
            bytes: Bytes::from(html.into_bytes()),
            etag,
        }
    }

    pub fn response(&self, request: &HeaderMap) -> Response<Body> {
        let not_modified = request
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|raw| crate::conditional::none_match_hits(raw, &self.etag));
        let mut response = Response::new(if not_modified {
            Body::empty()
        } else {
            Body::from(self.bytes.clone())
        });
        if not_modified {
            *response.status_mut() = StatusCode::NOT_MODIFIED;
        }
        let headers = response.headers_mut();
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-cache"),
        );
        headers.insert(
            header::VARY,
            HeaderValue::from_static("Cookie, HX-Request, HX-Boosted, HX-History-Restore-Request"),
        );
        if !not_modified {
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
        }
        if let Ok(value) = HeaderValue::from_str(&self.etag) {
            headers.insert(header::ETAG, value);
        }
        response
    }
}

fn key_size(key: &Key) -> usize {
    key.url.len()
        + key.lang.len()
        + key.site_url.len()
        + key.username.as_ref().map_or(0, String::len)
}
struct Entry {
    revision: String,
    page: Arc<Page>,
    bytes: usize,
    touched: u64,
}
struct State {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    clock: u64,
}
pub struct Cache {
    max_entries: usize,
    max_bytes: usize,
    state: Mutex<State>,
}

impl Cache {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
            state: Mutex::new(State {
                entries: HashMap::new(),
                bytes: 0,
                clock: 0,
            }),
        }
    }

    pub fn get(&self, key: &Key, revision: &str) -> Option<Arc<Page>> {
        let mut state = self.state.lock().ok()?;
        let is_current = state
            .entries
            .get(key)
            .is_some_and(|entry| entry.revision == revision);
        if !is_current {
            if let Some(entry) = state.entries.remove(key) {
                state.bytes -= entry.bytes;
            }
            return None;
        }
        state.clock = state.clock.wrapping_add(1);
        let clock = state.clock;
        let entry = state.entries.get_mut(key)?;
        entry.touched = clock;
        Some(entry.page.clone())
    }

    pub fn insert(&self, key: Key, revision: String, html: String) -> Arc<Page> {
        let page = Arc::new(Page::new(html));
        let bytes = page
            .bytes
            .len()
            .saturating_add(key_size(&key))
            .saturating_add(revision.len())
            .saturating_add(page.etag.len());
        if bytes > self.max_bytes || self.max_entries == 0 {
            return page;
        }
        let Ok(mut state) = self.state.lock() else {
            return page;
        };
        if let Some(old) = state.entries.remove(&key) {
            state.bytes -= old.bytes;
        }
        while state.entries.len() >= self.max_entries
            || state.bytes.saturating_add(bytes) > self.max_bytes
        {
            let Some(oldest) = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(old) = state.entries.remove(&oldest) {
                state.bytes -= old.bytes;
            }
        }
        state.clock = state.clock.wrapping_add(1);
        let touched = state.clock;
        state.bytes += bytes;
        state.entries.insert(
            key,
            Entry {
                revision,
                page: page.clone(),
                bytes,
                touched,
            },
        );
        page
    }
}

pub fn cache() -> &'static Cache {
    static CACHE: LazyLock<Cache> = LazyLock::new(|| Cache::new(256, 32 * 1024 * 1024));
    &CACHE
}

#[cfg(test)]
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    fn key(url: &str) -> Key {
        let mut ctx = Ctx::minimal();
        ctx.site_url = "https://example.test".into();
        Key::new(&ctx, url)
    }

    #[test]
    fn bounds_and_lru_eviction() {
        let cache = Cache::new(2, 1000);
        let a = key("/a");
        let b = key("/b");
        let c = key("/c");
        cache.insert(a.clone(), "r".into(), "aa".into());
        cache.insert(b.clone(), "r".into(), "bb".into());
        assert!(cache.get(&a, "r").is_some());
        cache.insert(c.clone(), "r".into(), "cc".into());
        assert!(cache.get(&b, "r").is_none());
        assert!(cache.get(&a, "r").is_some());
        assert!(cache.get(&c, "r").is_some());
    }

    #[test]
    fn revision_mismatch_invalidates_and_variants_are_isolated() {
        let cache = Cache::new(4, 100);
        let a = key("/a");
        let b = key("/b");
        cache.insert(a.clone(), "old".into(), "old html".into());
        cache.insert(b.clone(), "new".into(), "new html".into());
        assert!(cache.get(&a, "new").is_none());
        assert!(cache.get(&b, "new").is_some());
        let mut ctx = Ctx::minimal();
        ctx.site_url = "https://example.test".into();
        let base = Key::new(&ctx, "/a");
        ctx.htmx = true;
        assert_ne!(base, Key::new(&ctx, "/a"));
        ctx.htmx = false;
        ctx.boosted = true;
        assert_ne!(base, Key::new(&ctx, "/a"));
        ctx.boosted = false;
        ctx.lang = "fr".into();
        assert_ne!(base, Key::new(&ctx, "/a"));
        ctx.lang = crate::i18n::DEFAULT_LANG.to_string();
        ctx.user = Some(crate::db::sessions::SessionUser {
            id: "41".into(),
            username: "alice".into(),
            is_admin: false,
        });
        let signed_in = Key::new(&ctx, "/a");
        assert_ne!(base, signed_in);
        ctx.user = Some(crate::db::sessions::SessionUser {
            id: "42".into(),
            username: "alice".into(),
            is_admin: false,
        });
        assert_ne!(signed_in, Key::new(&ctx, "/a"));
        ctx.user = Some(crate::db::sessions::SessionUser {
            id: "41".into(),
            username: "alice".into(),
            is_admin: true,
        });
        assert_ne!(signed_in, Key::new(&ctx, "/a"));
    }

    #[tokio::test]
    async fn matching_etag_returns_private_bodyless_304() {
        let page = Page::new("html".into());
        let mut request = HeaderMap::new();
        request.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_str(&page.etag).unwrap(),
        );
        let response = page.response(&request);
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-cache"
        );
        assert!(!response.headers().contains_key(header::CONTENT_TYPE));
        assert!(axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty());
    }

    #[test]
    fn oversized_page_is_served_without_retention() {
        let cache = Cache::new(2, 8);
        let page = cache.insert(key("/long-url"), "revision".into(), "html body".into());
        assert_eq!(page.bytes.as_ref(), b"html body");
        assert!(cache.get(&key("/long-url"), "revision").is_none());
    }
}
