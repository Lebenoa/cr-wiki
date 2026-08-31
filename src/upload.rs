//! Admin image upload.
//!
//! Files land in `static/img/<section>/`, which the static handler already
//! serves, so an uploaded sprite is reachable the moment it is written — the
//! V app extends its in-memory static cache for the same reason.

use std::path::{Path, PathBuf};

/// Where the sprites for one section live, relative to the repo root.
pub fn section_dir(section: &str) -> Option<PathBuf> {
    let leaf = match section {
        "cookies" | "pets" | "treasures" | "episodes" | "ingredients" | "jellies" | "skins"
        | "relics" => section,
        _ => return None,
    };
    Some(PathBuf::from("../static/img").join(leaf))
}

/// Only the image types the catalog actually uses. The extension is taken
/// from this table rather than from the upload's filename, so a name like
/// `sprite.png.html` cannot end up served as markup.
pub fn extension_for(content_type: &str) -> Option<&'static str> {
    match content_type {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

/// A filesystem-safe stem built from the submitted name: lowercase, ASCII
/// alphanumerics and underscores only. Everything else collapses to `_`, so
/// no separator, traversal segment or control character survives.
pub fn safe_stem(raw: &str) -> String {
    let base = Path::new(raw)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let mut out = String::with_capacity(base.len());
    let mut last_underscore = false;
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "image".to_string()
    } else {
        trimmed.chars().take(60).collect()
    }
}

/// Picks a name that is not taken, adding _2, _3 and so on — the same shape
/// the seed script's image naming uses.
pub fn unique_name(dir: &Path, stem: &str, ext: &str) -> String {
    let mut candidate = format!("{stem}.{ext}");
    let mut n: u32 = 2;
    while dir.join(&candidate).exists() {
        candidate = format!("{stem}_{n}.{ext}");
        n = n.saturating_add(1);
        if n > 99 {
            break;
        }
    }
    candidate
}

/// 4MB, comfortably above a sprite and well below anything worth worrying
/// about holding in memory.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
