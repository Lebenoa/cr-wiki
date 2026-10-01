//! Admin image upload.
//!
//! Files land in `static/img/<section>/`, which the static handler already
//! serves, so an uploaded sprite is reachable the moment it is written — the
//! V app extends its in-memory static cache for the same reason.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::section::Section;

/// Where the sprites for one section live, relative to the working
/// directory — the same `static/` the `ServeDir` mount serves, so an uploaded
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
    if bytes.len() >= 12
        && bytes.starts_with(b"RIFF")
        && bytes.get(8..12) == Some(b"WEBP".as_slice())
    {
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

/// Writes `bytes` to `dir` under the first free `<stem>.<ext>` candidate,
/// adding _2, _3 and so on — the same shape the seed script's image naming
/// uses. The claim is atomic: `create_new` refuses to open an existing file
/// for writing, so a concurrent upload racing for the same name cannot
/// clobber the loser's sprite; the loser simply retries the next candidate.
/// `Ok(None)` when every candidate up to `stem_99` is taken: the caller must
/// fail the upload, never overwrite an existing sprite (the pre-`_100` break
/// once returned the last occupied candidate and clobbered that entity's
/// image). A failed write removes the just-created file so no half-written
/// sprite is ever served.
pub fn write_unique(
    dir: &Path,
    stem: &str,
    ext: &str,
    bytes: &[u8],
) -> std::io::Result<Option<String>> {
    let mut candidate = format!("{stem}.{ext}");
    let mut n: u32 = 2;
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(&candidate))
        {
            Ok(mut file) => {
                return match file.write_all(bytes) {
                    Ok(()) => Ok(Some(candidate)),
                    Err(e) => {
                        let _ = std::fs::remove_file(dir.join(&candidate));
                        Err(e)
                    }
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if n > 99 {
                    return Ok(None);
                }
                candidate = format!("{stem}_{n}.{ext}");
                n = n.saturating_add(1);
            }
            Err(e) => return Err(e),
        }
    }
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

    /// Every candidate up to `stem_99` taken must answer Ok(None), never the
    /// occupied `stem_99` — the caller writes whatever comes back, so a
    /// taken name in the return value means a clobbered sprite. The claim is
    /// also atomic: create_new means a second claim of the same name takes
    /// the next slot rather than clobbering the first (the TOCTOU the old
    /// check-then-write had).
    #[test]
    fn write_unique_exhaustion_and_race() {
        let dir = std::env::temp_dir().join(format!("cr_upload_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        // untouched stem: the plain name wins
        assert_eq!(
            write_unique(&dir, "ginger", "png", b"a").expect("write"),
            Some("ginger.png".to_string())
        );
        assert_eq!(std::fs::read(dir.join("ginger.png")).expect("read"), b"a");

        // one collision: the first numbered slot
        assert_eq!(
            write_unique(&dir, "ginger", "png", b"b").expect("write"),
            Some("ginger_2.png".to_string())
        );
        assert_eq!(std::fs::read(dir.join("ginger_2.png")).expect("read"), b"b");

        // everything taken up to the cap: fail, not overwrite
        for n in 3..=99 {
            std::fs::write(dir.join(format!("ginger_{n}.png")), b"x").expect("write");
        }
        assert_eq!(
            write_unique(&dir, "ginger", "png", b"c").expect("exhaustion"),
            None
        );
        // nothing clobbered by the failed claim
        assert_eq!(
            std::fs::read(dir.join("ginger_99.png")).expect("read"),
            b"x"
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The race the reviewers flagged: with a check-then-write, two
    /// concurrent claims could both pass `exists()` and the second write
    /// would clobber the first. create_new makes the second claimant take
    /// the next slot, and the first sprite's content survives.
    #[test]
    fn concurrent_write_unique_claims_never_clobber() {
        let dir = std::env::temp_dir().join(format!("cr_upload_race_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    write_unique(&dir, "skater", "png", &[i; 16]).expect("write")
                })
            })
            .collect();
        let mut names = Vec::new();
        for h in handles {
            names.extend(h.join().expect("join"));
        }

        // every claimant got a name, all distinct, all contents intact:
        // whoever won the plain name wrote exactly its own 16 bytes there —
        // create_new makes cross-clobbering impossible, which is the point
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len(), "no two claimants shared a name");
        assert_eq!(names[0], "skater.png");
        let plain = std::fs::read(dir.join("skater.png")).expect("read");
        assert!(
            plain == [0u8; 16]
                || plain == [1u8; 16]
                || plain == [2u8; 16]
                || plain == [3u8; 16]
                || plain == [4u8; 16]
                || plain == [5u8; 16]
                || plain == [6u8; 16]
                || plain == [7u8; 16],
            "skater.png must hold one claimant's content verbatim, got {plain:?}"
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
