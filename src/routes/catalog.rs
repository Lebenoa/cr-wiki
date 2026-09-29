//! The catalog list pages. Cookies, pets and treasures paginate 30 at a time
//! behind the htmx sentinel; the smaller catalogs render whole, as they do in
//! the V app.

use askama::Template;
use axum::extract::{Path, Query};
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db::{self, Card};
use crate::grade::Graded;
use crate::section::Section;
use crate::state::AppState;

use super::{errors::AppError, CommonQuery};

#[derive(Template)]
#[template(path = "catalog.html")]
struct CatalogPage {
    ctx: Ctx,
    section: String,
    /// the .tr keys are named for the section, plural
    title_key: String,
    desc_key: String,
    cards: Vec<Card>,
    next_page: i64,
    tab: String,
    tabbed: bool,
    /// the admin button's .tr key (`new_cookie_button`), named for the
    /// section's singular slug
    new_button_key: String,
    /// the list filter box's placeholder: treasures have their own wording
    filter_placeholder_key: &'static str,
    /// the simple catalogs' own rows; each kind fills exactly one of these
    episodes: Vec<db::EpisodeList>,
    ingredients: Vec<db::IngredientList>,
    jellies: Vec<db::JellyList>,
    skins: Vec<db::SkinList>,
    /// relics grouped under their owning episode, event relics last
    relic_groups: Vec<db::RelicGroup>,
}

#[derive(Template)]
#[template(path = "catalog_cards.html")]
struct CatalogCards {
    ctx: Ctx,
    section: String,
    cards: Vec<Card>,
    next_page: i64,
    tab: String,
}

/// One handler for every list: the section comes off the path, which keeps
/// the eight pages from being eight near-identical functions.
pub async fn list(
    ctx: Ctx,
    Path(section): Path<String>,
    q: Query<CommonQuery>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    render(state, ctx, section, q).await
}

