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

/// Defines the stamp table and the events that keep it current. Called on
/// connect like the other schema guards: an `IF NOT EXISTS` leaves a database
/// that already carries them untouched, and a seeded one is only missing the
/// rows, which its first write creates.
pub async fn ensure(db: &Db) -> Result<()> {
    db.query("DEFINE TABLE IF NOT EXISTS change_stamp SCHEMALESS;")
        .await?
        .check()?;
    for table in stamped_tables() {
        db.query(event_sql(table)).await?.check()?;
    }
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

    /// Every table a document is built from carries an event of its own, each
    /// writing that table's own record: the catalog sections, and news.
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
        assert!(tables.contains(&"news"), "the feed is stamped by news");
        assert_eq!(
            tables.len(),
            Section::ALL.len() + 1,
            "one event per table, no duplicates: {tables:?}"
        );

        for table in tables {
            let sql = event_sql(table);
            assert!(sql.contains(&format!("ON TABLE {table}")), "{sql}");
            assert!(
                sql.contains(&format!("type::record('change_stamp', '{table}')")),
                "each table has its own record: {sql}"
            );
            for event in ["'CREATE'", "'UPDATE'", "'DELETE'"] {
                assert!(sql.contains(event), "a {event} must move it: {sql}");
            }
            // the guard that keeps the date moving forward, and the clock
            assert!(sql.contains("math::max([time::unix()"), "{sql}");
        }
    }

    /// A catalog row is a sitemap url and a news post is both a url and a feed
    /// entry: the two documents' source lists differ, and the feed's is a
    /// strict subset — a catalog write must never invalidate a feed.
    #[test]
    fn the_two_documents_read_their_own_sources() {
        let sitemap = sitemap_sources();
        assert!(sitemap.contains(&"cookie"));
        assert!(sitemap.contains(&"news"), "a post is a url");
        assert_eq!(FEED_SOURCES, ["news"]);
        for source in FEED_SOURCES {
            assert!(sitemap.contains(&source), "{source} is in both");
        }
        assert_eq!(stamped_tables().len(), sitemap.len());
    }

    /// The whole mechanism against a live server: a created row, an edited row
    /// and a DELETED row each move the sitemap's stamp — the deletion being the
    /// one a row-derived timestamp can never express — the date never moves
    /// backwards, and a catalog write leaves the feed's stamp alone.
    ///
    /// Double-gated like the other writing tests: `CR_SURREAL_URL` names the
    /// server and `CR_NEWS_WRITE_TEST=1` consents to writes, and it always
    /// connects to a namespace of its own, so it can never touch the catalog
    /// the URL actually names.
    #[tokio::test]
    async fn a_row_appearing_or_vanishing_moves_the_sitemap_stamp() {
        // a stamp planted in the future, for the backwards-clock case below
        const FUTURE: i64 = 4_000_000_000;
        let Some(db) = scratch_db("stamp").await else {
            return;
        };
        // the seed's name-gram ingest event fires on a catalog create, so a
        // bare namespace needs the table it writes to
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

        // nothing has been written, so there is nothing to date the document by
        assert_eq!(change_stamp(&db, &sitemap_sources()).await.unwrap(), None);

        let create = "CREATE cookie:90001 SET image = 'x', tr = { en: { name: 'Stamped' } };";
        db.query(create).await.unwrap().check().unwrap();
        let created = change_stamp(&db, &sitemap_sources())
            .await
            .unwrap()
            .expect("a create dates the document");
        assert!(created > 1_700_000_000, "a real clock, not zero: {created}");
        assert_eq!(
            change_stamp(&db, &FEED_SOURCES).await.unwrap(),
            None,
            "a catalog row is not a feed entry"
        );

        // an edit moves it: the planted 1 makes the move visible inside the
        // second the two writes share
        plant(&db, "cookie", 1).await;
        db.query("UPDATE cookie:90001 SET image = 'y';")
            .await
            .unwrap()
            .check()
            .unwrap();
        let edited = change_stamp(&db, &sitemap_sources())
            .await
            .unwrap()
            .expect("an edit dates it");
        assert!(edited > 1, "the edit moved it forward: {edited}");

        // and a deletion moves it — the case no row can record, because the
        // row is gone
        plant(&db, "cookie", 1).await;
        db.query("DELETE cookie:90001;")
            .await
            .unwrap()
            .check()
            .unwrap();
        let deleted = change_stamp(&db, &sitemap_sources())
            .await
            .unwrap()
            .expect("a delete dates it");
        assert!(deleted > 1, "the delete moved it forward: {deleted}");

        // a clock that steps backwards must not take the date with it: with a
        // future stamp in place, another write leaves it exactly where it was
        plant(&db, "cookie", FUTURE).await;
        db.query(create).await.unwrap().check().unwrap();
        assert_eq!(
            change_stamp(&db, &sitemap_sources()).await.unwrap(),
            Some(FUTURE),
            "never backwards, or a client's date could be answered with a stale 304"
        );

        // news is both documents' source: a post moves the feed's stamp too
        plant(&db, "news", 1).await;
        let draft = crate::news::NewsDraft {
            title: "Stamped".into(),
            summary: String::new(),
            body: String::new(),
            published: true,
            published_at: now_unix(),
        };
        let id = crate::news::insert_post(&db, "en", &draft, "tester", 0)
            .await
            .unwrap();
        assert!(
            change_stamp(&db, &FEED_SOURCES).await.unwrap() > Some(1),
            "a post is a feed entry"
        );
        assert!(change_stamp(&db, &sitemap_sources()).await.unwrap() > Some(1));

        // cleanup: the rows this test made, and the stamps it planted
        crate::news::delete_post(&db, id).await.unwrap();
        db.query("DELETE cookie; DELETE change_stamp;")
            .await
            .unwrap()
            .check()
            .unwrap();
    }

    /// The url list is cached against the stamp, not against a clock — the bug
    /// this pins having been found live: with a time window the sitemap served
    /// a list from before a write while dating itself after it, so a crawler
    /// handed a tag for the changed document was told `304` for a document it
    /// had never seen.
    ///
    /// Two keys, chosen rather than read so the test cannot pass by the clock
    /// happening to tick between the writes.
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
        assert!(
            !crate::db::sitemap_entries(&db, BEFORE)
                .await
                .unwrap()
                .contains(&("cookies", 90002)),
            "the same stamp is the same list, so the eight scans are paid once"
        );
        assert!(
            crate::db::sitemap_entries(&db, AFTER)
                .await
                .unwrap()
                .contains(&("cookies", 90002)),
            "a moved stamp re-reads: the new url is listed"
        );

        db.query("DELETE cookie:90002;")
            .await
            .unwrap()
            .check()
            .unwrap();
        assert!(
            !crate::db::sitemap_entries(&db, Some(13))
                .await
                .unwrap()
                .contains(&("cookies", 90002)),
            "and a vanished url cannot be served from a list that predates it"
        );
        db.query("DELETE change_stamp;")
            .await
            .unwrap()
            .check()
            .unwrap();
    }

    /// Sets a stamp to a known value, so the next write's movement is visible
    /// without waiting out the clock's one-second resolution.
    async fn plant(db: &Db, table: &str, at: i64) {
        db.query("UPDATE type::record('change_stamp', $table) SET changed_at = $at")
            .bind(("table", table.to_string()))
            .bind(("at", at))
            .await
            .unwrap()
            .check()
            .unwrap();
    }

    /// The opt-in scratch connection: skips (with a notice) when the server is
    /// unnamed or the write consent is absent, and gives each suite its own
    /// namespace and database — `cargo test` runs these in parallel.
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
