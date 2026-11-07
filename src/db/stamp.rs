//! When the content behind a generated document last moved.
//!
//! The sitemap's urls are ids: catalog rows and news posts. The only writes
//! that change it are a row appearing and a row disappearing — and a timestamp
//! stored ON the row cannot express the second one, because the row is gone. A
//! stamp that lives beside the rows can, so every table a document is built
//! from carries one: `change_stamp:cookie`, `change_stamp:news`, and so on.
//!
//! The stamp is maintained by a database EVENT rather than by the write paths,
//! for two reasons that each stand on their own:
//!
//! - **no write can forget it.** The catalog editor, a hand-written
//!   `DELETE cookie:7` and a bulk `UPDATE cookie SET …` all move it, because
//!   all three are writes to the table — and the app has no delete path at all,
//!   so out-of-band writes are the only way a row can vanish;
//! - **it only moves forward.** The event takes `time::unix()` through a
//!   `math::max` against the value already stored, because a date that moves
//!   backwards would let a client's `If-Modified-Since` be answered "not
//!   modified" for a document that had in fact changed — and a row being
//!   deleted is exactly the case where every row-derived date moves backwards.
//!
//! One event, one statement, one record: `THEN { … }` blocks lose every
//! statement after the first on this server (verified against 3.3.4), so a
//! body that needs to touch two records is a shape to avoid — which is why the
//! records are per TABLE and each document reads the tables it is built from,
//! rather than each document carrying its own record.
//!
//! What the events do NOT see is `surreal import` (the caveat `ensure_search_schema`
//! documents): a freshly seeded database has no stamp rows until its first app
//! write, and until then a document dates itself by the newest news publication
//! it has, exactly as it did before these stamps existed. The generated `ETag`
//! is unaffected either way — it is computed from the document's own bytes.

use crate::section::Section;

use super::{Db, Result};

/// The tables whose writes can change the news feed.
pub const FEED_SOURCES: [&str; 1] = ["news"];

/// The tables whose writes can change the sitemap: every catalog section (a
/// row is a url) and news (a post's url appears, and later vanishes).
pub fn sitemap_sources() -> Vec<&'static str> {
    Section::ALL
        .iter()
        .map(|s| s.table())
        .chain(FEED_SOURCES)
        .collect()
}

/// Every table a generated document is dated by — the union of the two lists
/// above, which is what the events are defined over. A union rather than "the
/// sitemap's list" so that a document with a source the sitemap does not share
/// cannot be left waiting for an event nobody defined.
fn stamped_tables() -> Vec<&'static str> {
    let mut tables = sitemap_sources();
    tables.sort_unstable();
    tables.dedup();
    tables
}

const CATALOG_AUXILIARY_TABLES: [&str; 8] = [
    "combi",
    "jelly_maker",
    "ingredient_recipe",
    "episode_stage",
    "quest",
    "episode_relic",
    "episode_draw_reward",
    "episode_box_odds",
];

fn catalog_tables() -> Vec<&'static str> {
    let mut tables: Vec<_> = Section::ALL.iter().map(|section| section.table()).collect();
    tables.extend(CATALOG_AUXILIARY_TABLES);
    tables.sort_unstable();
    tables.dedup();
    tables
}

/// Defines existing stamps and the independent whole-catalog revision events.
pub async fn ensure(db: &Db) -> Result<()> {
    db.query("DEFINE TABLE IF NOT EXISTS change_stamp SCHEMALESS;")
        .await?
        .check()?;
    db.query("DEFINE TABLE IF NOT EXISTS catalog_revision SCHEMALESS;")
        .await?
        .check()?;
    for table in stamped_tables() {
        db.query(event_sql(table)).await?.check()?;
    }
    for table in catalog_tables() {
        db.query(catalog_event_sql(table)).await?.check()?;
    }
    db.query("UPSERT catalog_revision:all SET revision = revision ?? rand::id();")
        .await?
        .check()?;
    Ok(())
}

/// The event that dates one table's writes, kept whole in one place so a test
/// can read it back without a server.
fn event_sql(table: &str) -> String {
    format!(
        "DEFINE EVENT IF NOT EXISTS {table}_change_stamp ON TABLE {table} \
         WHEN ($event = 'CREATE' OR $event = 'UPDATE' OR $event = 'DELETE') \
         THEN UPSERT type::record('change_stamp', '{table}') SET \
         changed_at = math::max([time::unix(), \
         (SELECT VALUE changed_at FROM type::record('change_stamp', '{table}'))[0] ?? 0]);"
    )
}

fn catalog_event_sql(table: &str) -> String {
    format!(
        "DEFINE EVENT IF NOT EXISTS {table}_catalog_revision ON TABLE {table} \
         WHEN ($event = 'CREATE' OR $event = 'UPDATE' OR $event = 'DELETE') \
         THEN UPSERT type::record('catalog_revision', 'all') SET revision = rand::id();"
    )
}

