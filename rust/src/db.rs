//! SQLite access. rusqlite rather than an async driver: the V app talks to
//! SQLite synchronously and the queries are short, so a blocking pool behind
//! `spawn_blocking` keeps the port a straight translation instead of a
//! rewrite. FTS5 comes from the bundled SQLite build.

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

pub type Db = Pool<SqliteConnectionManager>;

pub fn open(path: &str) -> Result<Db, r2d2::Error> {
    let manager = SqliteConnectionManager::file(path);
    Pool::builder().max_size(8).build(manager)
}

/// One cookie as the catalog card needs it: the localized name with the
/// English one alongside for the cross-language filter.
#[derive(Debug, Clone)]
pub struct CookieCard {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub grade: i64,
}

/// Mirrors select_cookies: newest release date first with the id as the
/// tie-break. Without that second key SQLite may order rows tied on
/// release_date differently between the offset=0 and offset=30 fetches, and
/// infinite scroll would repeat or skip cards.
pub fn select_cookies(
    db: &Db,
    lang: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<CookieCard>, rusqlite::Error> {
    let conn = db.get().expect("db pool");
    let mut stmt = conn.prepare(
        "SELECT c.cookie_id AS id, c.image AS image, c.grade AS grade,
                COALESCE(tl.name, te.name) AS name,
                COALESCE(te.name, '')      AS en_name
           FROM cookie c
           LEFT JOIN cookie_translation tl
                  ON tl.owner_id = c.cookie_id AND tl.lang = ?1
           LEFT JOIN cookie_translation te
                  ON te.owner_id = c.cookie_id AND te.lang = 'en'
          WHERE tl.name IS NOT NULL OR te.name IS NOT NULL
          ORDER BY c.release_date DESC, c.cookie_id DESC
          LIMIT ?2 OFFSET ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![lang, limit, offset], |row| {
        Ok(CookieCard {
            id: row.get("id")?,
            image: row.get("image")?,
            grade: row.get("grade")?,
            name: row.get("name")?,
            en_name: row.get("en_name")?,
        })
    })?;
    rows.collect()
}
