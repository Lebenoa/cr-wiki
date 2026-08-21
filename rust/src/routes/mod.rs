pub mod auth;
pub mod builds;
pub mod catalog;
pub mod changelog;
pub mod detail;
pub mod misc;
pub mod picker;
pub mod planner;

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
    pub sort: Option<String>,
    pub ep: Option<String>,
    pub cookie: Option<i64>,
    pub pet: Option<i64>,
    pub treasure: Option<i64>,
    /// the option already in the slot the picker is opening for
    pub sel: Option<i64>,
    /// the other half of the combo pair, whose partners float to the top
    pub partner: Option<i64>,
}
