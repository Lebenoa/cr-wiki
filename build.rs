//! Captures the git log at compile time so the release tarball — which ships
//! binary+static+translations+seed and no `.git` directory — still gets a
//! changelog. The raw `git log` text lands in the build output directory as
//! changelog.txt and
//! src/changelog.rs parses it with the same code it used to run against the
//! live repository, so there is exactly one parser.
//!
//! Build-time-only helper: unwrap/expect/panic are denied package-wide, and a
//! missing changelog must not block a build (the page has an empty state), so
//! every failure path degrades to an empty capture.

use std::path::Path;
use std::process::Command;

/// keep in sync with src/changelog.rs LIMIT
const LIMIT: &str = "200";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // A commit only ever moves a ref, so watching HEAD (branch switches) plus
    // the ref stores (new commits, whether loose or packed) is enough to pick
    // up history changes without rebuilding on every working-tree edit.
    if Path::new(".git").is_file() {
        // worktree/submodule checkout: .git points at the real gitdir
        println!("cargo:rerun-if-changed=.git");
    } else {
        println!("cargo:rerun-if-changed=.git/HEAD");
        println!("cargo:rerun-if-changed=.git/packed-refs");
        println!("cargo:rerun-if-changed=.git/refs/heads");
    }

    let out = Command::new("git")
        .args(["log", "--no-color", "--date=short", &format!("--max-count={LIMIT}")])
        .output();
    let text = match out {
        // lossy here, not at parse time: include_str! needs valid UTF-8 and
        // commit messages from arbitrary authors are not guaranteed to be
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        // no git on PATH, or no history yet: the page shows its empty state
        _ => String::new(),
    };

    let path = match std::env::var_os("OUT_DIR") {
        Some(dir) => Path::new(&dir).join("changelog.txt"),
        None => return,
    };
    let _ = std::fs::write(&path, text);
}
