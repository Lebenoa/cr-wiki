//! `SurrealDB` access. The app never embeds a storage engine: every query goes
//! over the wire to the server named by `Config.toml`'s `[surreal]` section.
//!
//! The module is a facade: the submodules below own one concern each
//! (connection, the shared row vocabulary, and one file per read/write family),
//! and everything they expose is re-exported here, so callers keep writing
//! `crate::db::select_cookies` without knowing how the file is arranged.
//!
//! Modeling notes (this is a document store, not SQL):
//! - translations live NESTED inside each entity as `tr: { en: {...}, th:
//!   {...} }`, so the locale-fallback joins collapse into a path read:
//!   `(tr[$lang].name ?? tr.en.name)`
//! - a treasure's effect lines are denormalized onto the record as
//!   `effect_lines: [{ state, en, th, values: [..+0..+9] }]`
//! - ids are numeric (`cookie:123`); `record::id(id)` projects them back to
//!   plain integers so routes keep their `/section/:id` shape
//! - graded entities carry a maintained `rank` column (`grade::rank`),
//!   because ordering by display rank must happen in the database

use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

use std::fmt::Write as _;

use crate::config::SurrealConfig;
use crate::section::Section;

pub type Db = Surreal<Any>;

pub type Result<T> = surrealdb::Result<T>;

/// Connects to the external server and selects the namespace/database. No
/// local engine exists behind this — an unreachable URL is a hard startup
/// error.
pub async fn connect(cfg: &SurrealConfig) -> Result<Db> {
    connect_url(
        &cfg.url,
        &cfg.namespace,
        &cfg.database,
        &cfg.username,
        &cfg.password,
    )
    .await
}

/// Connects to an external server (`ws://` or `http://`). No local engine
/// exists behind this — an unreachable URL is a hard startup error.
pub async fn connect_url(
    url: &str,
    ns: &str,
    database: &str,
    username: &str,
    password: &str,
) -> Result<Db> {
    let db = surrealdb::engine::any::connect(url).await?;
    if !username.is_empty() {
        db.signin(Root {
            username: username.to_string(),
            password: password.to_string(),
        })
        .await?;
    }
    db.use_ns(ns).use_db(database).await?;
    ensure_search_schema(&db).await?;
    ensure_sequences(&db).await?;
    ensure_reset_schema(&db).await?;
    ensure_app_tables(&db).await?;
    sessions::ensure_schema(&db).await?;
    stamp::ensure(&db).await?;
    ensure_verified_backfill(&db).await?;
    Ok(db)
}

/// One throwaway round trip so the wire pool's first connection is already
/// established and authenticated before real traffic arrives. The SDK builds
/// its WebSocket lazily; without this the first request in a fresh process
/// pays the whole setup while its caller waits.
pub async fn warm_pool(db: &Db) -> Result<()> {
    let mut res = db.query("RETURN 1;").await?;
    let _: Option<i64> = res.take(0)?;
    Ok(())
}

/// The tables the search schema covers, each with the translation fields its
/// full-text indexes serve.
const SEARCH_TABLES: [(&str, &[&str]); 6] = [
    (
        "cookie",
        &[
            "tr.en.name",
            "tr.th.name",
            "tr.en.abilities",
            "tr.th.abilities",
            "tr.en.description",
            "tr.th.description",
        ],
    ),
    (
        "pet",
        &[
            "tr.en.name",
            "tr.th.name",
            "tr.en.description",
            "tr.th.description",
        ],
    ),
    (
        "treasure",
        &[
            "tr.en.name",
            "tr.th.name",
            "tr.en.description",
            "tr.th.description",
        ],
    ),
    (
        "relic",
        &[
            "tr.en.name",
            "tr.th.name",
            "tr.en.description",
            "tr.th.description",
        ],
    ),
    (
        "episode",
        &[
            "tr.en.name",
            "tr.th.name",
            "tr.en.description",
            "tr.th.description",
        ],
    ),
    (
        "ingredient",
        &[
            "tr.en.name",
            "tr.th.name",
            "tr.en.description",
            "tr.th.description",
        ],
    ),
];

