//! Detail pages. One handler across the kinds: they differ in which prose
//! fields they carry and whether they show effects or combo bonuses, which
//! the template decides from the section.

use askama::Template;
use axum::extract::Path;
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db::{self, CombiRow, Detail, EffectLine, TreasureLinks};
use crate::grade::Graded;
use crate::richtext;
use crate::section::Section;

use super::errors::AppError;

#[derive(Template)]
#[template(path = "detail.html")]
struct DetailPage {
    ctx: Ctx,
    section: String,
    item: Detail,
    /// prose already rendered through the rich-text markup
    abilities_html: String,
    description_html: String,
    power_plus_html: String,
    power_plus_requirement_html: String,
    unlock_goal_html: String,
    effects: Vec<EffectLine>,
    /// true when the blessed set is worth a toggle, i.e. it exists and is
    /// not identical to the normal one
    blessed_differs: bool,
    combi: Vec<CombiRow>,
    links: TreasureLinks,
    /// the treasure this cookie or pet unlocks: id, name, image
    unlocks: Option<(i64, String, Option<String>)>,
    /// an ingredient's drop/economy tile fields
    ingredient: db::IngredientFacts,
    /// the treasures this ingredient crafts
    recipes: Vec<db::CraftRecipe>,
    /// the ingredients this treasure is crafted from
    craft: Vec<db::CraftIngredient>,
    /// the linked variant: the base for an evolved row, the evolved form for
    /// a normal one
    variant: Option<(i64, String, Option<String>)>,
    /// an episode's child sections; empty for every other kind
    episode: db::EpisodeDetail,
    /// a jelly's producers; empty for every other kind
    makers: Vec<db::JellyMaker>,
}

impl DetailPage {
    /// The section for template expressions, which read `self.` fields.
    fn section(&self) -> &str {
        &self.section
    }

    /// The site URL likewise; askama expressions inside `{% call %}` see
    /// only `self` fields, not the handler's locals.
    fn site_url(&self) -> &str {
        &self.ctx.site_url
    }

    /// The section's own edit route, shown to an admin only.
    fn can_edit(&self) -> bool {
        self.ctx.is_admin()
    }
}

