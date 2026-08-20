pub mod changelog;
pub mod cookies;

use axum::extract::Query;
use serde::Deserialize;

use crate::i18n;

/// Query parameters every page shares. `lang` selects the locale, validated
/// against the loaded catalogs — the V app also falls back to the `wikilang`
/// cookie, which is still to port.
#[derive(Debug, Default, Deserialize)]
pub struct CommonQuery {
    pub lang: Option<String>,
    pub page: Option<i64>,
    /// used once the search-bearing pages land (see PORTING.md)
    #[allow(dead_code)]
    pub q: Option<String>,
    #[allow(dead_code)]
    pub tab: Option<String>,
}

pub fn lang_of(q: &Query<CommonQuery>) -> String {
    match q.lang.as_deref() {
        Some(l) if i18n::is_available(l) => l.to_string(),
        _ => i18n::DEFAULT_LANG.to_string(),
    }
}