/// Ensures the indexes needed by search exist for seeded and upgraded databases.
/// The application authenticates with the deployment's configured root user.
///
/// Also defines the per-table name-gram ingest events: whenever an entity's
/// translation lands (CREATE with tr, or the UPDATE `write_translation` sends
/// right after), the event replaces that entity's `name_gram` rows — old
/// name's grams deleted, new name's 3-grams inserted. seed.surql seeds the
/// rows themselves (events never fire on `surreal import`); this keeps
/// admin-created and renamed entities fresh on live databases.
async fn ensure_search_schema(db: &Db) -> Result<()> {
    db.query(
        "DEFINE ANALYZER IF NOT EXISTS cookie_search_en TOKENIZERS class, punct FILTERS lowercase;",
    )
    .await?
    .check()?;
    db.query("DEFINE ANALYZER IF NOT EXISTS cookie_search_th TOKENIZERS class FILTERS lowercase, ngram(1,3);")
        .await?
        .check()?;
    for (table, fields) in SEARCH_TABLES {
        for (index, field) in fields.iter().enumerate() {
            let analyzer = if field.contains(".th.") {
                "cookie_search_th"
            } else {
                "cookie_search_en"
            };
            let sql = format!(
                "DEFINE INDEX IF NOT EXISTS {table}_search_{index} ON TABLE {table} FIELDS {field} FULLTEXT ANALYZER {analyzer};"
            );
            db.query(sql).await?.check()?;
        }
    }
    ensure_gram_events(db).await
}

/// The name-gram ingest events. The guard accepts any event that leaves
/// an en name behind — CREATE-with-tr and `write_translation`'s UPDATE —
/// and the body first deletes the entity's existing gram rows, so a
/// rename cannot leave stale grams behind. seed.surql defines the same
/// statements for fresh imports.
async fn ensure_gram_events(db: &Db) -> Result<()> {
    for (table, _) in SEARCH_TABLES {
        let sql = format!(
            "DEFINE EVENT IF NOT EXISTS {table}_gram ON TABLE {table} \
             WHEN ($after.tr.en.name ?? '') != '' THEN {{ \
             FOR $g IN (SELECT VALUE gram FROM name_gram \
                        WHERE section = '{table}' AND entity_id = record::id($after.id)) {{ \
             DELETE name_gram WHERE section = '{table}' AND gram = $g \
                AND entity_id = record::id($after.id); }}; \
             FOR $g IN array::windows(array::filter(string::split(string::lowercase($after.tr.en.name), ''), |$c| $c != ''), 3) \
                       .map(|$w| array::join($w, '')) {{ \
             CREATE name_gram SET gram = $g, section = '{table}', \
                entity_id = record::id($after.id); }} }};"
        );
        db.query(sql).await?.check()?;
    }
    Ok(())
}
/// Defines every id sequence the app allocates from, past the seeded max
/// ids (see seed.surql, which carries the same definitions). `IF NOT
/// EXISTS` keeps restarts and already-seeded databases untouched; the
/// application never resizes a live sequence.
async fn ensure_sequences(db: &Db) -> Result<()> {
    let mut query = String::new();
    for table in Section::ALL
        .iter()
        .filter(|s| s.editable())
        .map(|s| s.table())
    {
        // user, build and news carry SurrealDB's random default record ids —
        // their pages are public and must not be enumerable — so no sequence
        // is defined for them
        // `writeln!` over `push_str(&format!..)`: nothing to reallocate twice
        let _ = writeln!(query, "DEFINE SEQUENCE IF NOT EXISTS {table}_seq START 1;");
    }
    db.query(query).await?.check()?;
    Ok(())
}

/// Defines the news and build tables. Created on connect rather than left to
/// the seed: a database that predates the surface still has to serve it, and
/// `IF NOT EXISTS` leaves a seeded one alone. The tables are schemaless like
/// `user`; their rows are only ever created by the app. On `SurrealDB` 4 a
/// SELECT against an undefined table is an error (`The table 'x' does not
/// exist`) where 3.x answered an empty list — an empty deployment must still
/// render the build list.
///
/// The `verified_count` column is the sort=verified key: one vote moved per
/// review event (the `review_verified` event defined below), maintained by
/// the database, so
/// the list page can `ORDER BY verified_count DESC` server-side and let the
/// index answer the sort. A live `SurrealDB` 3.3 answer would have needed to
/// fetch every build and count reviews per row (a `$parent`-correlated
/// subquery re-planned per row); on 4.0 the correlated aggregate still plans
/// a per-row table scan, so a maintained counter remains the only shape that
/// sorts server-side. `ensure_verified_backfill` fills the column on
/// databases whose builds predate the event.
async fn ensure_app_tables(db: &Db) -> Result<()> {
    db.query(
        "DEFINE TABLE IF NOT EXISTS news SCHEMALESS; \
         DEFINE TABLE IF NOT EXISTS build SCHEMALESS; \
         DEFINE TABLE IF NOT EXISTS review SCHEMALESS;",
    )
    .await?
    .check()?;
    db.query(
        "DEFINE FIELD IF NOT EXISTS verified_count ON build TYPE int DEFAULT 0; \
         DEFINE INDEX IF NOT EXISTS build_verified_count ON build FIELDS verified_count; \
         DEFINE EVENT IF NOT EXISTS review_verified ON TABLE review \
         WHEN $event = 'CREATE' \
            OR $event = 'DELETE' \
            OR ($event = 'UPDATE' AND $before.verified != $after.verified) \
         THEN { \
           IF $event = 'DELETE' { \
             UPDATE $before.build_id SET verified_count -= \
               (IF $before.verified THEN 1 ELSE 0 END); \
           } ELSE IF $event = 'UPDATE' { \
             UPDATE $after.build_id SET verified_count += \
               (IF $after.verified THEN 1 ELSE -1 END); \
           } ELSE { \
             UPDATE $after.build_id SET verified_count += \
               (IF $after.verified THEN 1 ELSE 0 END); \
           }; \
         };",
    )
    .await?
    .check()?;
    Ok(())
}