pub async fn show(
    ctx: Ctx,
    Path((section, id)): Path<(String, i64)>,
) -> Result<Response, AppError> {
    let state = crate::state::state();
    let Some(sec) = Section::parse(&section) else {
        return Ok(super::errors::not_found(ctx));
    };
    let item = db::select_detail(&state.db, &ctx.lang, &section, id).await?;
    let Some(item) = item else {
        return Ok(super::errors::not_found(ctx));
    };
    let effects = if sec == Section::Treasures {
        db::treasure_effects(&state.db, &ctx.lang, id).await?
    } else {
        Vec::new()
    };
    let combi = db::combi_bonuses(&state.db, &ctx.lang, &section, id).await?;
    let links = if sec == Section::Treasures {
        db::treasure_links(&state.db, &ctx.lang, id).await?
    } else {
        TreasureLinks::default()
    };
    // the reverse link: what this cookie or pet unlocks
    let unlocks = match sec {
        Section::Cookies | Section::Pets => {
            db::unlocked_treasure(&state.db, &ctx.lang, sec.table(), id).await?
        }
        _ => None,
    };
    // the kinds' own collections; each fills only what its page shows
    let ingredient = if sec == Section::Ingredients {
        db::ingredient_facts(&state.db, &ctx.lang, id).await?
    } else {
        db::IngredientFacts::default()
    };
    let recipes = if sec == Section::Ingredients {
        db::ingredient_recipes(&state.db, &ctx.lang, id).await?
    } else {
        Vec::new()
    };
    let craft = if sec == Section::Treasures {
        db::treasure_craft_ingredients(&state.db, &ctx.lang, id).await?
    } else {
        Vec::new()
    };
    // the linked variant: base of an evolved row, evolved form of a normal one
    let variant = if sec == Section::Treasures {
        db::treasure_variant(&state.db, &ctx.lang, id, item.is_evolved).await?
    } else {
        None
    };
    // one link cache across all five prose fields: an entity named twice on
    // one page costs one lookup
    let mut memo = richtext::LinkCache::new();
    let abilities_html =
        richtext::render_with(&state.db, &ctx.lang, &item.abilities, &mut memo).await;
    let description_html =
        richtext::render_with(&state.db, &ctx.lang, &item.description, &mut memo).await;
    let power_plus_html =
        richtext::render_with(&state.db, &ctx.lang, &item.power_plus, &mut memo).await;
    let power_plus_requirement_html = richtext::render_with(
        &state.db,
        &ctx.lang,
        &item.power_plus_requirement,
        &mut memo,
    )
    .await;
    let unlock_goal_html =
        richtext::render_with(&state.db, &ctx.lang, &item.unlock_goal, &mut memo).await;

    let blessed_differs = db::blessed_differs(&effects);
    let episode = if sec == Section::Episodes {
        db::episode_extras(&state.db, &ctx.lang, id).await?
    } else {
        db::EpisodeDetail::default()
    };
    let makers = if sec == Section::Jellies {
        db::jelly_makers(&state.db, &ctx.lang, id).await?
    } else {
        Vec::new()
    };
    let page = DetailPage {
        ctx,
        section,
        item,
        abilities_html,
        description_html,
        power_plus_html,
        power_plus_requirement_html,
        unlock_goal_html,
        effects,
        blessed_differs,
        combi,
        links,
        unlocks,
        ingredient,
        recipes,
        craft,
        variant,
        episode,
        makers,
    };
    Ok(Html(
        page.render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// Rich text: links resolve with their sprite, colours are constrained,
    /// and everything else is escaped. Lives beside the detail page because
    /// that is the surface the rendered prose reaches.
    #[tokio::test]
    async fn richtext_renders() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let mut memo = richtext::LinkCache::new();
        let out = richtext::render_with(&pool, "en", "see [[89]] here", &mut memo).await;
        assert!(out.contains("href=\"/cookies/89\""));
        assert!(out.contains("<img src=\"/static/img/cookies/"));

        // an unresolvable ref stays literal
        let miss = richtext::render_with(&pool, "en", "[[cookie:99999999]]", &mut memo).await;
        assert!(miss.contains("[[cookie:99999999]]"));

        let colored =
            richtext::render_with(&pool, "en", "a {color:red}red{/color} word", &mut memo).await;
        assert!(colored.contains("<span style=\"color:red\">red</span>"));

        // an injection attempt is not a valid colour, so the whole thing
        // renders as text: no span is opened and the quotes come out escaped
        let bad =
            richtext::render_with(&pool, "en", "{color:red\" onclick=\"x}y{/color}", &mut memo)
                .await;
        assert!(!bad.contains("<span style="), "{bad}");
        assert!(!bad.contains("onclick=\""), "{bad}");
        assert!(bad.contains("&quot;"), "{bad}");

        // pasted markup is escaped
        let script =
            richtext::render_with(&pool, "en", "<script>alert(1)</script>", &mut memo).await;
        assert!(!script.contains("<script>"));
        assert!(script.contains("&lt;script&gt;"));
    }

    /// A treasure's unlock chain resolves to the entity that grants it.
    #[tokio::test]
    async fn treasure_links_resolve() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        // treasure 255 (Banana Lion Tail) is unlocked by pet 77, Banana Lion
        let links = db::treasure_links(&pool, "en", 255).await.expect("links");
        assert!(links.has_unlock());
        assert_eq!(links.unlock_section, "pets");
        assert_eq!(links.unlock_id, 77);
        assert!(!links.unlock_name.is_empty());

        // relics and skins have detail rows now. Their ids are not 1-based —
        // relics start at 500001 and skins at 1800001 — so the test takes an
        // id from the list rather than assuming one.
        for section in ["relics", "skins"] {
            let list = db::select_simple(&pool, "en", section).await.unwrap();
            let first = list.first().expect("a row");
            let detail = db::select_detail(&pool, "en", section, first.id)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{section} {} has no detail", first.id));
            assert!(!detail.name.is_empty());
        }
    }

    /// A detail row carries the prose its kind has and nothing else.
    #[tokio::test]
    async fn detail_rows() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let cookie = db::select_detail(&pool, "en", "cookies", 89)
            .await
            .unwrap()
            .expect("cookie 89");
        assert!(!cookie.name.is_empty());
        assert!(!cookie.abilities.is_empty());

        // a treasure has no abilities column, and effects come with ladders
        let treasure = db::select_detail(&pool, "en", "treasures", 317)
            .await
            .unwrap()
            .expect("treasure 317");
        assert!(treasure.abilities.is_empty());
        let effects = db::treasure_effects(&pool, "en", 317).await.unwrap();
        assert!(!effects.is_empty());
        assert!(
            effects.iter().all(|e| e.values.len() == 10),
            "ten levels per effect"
        );

        assert!(db::select_detail(&pool, "en", "cookies", 99999)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn ingredient_crafting_and_treasure_variants() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };
        let recipes = db::ingredient_recipes(&pool, "en", 1).await.unwrap();
        assert_eq!(recipes.len(), 3);
        assert!(recipes
            .iter()
            .any(|r| r.treasure_id == 56 && r.name == "Mocking Carnival Mask"));

        let ingredients = db::treasure_craft_ingredients(&pool, "en", 56)
            .await
            .unwrap();
        assert!(ingredients
            .iter()
            .any(|i| i.ingredient_id == 1 && i.name == "Timber Board"));

        let base = db::treasure_variant(&pool, "en", 3, true)
            .await
            .unwrap()
            .expect("base");
        assert_eq!(base.0, 8);
        let evolved = db::treasure_variant(&pool, "en", 8, false)
            .await
            .unwrap()
            .expect("evolved");
        assert_eq!(evolved.0, 3);
    }

    /// The blessed toggle appears only when the blessed set actually differs
    /// from the normal one — a treasure whose two sets match should not offer
    /// a switch between identical readings.
    #[test]
    fn blessed_toggle_only_when_it_differs() {
        use db::EffectLine;
        let line = |text: &str, v: &[&str], blessed: bool| EffectLine {
            text: text.into(),
            values: v.iter().map(ToString::to_string).collect(),
            blessed,
        };

        // no blessed set at all
        assert!(!db::blessed_differs(&[line("Magnet", &["1"], false)]));
        // identical sets
        assert!(!db::blessed_differs(&[
            line("Magnet", &["1"], false),
            line("Magnet", &["1"], true),
        ]));
        // a different value is a difference worth showing
        assert!(db::blessed_differs(&[
            line("Magnet", &["1"], false),
            line("Magnet", &["2"], true),
        ]));
        // so is a different effect
        assert!(db::blessed_differs(&[
            line("Magnet", &["1"], false),
            line("Revive", &["1"], true),
        ]));
    }

    /// The combo editor lists a pairing with the row id it needs to remove it.
    #[tokio::test]
    async fn combi_editor_rows() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let rows = db::combi_edit_rows(&pool, "en", "cookies", 89)
            .await
            .expect("rows");
        assert!(!rows.is_empty(), "cookie 89 pairs with a pet");
        assert!(
            rows.iter().all(|r| r.id > 0),
            "every row carries its own id"
        );
        assert!(rows.iter().all(|r| !r.partner_name.is_empty()));
        assert!(db::combi_edit_rows(&pool, "en", "treasures", 1)
            .await
            .unwrap()
            .is_empty());
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
