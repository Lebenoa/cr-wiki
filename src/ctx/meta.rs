//! The head half of the context: titles, descriptions and the Open Graph image
//! facts, including the sprite measurement the `og:image` tags need.

use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;

use super::Ctx;
impl Ctx {
    /// Serialize breadcrumb tuples for safe embedding in a JSON-LD script.
    // an associated function would be cleaner, but askama templates call
    // this through the `ctx` receiver (`title.html`)
    #[allow(clippy::unused_self)]
    pub fn breadcrumb_json(&self, items: &[(String, Option<String>)]) -> String {
        let item_list = items
            .iter()
            .enumerate()
            .map(|(index, (name, url))| {
                let mut item = serde_json::Map::new();
                item.insert(
                    "@type".to_string(),
                    serde_json::Value::String("ListItem".to_string()),
                );
                item.insert("position".to_string(), serde_json::Value::from(index + 1));
                item.insert("name".to_string(), serde_json::Value::String(name.clone()));
                if let Some(url) = url {
                    item.insert("item".to_string(), serde_json::Value::String(url.clone()));
                }
                serde_json::Value::Object(item)
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "@context": "https://schema.org",
            "@type": "BreadcrumbList",
            "itemListElement": item_list,
        });
        value.to_string().replace('<', "\\u003c")
    }

    /// A page title with the site name appended.
    pub fn page_title(&self, key: &str) -> String {
        crate::chrome::page_title(&self.l, key)
    }

    /// The same, for a title built from an entity name rather than a key.
    // the {name} placeholder is consumed by replace(), not a formatting macro
    #[allow(clippy::literal_string_with_formatting_args)]
    pub fn entity_title(&self, name: &str) -> String {
        crate::chrome::entity_title(&self.l, name)
    }

    /// The site-wide description, used when a page names no key of its own.
    pub fn meta_description(&self) -> String {
        self.l.t("site_description")
    }

    /// A page's own description. A key with no string behind it falls back
    /// to the site one rather than printing the key.
    pub fn page_desc(&self, key: &str) -> String {
        crate::chrome::page_desc(&self.l, key)
    }

    /// A detail page's description. Only some kinds have a template of their
    /// own; the rest read the site line.
    // the {name} placeholder is consumed by replace(), not a formatting macro
    #[allow(clippy::literal_string_with_formatting_args)]
    pub fn entity_desc(&self, section: &str, name: &str) -> String {
        crate::chrome::entity_desc(&self.l, section, name)
    }

    /// The wide banner, absolute, for Open Graph and Twitter cards.
    #[allow(clippy::unused_self)] // askama calls it on the context
    pub fn social_image(&self) -> String {
        format!(
            "{}/static/img/landscape.jpg",
            self.site_url.trim_end_matches('/')
        )
    }

    /// A detail page shares the entity's own sprite instead, when it has one.
    pub fn entity_image(&self, section: &str, image: Option<&String>) -> String {
        match image {
            Some(img) if !img.is_empty() => {
                format!(
                    "{}/static/img/{section}/{img}",
                    self.site_url.trim_end_matches('/')
                )
            }
            _ => self.social_image(),
        }
    }

