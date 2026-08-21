pub mod admin;
pub mod api;
pub mod auth;
pub mod builds;
pub mod catalog;
pub mod changelog;
pub mod detail;
pub mod misc;
pub mod picker;
pub mod planner;
pub mod uploads;

use serde::{Deserialize, Deserializer};

/// Treats an empty parameter as absent.
///
/// picker.js builds the preview URL as `cookie=&c2=&pet=&t1=&t2=&t3=`, with
/// an empty value for every slot that has no pick yet. Plain `Option<i64>`
/// fails on the empty string, which fails the whole extractor and 400s the
/// request — so an unfilled planner would get no preview at all.
fn empty_as_none<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(d)?;
    Ok(raw
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok()))
}

/// Query parameters the pages share. `lang` is resolved by the middleware
/// (see ctx::resolve_lang) rather than read here; it stays in the struct so
/// serde does not reject the parameter.
#[derive(Debug, Default, Deserialize)]
pub struct CommonQuery {
    #[allow(dead_code)]
    pub lang: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub page: Option<i64>,
    pub q: Option<String>,
    pub tab: Option<String>,
    pub sort: Option<String>,
    pub ep: Option<String>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub cookie: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub pet: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub treasure: Option<i64>,
    /// the option already in the slot the picker is opening for
    #[serde(default, deserialize_with = "empty_as_none")]
    pub sel: Option<i64>,
    /// the other half of the combo pair, whose partners float to the top
    #[serde(default, deserialize_with = "empty_as_none")]
    pub partner: Option<i64>,
    /// the planner's live preview posts the whole selection
    #[serde(default, deserialize_with = "empty_as_none")]
    pub c2: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t1: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t2: Option<i64>,
    #[serde(default, deserialize_with = "empty_as_none")]
    pub t3: Option<i64>,
    /// which entity list the rich-text picker wants
    pub kind: Option<String>,
    /// where /api/set-lang should return to
    pub next: Option<String>,
}