/// Return the current whole-catalog revision; a missing record is an error.
pub async fn catalog_revision(db: &Db) -> Result<String> {
    let mut response = db
        .query("SELECT VALUE revision FROM catalog_revision:all")
        .await?
        .check()?;
    let values: Vec<String> = response.take(0)?;
    values
        .into_iter()
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            surrealdb::Error::thrown("catalog revision is missing or uninitialized".into())
        })
}

/// The newest moment any of `tables` last changed, or `None` when no write has
/// ever recorded one — a database that has not been written to since these
/// events were defined.
///
/// The max is taken here rather than in the query because a document reads a
/// handful of rows: `math::max` over a `GROUP ALL` answers `-Infinity` rather
/// than nothing when no row matches, which is a value every caller would then
/// have to unpick.
pub async fn change_stamp(db: &Db, tables: &[&str]) -> Result<Option<i64>> {
    let ids: Vec<String> = tables.iter().map(|t| (*t).to_string()).collect();
    let stamps: Vec<i64> = db
        .query("SELECT VALUE changed_at FROM change_stamp WHERE record::id(id) IN $ids")
        .bind(("ids", ids))
        .await?
        .take(0)?;
    Ok(stamps.into_iter().filter(|t| *t > 0).max())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use crate::time::now_unix;

    #[test]
    fn every_stamped_table_carries_its_own_event() {
        let tables = stamped_tables();
        for section in Section::ALL {
            assert!(
                tables.contains(&section.table()),
                "{} is not stamped",
                section.table()
            );
        }
        assert!(tables.contains(&"news"));
        assert_eq!(tables.len(), Section::ALL.len() + 1);
        for table in tables {
            let sql = event_sql(table);
            assert!(sql.contains(&format!("ON TABLE {table}")));
            assert!(sql.contains(&format!("type::record('change_stamp', '{table}')")));
            for event in ["'CREATE'", "'UPDATE'", "'DELETE'"] {
                assert!(sql.contains(event));
            }
            assert!(sql.contains("math::max([time::unix()"));
        }
    }

    #[test]
    fn the_two_documents_read_their_own_sources() {
        let sitemap = sitemap_sources();
        assert!(sitemap.contains(&"cookie"));
        assert!(sitemap.contains(&"news"));
        assert_eq!(FEED_SOURCES, ["news"]);
        for source in FEED_SOURCES {
            assert!(sitemap.contains(&source));
        }
        assert_eq!(stamped_tables().len(), sitemap.len());
    }

    #[tokio::test]
    async fn a_row_appearing_or_vanishing_moves_the_sitemap_stamp() {
        const FUTURE: i64 = 4_000_000_000;
        let Some(db) = scratch_db("stamp").await else {
            return;
        };
        db.query("DEFINE TABLE IF NOT EXISTS name_gram SCHEMALESS;")
            .await
            .unwrap()
            .check()
            .unwrap();
        db.query("DELETE change_stamp; DELETE cookie;")
            .await
            .unwrap()
            .check()
            .unwrap();
        assert_eq!(change_stamp(&db, &sitemap_sources()).await.unwrap(), None);
        let create = "CREATE cookie:90001 SET image = 'x', tr = { en: { name: 'Stamped' } };";
        db.query(create).await.unwrap().check().unwrap();
        let created = change_stamp(&db, &sitemap_sources())
            .await
            .unwrap()
            .expect("create dates it");
        assert!(created > 1_700_000_000);
        assert_eq!(change_stamp(&db, &FEED_SOURCES).await.unwrap(), None);
        plant(&db, "cookie", 1).await;
        db.query("UPDATE cookie:90001 SET image = 'y';")
            .await
            .unwrap()
            .check()
            .unwrap();
        assert!(
            change_stamp(&db, &sitemap_sources())
                .await
                .unwrap()
                .expect("edit dates it")
                > 1
        );
        plant(&db, "cookie", 1).await;
        db.query("DELETE cookie:90001;")
            .await
            .unwrap()
            .check()
            .unwrap();
        assert!(
            change_stamp(&db, &sitemap_sources())
                .await
                .unwrap()
                .expect("delete dates it")
                > 1
        );
        plant(&db, "cookie", FUTURE).await;
        db.query(create).await.unwrap().check().unwrap();
        assert_eq!(
            change_stamp(&db, &sitemap_sources()).await.unwrap(),
            Some(FUTURE)
        );
        plant(&db, "news", 1).await;
        let draft = crate::news::NewsDraft {
            featured: false,
            title: "Stamped".into(),
            summary: String::new(),
            body: String::new(),
            published: true,
            published_at: now_unix(),
        };
        let id = crate::news::insert_post(&db, "en", &draft, "tester", 0)
            .await
            .unwrap();
        assert!(change_stamp(&db, &FEED_SOURCES).await.unwrap() > Some(1));
        assert!(change_stamp(&db, &sitemap_sources()).await.unwrap() > Some(1));
        crate::news::delete_post(&db, id).await.unwrap();
        db.query("DELETE cookie; DELETE change_stamp;")
            .await
            .unwrap()
            .check()
            .unwrap();
    }

    #[tokio::test]
    async fn the_url_list_is_cached_against_the_stamp() {
        const BEFORE: Option<i64> = Some(11);
        const AFTER: Option<i64> = Some(12);
        let Some(db) = scratch_db("entries").await else {
            return;
        };
        db.query("DEFINE TABLE IF NOT EXISTS name_gram SCHEMALESS; DELETE cookie;")
            .await
            .unwrap()
            .check()
            .unwrap();
        let listed = crate::db::sitemap_entries(&db, BEFORE).await.unwrap();
        assert!(!listed.iter().any(|(section, _)| *section == "news"));
        db.query("CREATE cookie:90002 SET image = 'x', tr = { en: { name: 'Cached' } };")
            .await
            .unwrap()
            .check()
            .unwrap();
        assert!(!crate::db::sitemap_entries(&db, BEFORE)
            .await
            .unwrap()
            .contains(&("cookies", 90002)));
        assert!(crate::db::sitemap_entries(&db, AFTER)
            .await
            .unwrap()
            .contains(&("cookies", 90002)));
        db.query("DELETE cookie:90002;")
            .await
            .unwrap()
            .check()
            .unwrap();
        assert!(!crate::db::sitemap_entries(&db, Some(13))
            .await
            .unwrap()
            .contains(&("cookies", 90002)));
        db.query("DELETE change_stamp;")
            .await
            .unwrap()
            .check()
            .unwrap();
    }

    #[tokio::test]
    async fn catalog_revision_tracks_same_second_updates_and_auxiliary_deletes() {
        let Some(db) = scratch_db("catalog_revision").await else {
            return;
        };
        db.query("DEFINE TABLE IF NOT EXISTS name_gram SCHEMALESS;")
            .await
            .unwrap()
            .check()
            .unwrap();
        db.query("DELETE catalog_revision; DELETE cookie; DELETE combi;")
            .await
            .unwrap()
            .check()
            .unwrap();
        assert!(
            catalog_revision(&db).await.is_err(),
            "uninitialized state must fail closed"
        );
        db.query("CREATE cookie:90201 SET image = 'x', tr = { en: { name: 'Revision' } };")
            .await
            .unwrap()
            .check()
            .unwrap();
        let created = catalog_revision(&db).await.unwrap();
        db.query("UPDATE cookie:90201 SET image = 'y';")
            .await
            .unwrap()
            .check()
            .unwrap();
        let edited = catalog_revision(&db).await.unwrap();
        assert_ne!(created, edited, "same-second edit must change revision");
        db.query("CREATE combi:90201 SET value = 'x';")
            .await
            .unwrap()
            .check()
            .unwrap();
        let auxiliary = catalog_revision(&db).await.unwrap();
        assert_ne!(edited, auxiliary, "auxiliary table writes invalidate");
        db.query("DELETE combi:90201;")
            .await
            .unwrap()
            .check()
            .unwrap();
        let deleted = catalog_revision(&db).await.unwrap();
        assert_ne!(auxiliary, deleted, "auxiliary deletions invalidate");
        db.query("DELETE cookie:90201; DELETE catalog_revision;")
            .await
            .unwrap()
            .check()
            .unwrap();
    }

    async fn plant(db: &Db, table: &str, at: i64) {
        db.query("UPDATE type::record('change_stamp', $table) SET changed_at = $at")
            .bind(("table", table.to_string()))
            .bind(("at", at))
            .await
            .unwrap()
            .check()
            .unwrap();
    }

    async fn scratch_db(suite: &str) -> Option<Db> {
        let url = std::env::var("CR_SURREAL_URL").ok()?;
        if std::env::var("CR_NEWS_WRITE_TEST").ok().as_deref() != Some("1") {
            eprintln!("skip: set CR_NEWS_WRITE_TEST=1 to run the stamp write test");
            return None;
        }
        let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
        let ns = format!("cookierun_stamp_{suite}");
        match crate::db::connect_url(&url, &ns, &ns, &user, &pass).await {
            Ok(db) => Some(db),
            Err(e) => {
                eprintln!("skip: CR_SURREAL_URL is set but unusable ({e})");
                None
            }
        }
    }
}
