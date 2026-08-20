pub mod catalog;
pub mod changelog;
pub mod detail;
pub mod misc;

use serde::Deserialize;

/// Query parameters the pages share. `lang` is resolved by the middleware
/// (see ctx::resolve_lang) rather than read here; it stays in the struct so
/// serde does not reject the parameter.
#[derive(Debug, Default, Deserialize)]
pub struct CommonQuery {
    #[allow(dead_code)]
    pub lang: Option<String>,
    pub page: Option<i64>,
    pub q: Option<String>,
    pub tab: Option<String>,
}
