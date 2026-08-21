//! SQLite access. rusqlite rather than an async driver: the V app talks to
//! SQLite synchronously and the queries are short, so a blocking pool behind
//! `spawn_blocking` keeps the port a translation instead of a rewrite.

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, params_from_iter, Row};

use crate::grade;

pub type Db = Pool<SqliteConnectionManager>;

pub fn open(path: &str) -> Result<Db, r2d2::Error> {
    let manager = SqliteConnectionManager::file(path);
    Pool::builder().max_size(8).build(manager)
}

/// One catalog card: what the grid needs, with the English name alongside so
/// the filter matches across languages.
#[derive(Debug, Clone, Default)]
pub struct Card {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub is_evolved: bool,
}

impl Card {
    pub fn grade_slug(&self) -> String {
        self.grade.map(grade::slug).unwrap_or("").to_string()
    }
    /// "S+" for s_plus, the uppercase letter otherwise
    pub fn grade_label(&self) -> String {
        self.grade.map(grade::label).unwrap_or_default()
    }
    pub fn has_grade(&self) -> bool {
        self.grade.is_some()
    }
}

/// A detail page's fields. Which are filled depends on the kind; each
/// template reads only the ones its kind has.
#[derive(Debug, Clone, Default)]
pub struct Detail {
    #[allow(dead_code)]
    pub id: i64,
    pub name: String,
    /// the share card uses it once the OG tags land
    #[allow(dead_code)]
    pub en_name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub abilities: String,
    pub description: String,
    pub power_plus: String,
    pub power_plus_requirement: String,
    pub unlock_goal: String,
    /// unix seconds; the page formats it client-side in the viewer's locale.
    /// Zero for a kind whose table has no such column, and for a row that
    /// never got a date.
    pub release_date: i64,
}

impl Detail {
    pub fn grade_slug(&self) -> String {
        self.grade.map(grade::slug).unwrap_or("").to_string()
    }
    pub fn grade_label(&self) -> String {
        self.grade.map(grade::label).unwrap_or_default()
    }
    pub fn has_grade(&self) -> bool {
        self.grade.is_some()
    }
}

/// One effect line on a treasure, carrying the whole 0-9 ladder so the level
/// slider repaints without another request.
#[derive(Debug, Clone, Default)]
pub struct EffectLine {
    pub text: String,
    pub values: Vec<String>,
    pub blessed: bool,
}

impl EffectLine {
    pub fn values_attr(&self) -> String {
        self.values.join("|")
    }
    pub fn top(&self) -> String {
        self.values.last().cloned().unwrap_or_default()
    }
}

/// The other half of a cookie/pet pair, as the detail pages list it.
#[derive(Debug, Clone, Default)]
pub struct CombiRow {
    pub partner_id: i64,
    pub partner_name: String,
    pub partner_image: Option<String>,
    pub effect: String,
    pub is_hidden: bool,
}

type Conn = r2d2::PooledConnection<SqliteConnectionManager>;

fn conn(db: &Db) -> rusqlite::Result<Conn> {
    db.get()
        .map_err(|e| rusqlite::Error::InvalidParameterName(format!("pool: {e}")))
}

fn card_from(row: &Row, has_grade: bool, has_evolved: bool) -> rusqlite::Result<Card> {
    Ok(Card {
        id: row.get("id")?,
        name: row.get("name")?,
        en_name: row.get::<_, Option<String>>("en_name")?.unwrap_or_default(),
        image: row.get("image")?,
        grade: if has_grade { row.get("grade")? } else { None },
        is_evolved: if has_evolved { row.get::<_, i64>("is_evolved")? != 0 } else { false },
    })
}

/// The join every catalog list shares: the row, its translation in `lang`,
/// and the English one behind it. Rows translated in neither are dropped,
/// matching the V queries.
fn catalog_sql(
    table: &str,
    id_col: &str,
    owner_col: &str,
    tr_table: &str,
    extra: &str,
    order: &str,
) -> String {
    format!(
        "SELECT e.{id_col} AS id, e.image AS image{extra},
                COALESCE(tl.name, te.name) AS name,
                te.name AS en_name
           FROM {table} e
           LEFT JOIN {tr_table} tl ON tl.{owner_col} = e.{id_col} AND tl.lang = ?1
           LEFT JOIN {tr_table} te ON te.{owner_col} = e.{id_col} AND te.lang = 'en'
          WHERE tl.name IS NOT NULL OR te.name IS NOT NULL
          {order}"
    )
}

/// Newest release date first with the id as the tie-break. Without that
/// second key SQLite may order rows tied on release_date differently between
/// the offset=0 and offset=30 fetches, and infinite scroll would repeat or
/// skip cards.
pub fn select_cookies(db: &Db, lang: &str, limit: i64, offset: i64) -> rusqlite::Result<Vec<Card>> {
    let c = conn(db)?;
    let sql = catalog_sql(
        "cookie",
        "cookie_id",
        "owner_id",
        "cookie_translation",
        ", e.grade AS grade",
        "ORDER BY e.release_date DESC, e.cookie_id DESC LIMIT ?2 OFFSET ?3",
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params![lang, limit, offset], |r| card_from(r, true, false))?;
    rows.collect()
}

