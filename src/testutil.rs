//! Test support, behind `cfg(test)` only.
//!
//! The engine's `#[tokio::test]` fixtures are not exported to integration
//! tests, and the `Ctx` shape is its implementation detail — so the tests in
//! this crate share this stub instead of each rebuilding it.

use crate::ctx::Ctx;

/// Loads the .tr catalogs once per process, then returns a context standing
/// in for one resolved off a real request. Safe to call repeatedly.
pub fn test_ctx(lang: &str) -> Ctx {
    crate::i18n::load("translations");
    Ctx {
        l: crate::i18n::Loc::new(lang),
        lang: lang.to_string(),
        path: "/".into(),
        site_url: "http://localhost:6785".into(),
        // UTC unless a test overrides it: `Ctx { tz_offset: 420, ..
        // test_ctx("en") }` is how a zone-dependent render is pinned
        tz_offset: 0,
        htmx: false,
        boosted: false,
        is_local: true,
        user: None,
        google_login: false,
    }
}

/// The same stub with Google sign-in switched on, so a template that offers
/// the button can be rendered without installing the shared state.
pub fn test_ctx_with_google(lang: &str) -> Ctx {
    Ctx {
        google_login: true,
        ..test_ctx(lang)
    }
}
