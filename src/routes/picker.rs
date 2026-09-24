//! `/builds/options/:kind` — one page of a picker dialog's option grid.
//!
//! Search and the treasure tabs are applied here rather than by hiding DOM
//! nodes, because the grid is paginated and the browser only holds the pages
//! it has scrolled through. The lists themselves are cached per language, so
//! a keystroke costs a filter over a slice and a template render, no queries.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db;
use crate::grade::Graded;
use crate::options::{self, PickerOption};
use crate::pagination::slice_page;
use crate::state::AppState;

use super::{errors::AppError, CommonQuery};

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
    // askama passes loop variables by reference, so the id arrives as &i64
    #[allow(clippy::trivially_copy_pass_by_ref)]
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

fn next_url(
    kind: &str,
    lang: &str,
    q: &str,
    tab: &str,
    sel: i64,
    partner: i64,
    page: i64,
) -> String {
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
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b));
            }
            b' ' => out.push('+'),
            _ => {
                let hi = usize::from(b / 16);
                let lo = usize::from(b % 16);
                // a byte is two nibbles; both indices stay under 16
                out.push('%');
                out.push(char::from(*HEX.get(hi).unwrap_or(&b'0')));
                out.push(char::from(*HEX.get(lo).unwrap_or(&b'0')));
            }
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
    let wanted: Vec<(&'static str, &'static str, i64)> = vec![
        ("cookie", "cookies", q.cookie.unwrap_or(0)),
        ("cookie", "cookies", q.c2.unwrap_or(0)),
        ("pet", "pets", q.pet.unwrap_or(0)),
        ("treasure", "treasures", q.t1.unwrap_or(0)),
        ("treasure", "treasures", q.t2.unwrap_or(0)),
        ("treasure", "treasures", q.t3.unwrap_or(0)),
    ];
    let mut picks = Vec::new();
    for (kind, section, id) in wanted {
        if id <= 0 {
            continue;
        }
        if let Some((name, image)) = db::entity_link(&state.db, &ctx.lang, kind, id).await {
            picks.push(PickedSlot {
                section,
                name,
                image,
            });
        }
    }

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
) -> Result<Response, AppError> {
    if !matches!(kind.as_str(), "cookie" | "pet" | "treasure") {
        return Ok(super::errors::not_found(ctx));
    }
    let lang = ctx.lang.clone();
    let raw_q = q.q.clone().unwrap_or_default().trim().to_lowercase();
    let terms: Vec<String> = raw_q.split_whitespace().map(str::to_string).collect();
    let tab = match q.tab.as_deref() {
        Some(t @ ("normal" | "evo")) => t.to_string(),
        _ => "all".to_string(),
    };
    let page = q.page.unwrap_or(1).max(1);
    let sel = q.sel.unwrap_or(0);
    let partner = q.partner.unwrap_or(0);

    let all = options::options(&state.db, &ctx.lang, &kind).await?;
    // the combo partners of the other slot's pick: a combo is the main
    // reason a planner picks one pet over another, and the grid is too
    // long to hunt through
    let combi = if partner > 0 && kind != "treasure" {
        let partner_kind = if kind == "pet" { "cookies" } else { "pets" };
        db::combi_partner_ids(&state.db, partner_kind, partner).await?
    } else {
        Vec::new()
    };

    let mut matched: Vec<PickerOption> = all
        .iter()
        .filter(|o| tab_ok(o, &tab) && matches(o, &terms))
        .cloned()
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
                next_url(
                    &kind,
                    &lang,
                    &raw_q,
                    &tab,
                    sel,
                    partner,
                    page.saturating_add(1),
                )
            } else {
                String::new()
            };
            let window = matched
                .get(start..end)
                .map(<[PickerOption]>::to_vec)
                .unwrap_or_default();
            (window, next)
        }
        None if page > 1 => {
            // scrolled past the end (a stale sentinel): nothing to append
            return Ok(Html(String::new()).into_response());
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
    Ok(Html(
        grid.render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {

    /// The picker lists build, cache and carry their effect ladders.
    #[tokio::test]
    async fn picker_options_build() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let cookies = crate::options::options(&pool, "en", "cookie")
            .await
            .expect("cookie options");
        assert_eq!(cookies.len(), 94);
        let pets = crate::options::options(&pool, "en", "pet")
            .await
            .expect("pet options");
        assert_eq!(pets.len(), 101, "the two phantom Sotdae pets are gone");

        let treasures = crate::options::options(&pool, "en", "treasure")
            .await
            .expect("treasure options");
        assert!(treasures.len() > 700);
        // Power+ treasures are friendly-run bonuses and cannot be equipped
        assert!(treasures.iter().any(|t| !t.effects.is_empty()));
        let with_ladder = treasures
            .iter()
            .find(|t| t.effects.iter().any(|e| !e.values.is_empty()))
            .expect("a treasure with a ladder");
        assert_eq!(
            with_ladder
                .effects
                .iter()
                .find(|e| !e.values.is_empty())
                .unwrap()
                .values
                .len(),
            10
        );

        // the grade order puts the highest first, and E outranks L
        let ranks: Vec<i64> = treasures
            .iter()
            .filter_map(|t| t.grade)
            .map(crate::grade::rank)
            .collect();
        assert!(
            ranks.windows(2).all(|w| w[0] >= w[1]),
            "grade order is not monotonic"
        );

        // second call comes from the cache
        let again = crate::options::options(&pool, "en", "cookie")
            .await
            .expect("cookie options");
        assert_eq!(again.len(), cookies.len());
        crate::options::invalidate();
        assert_eq!(
            crate::options::options(&pool, "en", "cookie")
                .await
                .expect("cookie options")
                .len(),
            cookies.len()
        );
    }

    /// The DB handle behind the gated tests: unset means they skip, so
    /// `cargo test` stays green without a server. Points at a scratch
    /// namespace/database — never at data you cannot lose.
    async fn live_db() -> Option<crate::db::Db> {
        let url = std::env::var("CR_SURREAL_URL").ok()?;
        let ns = std::env::var("CR_SURREAL_NS").unwrap_or_else(|_| "cookierun".into());
        let database = std::env::var("CR_SURREAL_DB").unwrap_or_else(|_| "cookierun".into());
        let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
        crate::db::connect_url(&url, &ns, &database, &user, &pass)
            .await
            .ok()
    }
}