pub fn select_pets(db: &Db, lang: &str, limit: i64, offset: i64) -> rusqlite::Result<Vec<Card>> {
    let c = conn(db)?;
    let sql = catalog_sql(
        "pet",
        "pet_id",
        "pet_id",
        "pet_translation",
        ", e.grade AS grade",
        "ORDER BY e.release_date DESC, e.pet_id DESC LIMIT ?2 OFFSET ?3",
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params![lang, limit, offset], |r| card_from(r, true, false))?;
    rows.collect()
}

/// Grade (highest first), then newest, then name. `tab` is all/normal/evo.
pub fn select_treasures(
    db: &Db,
    lang: &str,
    tab: &str,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<Card>> {
    let c = conn(db)?;
    let tab_where = match tab {
        "normal" => " AND e.is_evolved = 0",
        "evo" => " AND e.is_evolved = 1",
        _ => "",
    };
    let sql = format!(
        "SELECT e.treasure_id AS id, e.image AS image, e.grade AS grade,
                e.is_evolved AS is_evolved,
                COALESCE(tl.name, te.name) AS name, te.name AS en_name
           FROM treasure e
           LEFT JOIN treasure_translation tl ON tl.treasure_id = e.treasure_id AND tl.lang = ?1
           LEFT JOIN treasure_translation te ON te.treasure_id = e.treasure_id AND te.lang = 'en'
          WHERE (tl.name IS NOT NULL OR te.name IS NOT NULL){tab_where}
          ORDER BY {rank} DESC, e.release_date DESC, COALESCE(tl.name, te.name) ASC
          LIMIT ?2 OFFSET ?3",
        rank = grade::rank_sql("e.grade")
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params![lang, limit, offset], |r| card_from(r, true, true))?;
    rows.collect()
}

/// The simple catalogs, which share a shape and are unpaginated in the V app.
pub fn select_simple(db: &Db, lang: &str, kind: &str) -> rusqlite::Result<Vec<Card>> {
    let (table, id_col, tr_table, extra, graded) = match kind {
        "episodes" => ("episode", "episode_id", "episode_translation", "", false),
        "ingredients" => (
            "ingredient",
            "ingredient_id",
            "ingredient_translation",
            ", e.grade AS grade",
            true,
        ),
        "jellies" => ("jelly", "jelly_id", "jelly_translation", "", false),
        "skins" => ("skin", "skin_id", "skin_translation", ", e.grade AS grade", true),
        "relics" => ("relic", "relic_id", "relic_translation", "", false),
        _ => return Ok(Vec::new()),
    };
    let c = conn(db)?;
    let sql = catalog_sql(table, id_col, id_col, tr_table, extra, &format!("ORDER BY e.{id_col}"));
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params![lang], |r| card_from(r, graded, false))?;
    rows.collect()
}

