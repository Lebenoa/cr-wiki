pub mod changelog;
pub mod cookies;

use serde::Deserialize;

/// Query parameters the pages share. `lang` is resolved by the middleware
/// (see ctx::resolve_lang) rather than read here; it stays in the struct so
/// serde does not reject the parameter.
#[derive(Debug, Default, Deserialize)]
pub struct CommonQuery {
    #[allow(dead_code)]
    pub lang: Option<String>,
    pub page: Option<i64>,
    /// used once the search-bearing pages land (see PORTING.md)
    #[allow(dead_code)]
    pub q: Option<String>,
    #[allow(dead_code)]
    pub tab: Option<String>,
}
