//! The side collections a detail page shows beside its main row. Every one of
//! them reads only `(section, id)` and the already-loaded row, so they all go
//! out at once: the page's latency is the slowest one round trip, not the sum
//! of ten of them. Each block still answers the default its kind's page
//! ignores, exactly as the sequential version did.

use crate::db::{self, CombiRow, EffectLine, TreasureLinks};
use crate::section::Section;

use crate::routes::errors::AppError;

/// The ten collections; a kind leaves the ones it does not use at their
/// defaults.
pub(super) struct Extras {
    pub effects: Vec<EffectLine>,
    pub combi: Vec<CombiRow>,
    pub links: TreasureLinks,
    /// the treasure this cookie or pet unlocks: id, name, image
    pub unlocks: Option<(i64, String, Option<String>)>,
    /// an ingredient's drop/economy tile fields
    pub ingredient: db::IngredientFacts,
    /// the treasures this ingredient crafts
    pub recipes: Vec<db::CraftRecipe>,
    /// the ingredients this treasure is crafted from
    pub craft: Vec<db::CraftIngredient>,
    /// the linked variant: the base for an evolved row, the evolved form for
    /// a normal one
    pub variant: Option<(i64, String, Option<String>)>,
    /// an episode's child sections; empty for every other kind
    pub episode: db::EpisodeDetail,
    /// a jelly's producers; empty for every other kind
    pub makers: Vec<db::JellyMaker>,
}

pub(super) async fn extras(
    db: &crate::db::Db,
    lang: &str,
    sec: Section,
    id: i64,
    is_evolved: bool,
) -> Result<Extras, AppError> {
    let (effects, combi, links, unlocks, ingredient, recipes, craft, variant, episode, makers) = tokio::join!(
        async {
            if sec == Section::Treasures {
                db::treasure_effects(db, lang, id).await
            } else {
                Ok(Vec::new())
            }
        },
        async { db::combi_bonuses(db, lang, sec.as_str(), id).await },
        async {
            if sec == Section::Treasures {
                db::treasure_links(db, lang, id).await
            } else {
                Ok(TreasureLinks::default())
            }
        },
        // the reverse link: what this cookie or pet unlocks
        async {
            match sec {
                Section::Cookies | Section::Pets => {
                    db::unlocked_treasure(db, lang, sec.table(), id).await
                }
                _ => Ok(None),
            }
        },
        async {
            if sec == Section::Ingredients {
                db::ingredient_facts(db, lang, id).await
            } else {
                Ok(db::IngredientFacts::default())
            }
        },
        async {
            if sec == Section::Ingredients {
                db::ingredient_recipes(db, lang, id).await
            } else {
                Ok(Vec::new())
            }
        },
        async {
            if sec == Section::Treasures {
                db::treasure_craft_ingredients(db, lang, id).await
            } else {
                Ok(Vec::new())
            }
        },
        // the linked variant: base of an evolved row, evolved form of a normal
        // one
        async {
            if sec == Section::Treasures {
                db::treasure_variant(db, lang, id, is_evolved).await
            } else {
                Ok(None)
            }
        },
        async {
            if sec == Section::Episodes {
                db::episode_extras(db, lang, id).await
            } else {
                Ok(db::EpisodeDetail::default())
            }
        },
        async {
            if sec == Section::Jellies {
                db::jelly_makers(db, lang, id).await
            } else {
                Ok(Vec::new())
            }
        },
    );
    Ok(Extras {
        effects: effects?,
        combi: combi?,
        links: links?,
        unlocks: unlocks?,
        ingredient: ingredient?,
        recipes: recipes?,
        craft: craft?,
        variant: variant?,
        episode: episode?,
        makers: makers?,
    })
}