/// One entity's detail row. Columns a kind does not have come back empty.
pub fn select_detail(db: &Db, lang: &str, kind: &str, id: i64) -> rusqlite::Result<Option<Detail>> {
    let cookie_prose = ", tr.abilities AS abilities, tr.description AS description, tr.power_plus AS power_plus, tr.power_plus_requirement AS ppr, tr.unlock_goal AS unlock_goal";
    let pet_prose = ", tr.abilities AS abilities, tr.description AS description, '' AS power_plus, '' AS ppr, '' AS unlock_goal";
    let plain = ", '' AS abilities, tr.description AS description, '' AS power_plus, '' AS ppr, '' AS unlock_goal";
    let (table, id_col, owner_col, tr_table, extra, prose) = match kind {
        "cookies" => ("cookie", "cookie_id", "owner_id", "cookie_translation", ", e.grade AS grade", cookie_prose),
        "pets" => ("pet", "pet_id", "pet_id", "pet_translation", ", e.grade AS grade", pet_prose),
        "treasures" => ("treasure", "treasure_id", "treasure_id", "treasure_translation", ", e.grade AS grade", plain),
        "episodes" => ("episode", "episode_id", "episode_id", "episode_translation", "", plain),
        "ingredients" => ("ingredient", "ingredient_id", "ingredient_id", "ingredient_translation", ", e.grade AS grade", plain),
        "jellies" => ("jelly", "jelly_id", "jelly_id", "jelly_translation", "", plain),
        "relics" => ("relic", "relic_id", "relic_id", "relic_translation", "", plain),
        "skins" => ("skin", "skin_id", "skin_id", "skin_translation", ", e.grade AS grade", plain),
        _ => return Ok(None),
    };
    let graded = !extra.is_empty();
    // only these three tables carry the column
    let has_date = matches!(kind, "cookies" | "pets" | "treasures");
    let dated = if has_date { ", COALESCE(e.release_date, 0) AS release_date" } else { "" };
    let c = conn(db)?;
    let sql = format!(
        "SELECT e.{id_col} AS id, e.image AS image{extra}{dated},
                COALESCE(tr.name, '') AS name, COALESCE(te.name, '') AS en_name{prose}
           FROM {table} e
           LEFT JOIN {tr_table} tr ON tr.{owner_col} = e.{id_col} AND tr.lang IN (?1, 'en')
           LEFT JOIN {tr_table} te ON te.{owner_col} = e.{id_col} AND te.lang = 'en'
          WHERE e.{id_col} = ?2
          ORDER BY (tr.lang = ?1) DESC
          LIMIT 1"
    );
    let mut stmt = c.prepare(&sql)?;
    let mut rows = stmt.query(params![lang, id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(Detail {
        id: row.get("id")?,
        name: row.get("name")?,
        en_name: row.get("en_name")?,
        image: row.get("image")?,
        grade: if graded { row.get("grade")? } else { None },
        abilities: row.get::<_, Option<String>>("abilities")?.unwrap_or_default(),
        description: row.get::<_, Option<String>>("description")?.unwrap_or_default(),
        power_plus: row.get::<_, Option<String>>("power_plus")?.unwrap_or_default(),
        power_plus_requirement: row.get::<_, Option<String>>("ppr")?.unwrap_or_default(),
        unlock_goal: row.get::<_, Option<String>>("unlock_goal")?.unwrap_or_default(),
        release_date: if has_date { row.get("release_date")? } else { 0 },
    }))
}

/// The treasure a cookie or pet unlocks, if any — the reverse of the link
/// the treasure page shows. `kind` is "cookie" or "pet".
pub fn unlocked_treasure(
    db: &Db,
    lang: &str,
    kind: &str,
    id: i64,
) -> rusqlite::Result<Option<(i64, String, Option<String>)>> {
    let col = match kind {
        "cookie" => "unlock_cookie_id",
        "pet" => "unlock_pet_id",
        _ => return Ok(None),
    };
    let c = conn(db)?;
    let sql = format!(
        "SELECT t.treasure_id AS id, t.image AS image,
                COALESCE(tl.name, te.name, '') AS name
           FROM treasure t
           LEFT JOIN treasure_translation tl ON tl.treasure_id = t.treasure_id AND tl.lang = ?1
           LEFT JOIN treasure_translation te ON te.treasure_id = t.treasure_id AND te.lang = 'en'
          WHERE t.{col} = ?2
          LIMIT 1"
    );
    let mut stmt = c.prepare(&sql)?;
    let mut rows = stmt.query(params![lang, id])?;
    match rows.next()? {
        Some(row) => Ok(Some((row.get("id")?, row.get("name")?, row.get("image")?))),
        None => Ok(None),
    }
}

/// A treasure's effect lines with their 0-9 ladders, normal and blessed,
/// deduped and in wiki order. The column is bracket-quoted because `values`
/// is a keyword in newer SQLite.
pub fn treasure_effects(db: &Db, lang: &str, id: i64) -> rusqlite::Result<Vec<EffectLine>> {
    let c = conn(db)?;
    let links: Vec<(i64, i64, String)> = {
        let mut stmt = c.prepare(
            "SELECT te.effect_id AS effect_id, te.state AS state,
                    COALESCE(el.name, ee.name, '') AS text
               FROM treasure_effect te
               LEFT JOIN effect_translation el ON el.effect_id = te.effect_id AND el.lang = ?1
               LEFT JOIN effect_translation ee ON ee.effect_id = te.effect_id AND ee.lang = 'en'
              WHERE te.treasure_id = ?2
              ORDER BY te.treasure_effect_id",
        )?;
        let rows = stmt.query_map(params![lang, id], |r| {
            Ok((r.get("effect_id")?, r.get("state")?, r.get("text")?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    let mut out = Vec::new();
    let mut seen: Vec<(i64, i64)> = Vec::new();
    let mut lv = c.prepare(
        "SELECT [values] AS v FROM treasure_level
          WHERE treasure_id = ?1 AND effect_id = ?2 AND state = ?3
          ORDER BY level",
    )?;
    for (effect_id, state, text) in links {
        if seen.contains(&(effect_id, state)) {
            continue;
        }
        seen.push((effect_id, state));
        let values: Vec<String> = lv
            .query_map(params![id, effect_id, state], |r| r.get::<_, String>("v"))?
            .collect::<rusqlite::Result<_>>()?;
        out.push(EffectLine { text, values, blessed: state == 1 });
    }
    Ok(out)
}

/// The combo bonuses a cookie or pet takes part in, partner resolved.
pub fn combi_bonuses(db: &Db, lang: &str, kind: &str, id: i64) -> rusqlite::Result<Vec<CombiRow>> {
    let (own_col, partner_col, partner_table, partner_id_col, partner_tr, partner_owner) =
        match kind {
            "cookies" => ("cookie_id", "pet_id", "pet", "pet_id", "pet_translation", "pet_id"),
            "pets" => ("pet_id", "cookie_id", "cookie", "cookie_id", "cookie_translation", "owner_id"),
            _ => return Ok(Vec::new()),
        };
    let c = conn(db)?;
    let sql = format!(
        "SELECT p.{partner_id_col} AS partner_id, p.image AS partner_image,
                COALESCE(tl.name, te.name, '') AS partner_name,
                COALESCE(el.name, ee.name, '') AS effect,
                cb.is_hidden AS is_hidden
           FROM combi_bonus cb
           JOIN {partner_table} p ON p.{partner_id_col} = cb.{partner_col}
           LEFT JOIN {partner_tr} tl ON tl.{partner_owner} = p.{partner_id_col} AND tl.lang = ?1
           LEFT JOIN {partner_tr} te ON te.{partner_owner} = p.{partner_id_col} AND te.lang = 'en'
           LEFT JOIN effect_translation el ON el.effect_id = cb.effect_id AND el.lang = ?1
           LEFT JOIN effect_translation ee ON ee.effect_id = cb.effect_id AND ee.lang = 'en'
          WHERE cb.{own_col} = ?2
          ORDER BY cb.id"
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params![lang, id], |r| {
        Ok(CombiRow {
            partner_id: r.get("partner_id")?,
            partner_name: r.get("partner_name")?,
            partner_image: r.get("partner_image")?,
            effect: r.get("effect")?,
            is_hidden: r.get::<_, i64>("is_hidden")? != 0,
        })
    })?;
    rows.collect()
}

/// Name and sprite for a rich-text link, in one query — which keeps a
/// description with several links from costing a round trip each.
pub fn entity_link(db: &Db, lang: &str, kind: &str, id: i64) -> Option<(String, Option<String>)> {
    let (table, id_col, tr_table, owner_col) = match kind {
        "pet" => ("pet", "pet_id", "pet_translation", "pet_id"),
        "treasure" => ("treasure", "treasure_id", "treasure_translation", "treasure_id"),
        _ => ("cookie", "cookie_id", "cookie_translation", "owner_id"),
    };
    let c = conn(db).ok()?;
    let sql = format!(
        "SELECT e.image AS image, t.name AS name
           FROM {table} e
           LEFT JOIN {tr_table} t ON t.{owner_col} = e.{id_col} AND t.lang IN (?1, 'en')
          WHERE e.{id_col} = ?2
          ORDER BY (t.lang = ?1) DESC
          LIMIT 1"
    );
    let mut stmt = c.prepare(&sql).ok()?;
    let mut rows = stmt.query(params![lang, id]).ok()?;
    let row = rows.next().ok()??;
    let name: Option<String> = row.get("name").ok()?;
    let image: Option<String> = row.get("image").ok()?;
    name.filter(|n| !n.is_empty()).map(|n| (n, image))
}

/// Every (section, id) pair the sitemap lists.
pub fn sitemap_entries(db: &Db) -> rusqlite::Result<Vec<(String, i64)>> {
    let c = conn(db)?;
    let mut out = Vec::new();
    for (section, table, id_col) in [
        ("cookies", "cookie", "cookie_id"),
        ("pets", "pet", "pet_id"),
        ("treasures", "treasure", "treasure_id"),
        ("episodes", "episode", "episode_id"),
        ("ingredients", "ingredient", "ingredient_id"),
        ("jellies", "jelly", "jelly_id"),
    ] {
        let mut stmt = c.prepare(&format!("SELECT {id_col} FROM {table} ORDER BY {id_col}"))?;
        let ids: Vec<i64> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        out.extend(ids.into_iter().map(|id| (section.to_string(), id)));
    }
    Ok(out)
}

/// Cross-entity search over the localized and English names. LIKE rather than
/// FTS: Thai has no word breaks, so the tokenizer misses terms a substring
/// match finds, and the V app keeps the same fallback for exactly that. The
/// escape character is `!` so a literal % or _ in the query cannot act as a
/// wildcard.
pub fn search(db: &Db, lang: &str, q: &str, limit: i64) -> rusqlite::Result<Vec<(String, Card)>> {
    let q = q.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let escaped = q.replace('!', "!!").replace('%', "!%").replace('_', "!_");
    let like = format!("%{escaped}%");
    let c = conn(db)?;
    let mut out = Vec::new();
    // the same columns the V app's FTS tables index, so a query finds the
    // same rows: a cookie is searchable by its abilities text, not just its
    // name, which is how "magnet" reaches the cookies that grant one
    for (section, table, id_col, owner_col, tr_table, prose) in [
        ("cookies", "cookie", "cookie_id", "owner_id", "cookie_translation", "abilities, description"),
        ("pets", "pet", "pet_id", "pet_id", "pet_translation", "description"),
        ("treasures", "treasure", "treasure_id", "treasure_id", "treasure_translation", "description"),
        // the V app's SearchResults covers these too, so a relic or an
        // episode is findable by name the same way a cookie is
        ("relics", "relic", "relic_id", "relic_id", "relic_translation", "description"),
        ("episodes", "episode", "episode_id", "episode_id", "episode_translation", "description"),
        ("ingredients", "ingredient", "ingredient_id", "ingredient_id", "ingredient_translation", "description"),
    ] {
        // one OR per indexed column, on both the localized row and the
        // English one behind it
        let mut clauses: Vec<String> = Vec::new();
        for col in std::iter::once("name").chain(prose.split(", ")) {
            clauses.push(format!("tl.{col} LIKE ?2 ESCAPE '!'"));
            clauses.push(format!("te.{col} LIKE ?2 ESCAPE '!'"));
        }
        let where_sql = clauses.join(" OR ");
        let graded = matches!(section, "cookies" | "pets" | "treasures" | "ingredients");
        let grade_col = if graded { "e.grade AS grade" } else { "NULL AS grade" };
        let sql = format!(
            "SELECT e.{id_col} AS id, e.image AS image, {grade_col},
                    COALESCE(tl.name, te.name) AS name, te.name AS en_name
               FROM {table} e
               LEFT JOIN {tr_table} tl ON tl.{owner_col} = e.{id_col} AND tl.lang = ?1
               LEFT JOIN {tr_table} te ON te.{owner_col} = e.{id_col} AND te.lang = 'en'
              WHERE ({where_sql})
              ORDER BY COALESCE(tl.name, te.name)
              LIMIT ?3"
        );
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt.query_map(params![lang, like, limit], |r| card_from(r, graded, false))?;
        for card in rows {
            out.push((section.to_string(), card?));
        }
    }
    Ok(out)
}

/// A user row, for the session layer (see PORTING.md).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password: String,
    pub is_admin: bool,
}

#[allow(dead_code)]
pub fn find_user(db: &Db, username: &str) -> rusqlite::Result<Option<User>> {
    let c = conn(db)?;
    let mut stmt = c.prepare(
        "SELECT user_id, username, password, is_admin FROM user WHERE username = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query(params![username])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(User {
        id: row.get("user_id")?,
        username: row.get("username")?,
        password: row.get("password")?,
        is_admin: row.get::<_, i64>("is_admin")? != 0,
    }))
}

/// Hydrates an explicit id list, preserving the caller's order — which is the
/// ranking, so it has to survive the round trip.
#[allow(dead_code)]
pub fn cards_by_ids(db: &Db, lang: &str, kind: &str, ids: &[i64]) -> rusqlite::Result<Vec<Card>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let (table, id_col, owner_col, tr_table) = match kind {
        "pets" => ("pet", "pet_id", "pet_id", "pet_translation"),
        "treasures" => ("treasure", "treasure_id", "treasure_id", "treasure_translation"),
        _ => ("cookie", "cookie_id", "owner_id", "cookie_translation"),
    };
    let c = conn(db)?;
    let holes = std::iter::repeat("?").take(ids.len()).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT e.{id_col} AS id, e.image AS image, e.grade AS grade,
                COALESCE(tl.name, te.name) AS name, te.name AS en_name
           FROM {table} e
           LEFT JOIN {tr_table} tl ON tl.{owner_col} = e.{id_col} AND tl.lang = ?1
           LEFT JOIN {tr_table} te ON te.{owner_col} = e.{id_col} AND te.lang = 'en'
          WHERE e.{id_col} IN ({holes})"
    );
    let mut stmt = c.prepare(&sql)?;
    let mut args: Vec<String> = vec![lang.to_string()];
    args.extend(ids.iter().map(|i| i.to_string()));
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| card_from(r, true, false))?;
    let found: Vec<Card> = rows.collect::<rusqlite::Result<_>>()?;
    let mut ordered = Vec::with_capacity(found.len());
    for id in ids {
        if let Some(card) = found.iter().find(|c| c.id == *id) {
            ordered.push(card.clone());
        }
    }
    Ok(ordered)
}