    /// An entity sprite is a square icon, so it reads better as a summary
    /// thumbnail; the wide banner wants the large card. A method because
    /// askama calls it on the context.
    #[allow(clippy::unused_self)]
    pub fn social_card_type(&self, image: &str) -> &'static str {
        if image.ends_with("/static/img/landscape.jpg") {
            "summary_large_image"
        } else {
            "summary"
        }
    }

    /// The image URL's MIME type from its extension; sprites are PNG, the
    /// banner JPEG. A method because askama's lexer has no single-quoted
    /// strings, so the branch cannot live inline in the attribute. The
    /// comparison is case-insensitive out of paranoia about what a hand
    /// edit of the image column can hold.
    #[allow(clippy::unused_self)]
    pub fn image_mime(&self, image: &str) -> &'static str {
        if image.to_ascii_lowercase().ends_with(".png") {
            "image/png"
        } else {
            "image/jpeg"
        }
    }

    /// The image URL's on-disk size for `og:image:width`/`height`, so
    /// platform previews reserve the right aspect for a detail page's sprite
    /// as well as the wide banner. Banner dims when the file is missing or
    /// unreadable — a wrong hint is better than an absent one, and the
    /// sprite is served from `/static/img/...`, which this method maps to
    /// `static/img/...`.
    ///
    /// Sprites are immutable (the uploader hands out fresh names and the
    /// static handler caches them for a year), so the measured size is
    /// memoized per path: a detail page otherwise rereads the file header
    /// on every view.
    #[allow(clippy::unused_self)]
    pub fn image_dimensions(&self, image: &str) -> (u32, u32) {
        const BANNER: (u32, u32) = (2560, 1440);
        let Some(rel) = image
            .strip_prefix(&format!("{}/", self.site_url.trim_end_matches('/')))
            .and_then(|rel| rel.strip_prefix("static/"))
        else {
            return BANNER;
        };
        let key = format!("static/{rel}");
        if let Some(dims) = dims::get(&key) {
            return dims;
        }
        match measure(&key) {
            Some(dims) => {
                dims::insert(key, dims);
                dims
            }
            // only successful reads are cached: a sprite that appears later
            // (admin upload) must not be hidden behind a stale banner
            None => BANNER,
        }
    }
}

/// Measures an image file's pixel size by reading its header bytes.
fn measure(path: &str) -> Option<(u32, u32)> {
    let f = std::fs::File::open(path).ok()?;
    imagesize::reader_size(std::io::BufReader::new(f))
        .ok()
        .map(|d| {
            (
                u32::try_from(d.width).unwrap_or(0),
                u32::try_from(d.height).unwrap_or(0),
            )
        })
}

/// Memoized sprite dimensions, keyed by `static/img/...` path. Bounded by
/// the catalog size (one entry per sprite; every name is unique forever),
/// and only successful reads are stored — an upload that later fills a
/// path must show real dimensions, not a cached banner.
mod dims {
    use super::{HashMap, LazyLock, Mutex};

    static CACHE: LazyLock<Mutex<HashMap<String, (u32, u32)>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    pub fn get(key: &str) -> Option<(u32, u32)> {
        CACHE.lock().get(key).copied()
    }

    pub fn insert(key: String, dims: (u32, u32)) {
        CACHE.lock().insert(key, dims);
    }
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    /// A 1x1 PNG, enough for the header parser to answer (width, height).
    const ONE_BY_ONE_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    /// The header reader answers the real pixel size, and the memo only
    /// serves what succeeded — a missing file never poisons the cache with
    /// a banner that would hide a later upload.
    #[test]
    fn sprite_dimensions_measure_and_cache_successes() {
        let dir = std::env::temp_dir().join(format!("cr_dims_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let sprite = dir.join("sprite.png");
        std::fs::write(&sprite, ONE_BY_ONE_PNG).expect("write");

        assert_eq!(measure(sprite.to_str().expect("path")), Some((1, 1)));
        // a missing file measures None (and nothing is cached for it)
        assert_eq!(
            measure(&(sprite.to_str().expect("path").to_owned() + ".missing")),
            None
        );

        let key = "static/img/cookies/sprite.png".to_string();
        dims::insert(key.clone(), (1, 1));
        assert_eq!(dims::get(&key), Some((1, 1)));
        assert_eq!(dims::get("static/img/cookies/other.png"), None);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
    #[test]
    fn breadcrumb_json_preserves_script_sensitive_values() {
        let values = [
            (
                "quote \" & ampersand \\ newline\n</script>".to_string(),
                Some("https://example.test/a?x=\"&y=\\\nz".to_string()),
            ),
            ("current </script>".to_string(), None),
        ];
        let ctx = crate::testutil::test_ctx("en");
        let rendered = ctx.breadcrumb_json(&values);
        assert!(!rendered.contains("</script>"));
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON-LD");
        assert_eq!(parsed["itemListElement"][0]["name"], values[0].0);
        assert_eq!(
            parsed["itemListElement"][0]["item"],
            values[0].1.as_deref().unwrap()
        );
        assert_eq!(parsed["itemListElement"][0]["position"], 1);
        assert_eq!(parsed["itemListElement"][1]["name"], values[1].0);
        assert_eq!(parsed["itemListElement"][1]["position"], 2);
        assert!(parsed["itemListElement"][1].get("item").is_none());
    }
}