async fn render(
    state: &'static AppState,
    ctx: Ctx,
    section: String,
    q: Query<CommonQuery>,
) -> Result<Response, AppError> {
    // the path segment is user input: anything unknown is a 404 rather than
    // an empty grid titled after itself
    let Some(sec) = Section::parse(&section) else {
        return Ok(super::errors::not_found(ctx));
    };
    // page 1..=10_000 keeps the deep-offset abuse away: beyond the last
    // real page the grid is empty anyway, and 10k * 30 rows already dwarfs
    // the catalog
    let page = q.page.unwrap_or(1).clamp(1, 10_000);
    let offset = page.saturating_sub(1).saturating_mul(crate::section::PAGE_SIZE);
    let tab = match q.tab.as_deref() {
        Some(t @ ("normal" | "evo")) => t.to_string(),
        _ => "all".to_string(),
    };

    let paginated = sec.paginated();
    let cards = match sec {
        Section::Cookies => db::select_cookies(&state.db, &ctx.lang, crate::section::PAGE_SIZE, offset).await?,
        Section::Pets => db::select_pets(&state.db, &ctx.lang, crate::section::PAGE_SIZE, offset).await?,
        Section::Treasures => {
            db::select_treasures(&state.db, &ctx.lang, &tab, crate::section::PAGE_SIZE, offset).await?
        }
        _ => Vec::new(),
    };
    // Simple catalogs use their own projections; only the paginated grids
    // need the shared card rows.
    let (mut episodes, mut ingredients, mut jellies, mut skins, mut relic_groups) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    match sec {
        Section::Episodes => episodes = db::select_episode_list(&state.db, &ctx.lang).await?,
        Section::Ingredients => {
            ingredients = db::select_ingredient_list(&state.db, &ctx.lang).await?;
        }
        Section::Jellies => jellies = db::select_jelly_list(&state.db, &ctx.lang).await?,
        Section::Skins => skins = db::select_skin_list(&state.db, &ctx.lang).await?,
        Section::Relics => relic_groups = db::select_relic_groups(&state.db, &ctx.lang).await?,
        _ => {}
    }

    let next_page = if paginated && i64::try_from(cards.len()).unwrap_or(0) == crate::section::PAGE_SIZE {
        page.saturating_add(1)
    } else {
        0
    };

    let html = if ctx.is_fragment() {
        CatalogCards {
            ctx,
            section: section.clone(),
            cards,
            next_page,
            tab,
        }
        .render()
    } else {
        let title_key = format!("{section}_page_title");
        let desc_key = format!("{section}_page_description");
        CatalogPage {
            ctx,
            tabbed: sec == Section::Treasures,
            new_button_key: format!("new_{}_button", sec.singular()),
            filter_placeholder_key: if sec == Section::Treasures {
                "treasure_filter_placeholder"
            } else {
                "navbar_search_placeholder"
            },
            section,
            title_key,
            desc_key,
            cards,
            next_page,
            tab,
            episodes,
            ingredients,
            jellies,
            skins,
            relic_groups,
        }
        .render()
    };
    Ok(Html(html.unwrap_or_else(|e| format!("template error: {e}"))).into_response())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// The cookie list query runs against the real database and comes back in
    /// the catalog's order. The th page still carries the English name for
    /// cross-language search.
    #[tokio::test]
    async fn cookie_query_and_render() {
        crate::i18n::load("translations");
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let rows = db::select_cookies(&pool, "en", 30, 0).await.expect("query");
        assert_eq!(rows.len(), 30);
        assert!(rows.iter().all(|c| !c.name.is_empty()));

        let th = db::select_cookies(&pool, "th", 5, 0)
            .await
            .expect("query th");
        assert_eq!(th.len(), 5);
        assert!(th.iter().all(|c| !c.en_name.is_empty()));
    }

    /// Every catalog list answers, in both locales, with the English name
    /// kept alongside for the cross-language filter.
    #[tokio::test]
    async fn catalog_queries() {
        crate::i18n::load("translations");
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let cookies = db::select_cookies(&pool, "en", 30, 0).await.unwrap();
        assert_eq!(cookies.len(), 30);
        let pets = db::select_pets(&pool, "th", 30, 0).await.unwrap();
        assert_eq!(pets.len(), 30);
        assert!(pets.iter().all(|p| !p.en_name.is_empty()));

        // the tabs partition the treasures rather than overlapping
        let all = db::select_treasures(&pool, "en", "all", 30, 0)
            .await
            .unwrap();
        let normal = db::select_treasures(&pool, "en", "normal", 30, 0)
            .await
            .unwrap();
        let evo = db::select_treasures(&pool, "en", "evo", 30, 0)
            .await
            .unwrap();
        assert_eq!(all.len(), 30);
        assert!(normal.iter().all(|t| !t.is_evolved));
        assert!(evo.iter().all(|t| t.is_evolved));

        for kind in ["episodes", "ingredients", "jellies", "skins", "relics"] {
            assert!(
                !db::select_simple(&pool, "en", kind)
                    .await
                    .unwrap()
                    .is_empty(),
                "{kind} empty"
            );
        }
    }

    #[tokio::test]
    async fn catalog_relationships_and_order() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let episodes = db::select_episode_list(&pool, "en").await.unwrap();
        let first = episodes.iter().find(|e| e.id == 1).expect("episode 1");
        assert_eq!(
            (first.stage_count, first.quest_count, first.relic_count),
            (11, 334, 3)
        );

        let ingredients = db::select_ingredient_list(&pool, "en").await.unwrap();
        let first = ingredients.first().expect("ingredient");
        assert!(first.grade.is_some(), "ungraded ingredients must sort last");
        let timber = ingredients.iter().find(|i| i.id == 1).expect("timber");
        assert_eq!(timber.recipe_count, 3);
        assert!(ingredients.iter().rev().take(10).all(|i| i.grade.is_none()));

        let skins = db::select_skin_list(&pool, "en").await.unwrap();
        let ginger = skins
            .iter()
            .find(|s| s.id == 1_800_001)
            .expect("GingerBrave skin");
        assert_eq!(
            (
                ginger.owner_id,
                ginger.owner_name.as_str(),
                ginger.owner_section
            ),
            (1, "GingerBrave", "cookies")
        );

        let relics = db::select_relic_groups(&pool, "en").await.unwrap();
        assert!(relics
            .iter()
            .any(|g| g.episode_id > 0 && !g.episode_name.is_empty()));
        assert!(relics
            .iter()
            .flat_map(|g| &g.relics)
            .any(|r| r.unlock_cookie_id == 64 && r.unlock_cookie_name == "Sea Fairy Cookie"));
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