/// Creates a user with an already-hashed password. `None` when the username
/// is taken, which the unique index enforces rather than a prior SELECT.
#[allow(dead_code)]
pub fn create_user(db: &Db, username: &str, password_hash: &str) -> rusqlite::Result<Option<User>> {
    let c = conn(db)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let res = c.execute(
        "INSERT INTO user (username, password, is_admin, created_at) VALUES (?1, ?2, 0, ?3)",
        params![username, password_hash, now],
    );
    match res {
        Ok(_) => Ok(Some(User {
            id: c.last_insert_rowid(),
            username: username.to_string(),
            password: password_hash.to_string(),
            is_admin: false,
        })),
        // a duplicate username is a constraint violation, not an error worth
        // surfacing: the form says the name is taken
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => Ok(None),
        Err(e) => Err(e),
    }
}

/// The ids that pair with `kind` id `id` for a combo bonus. Just the ids: the
/// picker floats them to the top of the other slot's grid and needs neither
/// the names, the sprites nor the effect text.
pub fn combi_partner_ids(db: &Db, kind: &str, id: i64) -> rusqlite::Result<Vec<i64>> {
    if id <= 0 {
        return Ok(Vec::new());
    }
    let (own_col, partner_col) = match kind {
        "cookies" => ("cookie_id", "pet_id"),
        "pets" => ("pet_id", "cookie_id"),
        _ => return Ok(Vec::new()),
    };
    let c = conn(db)?;
    let sql = format!("SELECT DISTINCT {partner_col} FROM combi_bonus WHERE {own_col} = ?1");
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params![id], |r| r.get::<_, i64>(0))?;
    rows.collect()
}