/// One-time backfill of `verified_count` for a database upgraded from before
/// the counter existed. Cheap on a database that already carries the marker
/// row (one indexed read); the rewrite itself recomputes every count from
/// the review rows, so it runs once per database.
async fn ensure_verified_backfill(db: &Db) -> Result<()> {
    // the marker: absent on databases that predate the counter. The record
    // fetch reads by key and answers an empty list for a missing record.
    let stamped: Vec<surrealdb::types::RecordId> = db
        .query("SELECT VALUE id FROM change_stamp:verified_backfill;")
        .await?
        .take(0)?;
    if !stamped.is_empty() {
        return Ok(());
    }
    db.query(
        "UPDATE build SET verified_count = \
           count(array::filter(<~review.*.verified, |$v| $v = true)); \
         UPSERT change_stamp:verified_backfill SET changed_at = 0;",
    )
    .await?
    .check()?;
    Ok(())
}

/// Defines the password-reset tokens and their lookup index. Created on
/// connect rather than left to the seed: a database that never took a reset
/// export still has to answer the form, and `IF NOT EXISTS` leaves a seeded
/// one alone. The index is unique so two accounts can never share a token
/// hash, and the table stays schemaless like `user`.
async fn ensure_reset_schema(db: &Db) -> Result<()> {
    db.query(
        "DEFINE TABLE IF NOT EXISTS password_reset SCHEMALESS; \
         DEFINE INDEX IF NOT EXISTS password_reset_token ON password_reset FIELDS token_hash UNIQUE;",
    )
    .await?
    .check()?;
    Ok(())
}

pub mod admin;
pub mod catalog;
pub mod combi;
pub mod detail;
pub mod episodes;
pub mod gacha;
pub mod search;
pub mod sessions;
pub mod stamp;
pub mod types;
pub mod users;

/// The read/write surface the rest of the crate uses, re-exported so a caller
/// never names a submodule.
pub use admin::{insert_entity, update_entity};
pub use catalog::{
    cards_by_ids, select_cookies, select_episode_list, select_ingredient_list, select_jelly_list,
    select_pets, select_relic_groups, select_simple, select_skin_list, select_treasures,
    sitemap_entries, EpisodeList, IngredientList, JellyList, RelicGroup, SkinList,
};
pub use combi::{
    combi_bonuses, combi_edit_rows, combi_partner_ids, delete_combi, entity_link, names_for,
    CombiEditRow, CombiRow,
};
pub use detail::{
    blessed_differs, ingredient_facts, ingredient_recipes, jelly_makers, select_detail,
    select_detail_strict, treasure_craft_ingredients, treasure_edit_links, treasure_effects,
    treasure_links, treasure_variant, unlocked_treasure, CraftIngredient, CraftRecipe, EffectLine,
    EffectLineRow, IngredientFacts, JellyMaker, TreasureEditLinks, TreasureLinks,
};
pub use episodes::{episode_extras, EpisodeDetail};
pub use gacha::{select_gacha, GachaPool};
pub use search::search;
pub use stamp::{catalog_revision, change_stamp, sitemap_sources, FEED_SOURCES};
pub use types::{tr, Card, Detail, Kind};
pub use users::{
    consume_reset_token, create_user, find_user, find_user_by_email, find_user_by_google_sub,
    find_user_by_id, issue_reset_token, link_user_google, reset_requested_within,
    reset_token_pending, set_user_avatar, set_user_email, set_user_password, User,
};
