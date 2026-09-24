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
        htmx: false,
        boosted: false,
        is_local: true,
        user: None,
    }
}
