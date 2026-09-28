//! Admin image upload.
//!
//! Files land in `static/img/<section>/`, which the static handler already
//! serves, so an uploaded sprite is reachable the moment it is written — the
//! V app extends its in-memory static cache for the same reason.

use std::path::{Path, PathBuf};

use crate::section::Section;

/// Where the sprites for one section live, relative to the working
/// directory — the same `static/` the ServeDir mount serves, so an uploaded
/// sprite is reachable immediately. The section arrives already typed off
/// the path capture, so the only question left is whether the editor
/// writes it.
pub fn section_dir(section: Section) -> Option<PathBuf> {
    section
        .editable()
        .then(|| PathBuf::from("static/img").join(section.as_str()))
}

/// Content the catalog actually serves, verified by magic bytes rather than
/// the client's declared Content-Type, which an attacker controls. Each
/// signature is the file-format anchor; anything else is rejected.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Some("jpg");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    None
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

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// Upload names are sanitised: no separator, traversal segment or
    /// control character survives, and the extension comes from the declared
    /// content type rather than the submitted filename.
    #[test]
    fn upload_names_are_safe() {
        assert_eq!(safe_stem("Cloud Boots.png"), "cloud_boots");
        assert_eq!(safe_stem("../../etc/passwd"), "passwd");
        assert_eq!(safe_stem("a/b/c.png"), "c");
        assert_eq!(safe_stem("....."), "image");
        assert_eq!(safe_stem(""), "image");
        assert!(!safe_stem("sprite.png.html").contains('.'));
        assert!(safe_stem("x".repeat(200).as_str()).len() <= 60);

        assert_eq!(extension_for("image/png"), Some("png"));
        assert_eq!(extension_for("image/jpeg"), Some("jpg"));
        // markup and scripts are not images, whatever the filename says
        assert_eq!(extension_for("text/html"), None);
        assert_eq!(extension_for("application/octet-stream"), None);

        assert!(section_dir(Section::Cookies).is_some());
        assert!(section_dir(Section::Treasures).is_some());
        assert!(section_dir(Section::Relics).is_none());
        assert!(section_dir(Section::Skins).is_none());
    }
}