/// One draw pool with its disclosed odds, as the gacha page lists them.
#[derive(Debug, Clone)]
pub struct GachaPool {
    pub id: i64,
    /// the tier slug, which the .tr key is built from; the table's own
    /// `name` column holds a key rather than prose, so nothing reads it
    pub tier: String,
    pub entries: Vec<GachaEntry>,
}

#[derive(Debug, Clone)]
pub struct GachaEntry {
    pub section: &'static str,
    pub id: i64,
    pub name: String,
    /// the English name, so the client-side filter matches either language
    pub en_name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub odds: f64,
    pub is_evolved: bool,
}

impl GachaEntry {
    pub fn grade_slug(&self) -> String {
        self.grade.map(grade::slug).unwrap_or("").to_string()
    }
    pub fn has_grade(&self) -> bool {
        self.grade.is_some()
    }
    pub fn grade_label(&self) -> String {
        self.grade.map(grade::label).unwrap_or_default()
    }
    /// The odds as the page prints them: the stored value with no padding,
    /// so 4.4 reads "4.4%" rather than "4.40%".
    pub fn odds_label(&self) -> String {
        let mut s = format!("{}", self.odds);
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        format!("{s}%")
    }
}

/// Every pool with its entries in the catalog's own order. The prize name is
/// resolved through the same locale fallback as everywhere else.
pub fn select_gacha(db: &Db, lang: &str) -> rusqlite::Result<Vec<GachaPool>> {
    let c = conn(db)?;
    let mut pools: Vec<GachaPool> = {
        let mut stmt = c.prepare("SELECT pool_id, tier FROM gacha_pool ORDER BY pool_id")?;
        let rows = stmt.query_map([], |r| {
            Ok(GachaPool {
                id: r.get("pool_id")?,
                tier: r.get::<_, Option<String>>("tier")?.unwrap_or_default(),
                entries: Vec::new(),
            })
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    let mut stmt = c.prepare(
        "SELECT g.pool_id AS pool_id, g.odds AS odds, g.grade AS grade,
                g.treasure_id AS treasure_id, g.pet_id AS pet_id,
                COALESCE(tt.name, te.name, pt.name, pe.name, '') AS name,
                COALESCE(te.name, pe.name, '') AS en_name,
                COALESCE(t.is_evolved, 0) AS is_evolved,
                COALESCE(t.image, p.image) AS image
           FROM gacha_pool_entry g
           LEFT JOIN treasure t ON t.treasure_id = g.treasure_id
           LEFT JOIN treasure_translation tt ON tt.treasure_id = g.treasure_id AND tt.lang = ?1
           LEFT JOIN treasure_translation te ON te.treasure_id = g.treasure_id AND te.lang = 'en'
           LEFT JOIN pet p ON p.pet_id = g.pet_id
           LEFT JOIN pet_translation pt ON pt.pet_id = g.pet_id AND pt.lang = ?1
           LEFT JOIN pet_translation pe ON pe.pet_id = g.pet_id AND pe.lang = 'en'
          ORDER BY g.sort_order",
    )?;
    let rows = stmt.query_map(params![lang], |r| {
        let treasure_id: Option<i64> = r.get("treasure_id")?;
        let pet_id: Option<i64> = r.get("pet_id")?;
        let (section, id) = match (treasure_id, pet_id) {
            (Some(t), _) => ("treasures", t),
            (_, Some(p)) => ("pets", p),
            _ => ("treasures", 0),
        };
        Ok((
            r.get::<_, i64>("pool_id")?,
            GachaEntry {
                section,
                id,
                name: r.get("name")?,
                en_name: r.get("en_name")?,
                image: r.get("image")?,
                grade: r.get("grade")?,
                odds: r.get("odds")?,
                is_evolved: r.get::<_, i64>("is_evolved")? != 0,
            },
        ))
    })?;
    for row in rows {
        let (pool_id, entry) = row?;
        if let Some(pool) = pools.iter_mut().find(|p| p.id == pool_id) {
            pool.entries.push(entry);
        }
    }
    Ok(pools)
}

/// Table names for one catalog section: the row, its id column, its
/// translation table and that table's owner column.
fn entity_tables(section: &str) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    match section {
        "cookies" => Some(("cookie", "cookie_id", "cookie_translation", "owner_id")),
        "pets" => Some(("pet", "pet_id", "pet_translation", "pet_id")),
        "treasures" => Some(("treasure", "treasure_id", "treasure_translation", "treasure_id")),
        _ => None,
    }
}

/// Creates an entity and its translation in `lang`, returning the new id.
/// The prose columns a section does not have are simply not written.
pub fn insert_entity(
    db: &Db,
    lang: &str,
    section: &str,
    form: &crate::routes::admin::EntityForm,
) -> rusqlite::Result<i64> {
    let Some((table, id_col, tr_table, owner_col)) = entity_tables(section) else {
        return Ok(0);
    };
    let c = conn(db)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let image = if form.image.trim().is_empty() { None } else { Some(form.image.trim()) };
    let grade = form.grade.unwrap_or(1);

    if section == "treasures" {
        c.execute(
            "INSERT INTO treasure (image, grade, is_evolved, is_power_plus, family, source, sub, release_date)
             VALUES (?1, ?2, 0, 0, '', '', '', ?3)",
            params![image, grade, now],
        )?;
    } else {
        c.execute(
            &format!("INSERT INTO {table} (image, grade, release_date) VALUES (?1, ?2, ?3)"),
            params![image, grade, now],
        )?;
    }
    let id = c.last_insert_rowid();
    write_translation(&c, tr_table, owner_col, section, id, lang, form)?;
    let _ = id_col;
    Ok(id)
}

/// Updates an entity and its translation in `lang`.
pub fn update_entity(
    db: &Db,
    lang: &str,
    section: &str,
    id: i64,
    form: &crate::routes::admin::EntityForm,
) -> rusqlite::Result<bool> {
    let Some((table, id_col, tr_table, owner_col)) = entity_tables(section) else {
        return Ok(false);
    };
    let c = conn(db)?;
    let image = if form.image.trim().is_empty() { None } else { Some(form.image.trim()) };
    if let Some(grade) = form.grade {
        c.execute(
            &format!("UPDATE {table} SET grade = ?1 WHERE {id_col} = ?2"),
            params![grade, id],
        )?;
    }
    if image.is_some() {
        c.execute(
            &format!("UPDATE {table} SET image = ?1 WHERE {id_col} = ?2"),
            params![image, id],
        )?;
    }
    write_translation(&c, tr_table, owner_col, section, id, lang, form)?;
    Ok(true)
}

/// Upserts the translation row for one language, so editing in Thai cannot
/// wipe the English text and the other way round.
fn write_translation(
    c: &Conn,
    tr_table: &str,
    owner_col: &str,
    section: &str,
    id: i64,
    lang: &str,
    form: &crate::routes::admin::EntityForm,
) -> rusqlite::Result<()> {
    let name = form.name.trim();
    let existing: Option<i64> = c
        .query_row(
            &format!("SELECT 1 FROM {tr_table} WHERE {owner_col} = ?1 AND lang = ?2"),
            params![id, lang],
            |r| r.get(0),
        )
        .ok();

    match section {
        "cookies" => {
            if existing.is_some() {
                c.execute(
                    &format!(
                        "UPDATE {tr_table} SET name = ?1, abilities = ?2, description = ?3,
                                power_plus = ?4, power_plus_requirement = ?5, unlock_goal = ?6
                          WHERE {owner_col} = ?7 AND lang = ?8"
                    ),
                    params![
                        name,
                        form.abilities,
                        form.description,
                        form.power_plus,
                        form.power_plus_requirement,
                        form.unlock_goal,
                        id,
                        lang
                    ],
                )?;
            } else {
                c.execute(
                    &format!(
                        "INSERT INTO {tr_table} ({owner_col}, lang, name, abilities, description,
                                power_plus, power_plus_requirement, unlock_goal)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
                    ),
                    params![
                        id,
                        lang,
                        name,
                        form.abilities,
                        form.description,
                        form.power_plus,
                        form.power_plus_requirement,
                        form.unlock_goal
                    ],
                )?;
            }
        }
        "pets" => {
            if existing.is_some() {
                c.execute(
                    &format!(
                        "UPDATE {tr_table} SET name = ?1, abilities = ?2, description = ?3
                          WHERE {owner_col} = ?4 AND lang = ?5"
                    ),
                    params![name, form.abilities, form.description, id, lang],
                )?;
            } else {
                c.execute(
                    &format!(
                        "INSERT INTO {tr_table} ({owner_col}, lang, name, abilities, description)
                         VALUES (?1, ?2, ?3, ?4, ?5)"
                    ),
                    params![id, lang, name, form.abilities, form.description],
                )?;
            }
        }
        _ => {
            if existing.is_some() {
                c.execute(
                    &format!(
                        "UPDATE {tr_table} SET name = ?1, description = ?2
                          WHERE {owner_col} = ?3 AND lang = ?4"
                    ),
                    params![name, form.description, id, lang],
                )?;
            } else {
                c.execute(
                    &format!(
                        "INSERT INTO {tr_table} ({owner_col}, lang, name, description)
                         VALUES (?1, ?2, ?3, ?4)"
                    ),
                    params![id, lang, name, form.description],
                )?;
            }
        }
    }
    Ok(())
}

/// What a treasure is tied to: the cookie or pet that unlocks it at max
/// level, and the base it evolved from. Empty for a treasure with neither.
#[derive(Debug, Clone, Default)]
pub struct TreasureLinks {
    pub unlock_section: &'static str,
    pub unlock_id: i64,
    pub unlock_name: String,
    pub unlock_image: Option<String>,
    pub base_id: i64,
    pub base_name: String,
    pub base_image: Option<String>,
}

impl TreasureLinks {
    pub fn has_unlock(&self) -> bool {
        self.unlock_id > 0 && !self.unlock_name.is_empty()
    }
    pub fn has_base(&self) -> bool {
        self.base_id > 0 && !self.base_name.is_empty()
    }
}

pub fn treasure_links(db: &Db, lang: &str, id: i64) -> rusqlite::Result<TreasureLinks> {
    let c = conn(db)?;
    let mut stmt = c.prepare(
        "SELECT unlock_cookie_id, unlock_pet_id, base_treasure_id FROM treasure WHERE treasure_id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(TreasureLinks::default());
    };
    let unlock_cookie: Option<i64> = row.get(0)?;
    let unlock_pet: Option<i64> = row.get(1)?;
    let base: Option<i64> = row.get(2)?;
    drop(rows);
    drop(stmt);
    drop(c);

    let mut out = TreasureLinks::default();
    if let Some(cid) = unlock_cookie.filter(|v| *v > 0) {
        if let Some((name, image)) = entity_link(db, lang, "cookie", cid) {
            out.unlock_section = "cookies";
            out.unlock_id = cid;
            out.unlock_name = name;
            out.unlock_image = image;
        }
    } else if let Some(pid) = unlock_pet.filter(|v| *v > 0) {
        if let Some((name, image)) = entity_link(db, lang, "pet", pid) {
            out.unlock_section = "pets";
            out.unlock_id = pid;
            out.unlock_name = name;
            out.unlock_image = image;
        }
    }
    if let Some(bid) = base.filter(|v| *v > 0) {
        if let Some((name, image)) = entity_link(db, lang, "treasure", bid) {
            out.base_id = bid;
            out.base_name = name;
            out.base_image = image;
        }
    }
    Ok(out)
}

/// Whether a treasure's blessed effect set differs from its normal one, and
/// so is worth a toggle on the detail page. The picker computes the same
/// thing over its own option list; this is the detail page's answer.
pub fn blessed_differs(effects: &[EffectLine]) -> bool {
    let normal: Vec<(&str, &Vec<String>)> = effects
        .iter()
        .filter(|e| !e.blessed)
        .map(|e| (e.text.as_str(), &e.values))
        .collect();
    let blessed: Vec<(&str, &Vec<String>)> = effects
        .iter()
        .filter(|e| e.blessed)
        .map(|e| (e.text.as_str(), &e.values))
        .collect();
    !blessed.is_empty() && !normal.is_empty() && normal != blessed
}

/// One combo row as the admin editor lists it: the pairing's own id, so a row
/// can be updated or removed, alongside the partner and the effect.
#[derive(Debug, Clone)]
pub struct CombiEditRow {
    pub id: i64,
    /// the entity on the other side; the editor links to it once the row
    /// gains an edit form of its own
    #[allow(dead_code)]
    pub partner_id: i64,
    pub partner_name: String,
    pub effect: String,
    pub is_hidden: bool,
}

pub fn combi_edit_rows(
    db: &Db,
    lang: &str,
    kind: &str,
    id: i64,
) -> rusqlite::Result<Vec<CombiEditRow>> {
    let (own_col, partner_col, partner_table, partner_id_col, partner_tr, partner_owner) =
        match kind {
            "cookies" => ("cookie_id", "pet_id", "pet", "pet_id", "pet_translation", "pet_id"),
            "pets" => ("pet_id", "cookie_id", "cookie", "cookie_id", "cookie_translation", "owner_id"),
            _ => return Ok(Vec::new()),
        };
    let c = conn(db)?;
    let sql = format!(
        "SELECT cb.id AS id, p.{partner_id_col} AS partner_id,
                COALESCE(tl.name, te.name, '') AS partner_name,
                COALESCE(el.name, ee.name, '') AS effect,
                cb.is_hidden AS is_hidden
           FROM combi_bonus cb
           JOIN {partner_table} p ON p.{partner_id_col} = cb.{partner_col}
           LEFT JOIN {partner_tr} tl ON tl.{partner_owner} = p.{partner_id_col} AND tl.lang = ?1
           LEFT JOIN {partner_tr} te ON te.{partner_owner} = p.{partner_id_col} AND te.lang = 'en'
           LEFT JOIN effect_translation el ON el.effect_id = cb.effect_id AND el.lang = ?1
           LEFT JOIN effect_translation ee ON ee.effect_id = cb.effect_id AND ee.lang = 'en'
          WHERE cb.{own_col} = ?2
          ORDER BY cb.id"
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params![lang, id], |r| {
        Ok(CombiEditRow {
            id: r.get("id")?,
            partner_id: r.get("partner_id")?,
            partner_name: r.get("partner_name")?,
            effect: r.get("effect")?,
            is_hidden: r.get::<_, i64>("is_hidden")? != 0,
        })
    })?;
    rows.collect()
}

/// Removes one combo pairing. The editor's only destructive action, so it
/// takes the row id rather than a pair of entity ids — deleting by pair would
/// take every duplicate with it.
pub fn delete_combi(db: &Db, row_id: i64) -> rusqlite::Result<()> {
    let c = conn(db)?;
    c.execute("DELETE FROM combi_bonus WHERE id = ?1", params![row_id])?;
    Ok(())
}
