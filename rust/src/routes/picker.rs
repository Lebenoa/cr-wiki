//! `/builds/options/:kind` — one page of a picker dialog's option grid.
//!
//! Search and the treasure tabs are applied here rather than by hiding DOM
//! nodes, because the grid is paginated and the browser only holds the pages
//! it has scrolled through. The lists themselves are cached per language, so
//! a keystroke costs a filter over a slice and a template render, no queries.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db;
use crate::options::{self, PickerOption};
use crate::pagination::slice_page;
use crate::state::AppState;

use super::CommonQuery;

pub const PAGE_SIZE: i64 = 30;

#[derive(Template)]
#[template(path = "picker_options.html")]
struct PickerGrid {
    ctx: Ctx,
    /// the section directory the sprites come from
    dir: String,
    options: Vec<PickerOption>,
    /// the id already in the slot, pinned first and ringed
    sel: i64,
    /// ids that pair with the other slot's pick
    combi: Vec<i64>,
    next_url: String,
    page: i64,
}

impl PickerGrid {
    fn is_combi(&self, id: &i64) -> bool {
        self.combi.contains(id)
    }
}

/// Every term has to appear somewhere in the option, though not in the same
/// place: "magnet revive" finds the treasures carrying a magnet effect *and*
/// a revive effect, which a single substring test could never match because
/// no one field holds both. Effect values are left out — they move with the
/// level slider, so what the card shows is not a stable thing to search.
fn haystack(opt: &PickerOption) -> String {
    let mut s = String::with_capacity(64);
    s.push_str(&opt.name);
    s.push(' ');
    s.push_str(&opt.en_name);
    for e in opt.effects.iter().chain(opt.effects_blessed.iter()) {
        s.push(' ');
        s.push_str(&e.text);
    }
    s.to_lowercase()
}

fn matches(opt: &PickerOption, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let hay = haystack(opt);
    terms.iter().all(|t| hay.contains(t.as_str()))
}

/// The treasure picker's all/normal/evolved tabs. Cookies and pets have none
/// and always pass "all".
fn tab_ok(opt: &PickerOption, tab: &str) -> bool {
    match tab {
        "normal" => !opt.is_evolved,
        "evo" => opt.is_evolved,
        _ => true,
    }
}

fn next_url(kind: &str, lang: &str, q: &str, tab: &str, sel: i64, partner: i64, page: i64) -> String {
    format!(
        "/builds/options/{kind}?lang={}&q={}&tab={}&sel={sel}&partner={partner}&page={page}",
        urlencode(lang),
        urlencode(q),
        urlencode(tab)
    )
}

/// Percent-encodes the few characters a query value can carry that would
/// otherwise end the parameter.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Template)]
#[template(path = "build_preview.html")]
struct PreviewFragment {
    ctx: Ctx,
    picks: Vec<PickedSlot>,
}

/// One resolved slot in the live preview.
pub struct PickedSlot {
    pub section: &'static str,
    pub name: String,
    pub image: Option<String>,
}

/// `/builds/preview` — the planner re-renders the loadout after each pick by
/// fetching this with the current selection in the query string.
pub async fn preview(
    State(state): State<AppState>,
    ctx: Ctx,
    q: Query<CommonQuery>,
) -> Html<String> {
    let lang = ctx.lang.clone();
    let wanted: Vec<(&'static str, &'static str, i64)> = vec![
        ("cookie", "cookies", q.cookie.unwrap_or(0)),
        ("cookie", "cookies", q.c2.unwrap_or(0)),
        ("pet", "pets", q.pet.unwrap_or(0)),
        ("treasure", "treasures", q.t1.unwrap_or(0)),
        ("treasure", "treasures", q.t2.unwrap_or(0)),
        ("treasure", "treasures", q.t3.unwrap_or(0)),
    ];
    let db = state.db.clone();
    let picks = tokio::task::spawn_blocking(move || {
        wanted
            .into_iter()
            .filter(|(_, _, id)| *id > 0)
            .filter_map(|(kind, section, id)| {
                db::entity_link(&db, &lang, kind, id).map(|(name, image)| PickedSlot {
                    section,
                    name,
                    image,
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();

    Html(
        PreviewFragment { ctx, picks }
            .render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
}

pub async fn options_grid(
    State(state): State<AppState>,
    ctx: Ctx,
    Path(kind): Path<String>,
    q: Query<CommonQuery>,
) -> Response {
    if !matches!(kind.as_str(), "cookie" | "pet" | "treasure") {
        return super::errors::not_found(ctx);
    }
    let lang = ctx.lang.clone();
    let raw_q = q.q.clone().unwrap_or_default().trim().to_lowercase();
    let terms: Vec<String> = raw_q.split_whitespace().map(|s| s.to_string()).collect();
    let tab = match q.tab.as_deref() {
        Some(t @ ("normal" | "evo")) => t.to_string(),
        _ => "all".to_string(),
    };
    let page = q.page.unwrap_or(1).max(1);
    let sel = q.sel.unwrap_or(0);
    let partner = q.partner.unwrap_or(0);

    let db_pool = state.db.clone();
    let kind_for_db = kind.clone();
    let lang_for_db = lang.clone();
    let built = tokio::task::spawn_blocking(move || {
        let all = options::options(&db_pool, &lang_for_db, &kind_for_db);
        // the combo partners of the other slot's pick: a combo is the main
        // reason a planner picks one pet over another, and the grid is too
        // long to hunt through
        let combi = if partner > 0 && kind_for_db != "treasure" {
            let partner_kind = if kind_for_db == "pet" { "cookies" } else { "pets" };
            db::combi_partner_ids(&db_pool, partner_kind, partner).unwrap_or_default()
        } else {
            Vec::new()
        };
        (all, combi)
    })
    .await;

    let Ok((all, combi)) = built else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "options unavailable").into_response();
    };

    let mut matched: Vec<PickerOption> = all
        .into_iter()
        .filter(|o| tab_ok(o, &tab) && matches(o, &terms))
        .collect();

    // combo partners float above the rest, then the slot's own pick is
    // pinned to the very front. Both happen before the slice and on every
    // page, so the pages stay a clean cut of one stable ordering.
    if !combi.is_empty() {
        let (mut pairs, rest): (Vec<_>, Vec<_>) =
            matched.into_iter().partition(|o| combi.contains(&o.id));
        pairs.extend(rest);
        matched = pairs;
    }
    if sel > 0 {
        if let Some(at) = matched.iter().position(|o| o.id == sel) {
            let pinned = matched.remove(at);
            matched.insert(0, pinned);
        }
    }

    let total = matched.len();
    let (options_page, next) = match slice_page(page, PAGE_SIZE, total) {
        Some((start, end)) => {
            let next = if end < total {
                next_url(&kind, &lang, &raw_q, &tab, sel, partner, page + 1)
            } else {
                String::new()
            };
            (matched[start..end].to_vec(), next)
        }
        None if page > 1 => {
            // scrolled past the end (a stale sentinel): nothing to append
            return Html(String::new()).into_response();
        }
        None => (Vec::new(), String::new()),
    };

    let dir = match kind.as_str() {
        "pet" => "pets",
        "treasure" => "treasures",
        _ => "cookies",
    };
    let grid = PickerGrid {
        ctx,
        dir: dir.to_string(),
        options: options_page,
        sel,
        combi,
        next_url: next,
        page,
    };
    Html(grid.render().unwrap_or_else(|e| format!("template error: {e}"))).into_response()
}
