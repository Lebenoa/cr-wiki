//! SurrealDB access. The app never embeds a storage engine: every query goes
//! over the wire (`ws://` or `http://`) to an external server configured in
//! Config.toml's `[surreal]` section.
//!
//! Modeling notes (this is a document store, not SQL):
//! - translations live NESTED inside each entity as `tr: { en: {...}, th:
//!   {...} }`, so the locale-fallback joins collapse into a path read:
//!   `(tr[$lang].name ?? tr.en.name)`
//! - a treasure's effect lines are denormalized onto the record as
//!   `effect_lines: [{ state, en, th, values: [..+0..+9] }]`
//! - ids are numeric (`cookie:123`); `record::id(id)` projects them back to
//!   plain integers so routes keep their `/section/:id` shape
//! - graded entities carry a maintained `rank` column (grade::rank), because
//!   ordering by display rank must happen in the database

use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

use crate::config::SurrealConfig;
use crate::grade;

pub type Db = Surreal<Any>;

pub type Result<T> = surrealdb::Result<T>;

/// Connects to the external server and selects the namespace/database. No
/// local engine exists behind this — an unreachable URL is a hard startup
/// error.
pub async fn connect(cfg: &SurrealConfig) -> Result<Db> {
    connect_url(&cfg.url, &cfg.namespace, &cfg.database, &cfg.username, &cfg.password).await
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
        db.signin(Root { username, password }).await?;
    }
    db.use_ns(ns).use_db(database).await?;
    Ok(db)
}

/// The largest numeric id in a table, 0 when empty. New records take max+1;
/// writes are admin-only and rare, so the unguarded read-modify-write is fine.
async fn next_id(db: &Db, table: &str) -> Result<i64> {
    let mut res = db
        .query("LET $ids = (SELECT VALUE record::id(id) FROM type::table($tb)); RETURN array::max($ids) ?? 0;")
        .bind(("tb", table.to_string()))
        .await?;
    let max = res.take::<Option<i64>>(1)?;
    Ok(max.unwrap_or(0))
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

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CardRow {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    en_name: String,
    image: Option<String>,
    grade: Option<i64>,
    #[serde(default)]
    is_evolved: bool,
}

impl From<CardRow> for Card {
    fn from(r: CardRow) -> Self {
        Card {
            id: r.id,
            name: r.name,
            en_name: r.en_name,
            image: r.image,
            grade: r.grade,
            is_evolved: r.is_evolved,
        }
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

/// Which table a catalog section reads, plus whether it grades its rows and
/// stamps release dates. Every list/detail/search query branches through
/// this instead of carrying per-section SQL.
pub(crate) struct Kind {
    pub(crate) table: &'static str,
    pub(crate) graded: bool,
    pub(crate) dated: bool,
}

fn kind_of(section: &str) -> Option<Kind> {
    let (table, graded, dated) = match section {
        "cookies" => ("cookie", true, true),
        "pets" => ("pet", true, true),
        "treasures" => ("treasure", true, true),
        "episodes" => ("episode", false, false),
        "ingredients" => ("ingredient", true, false),
        "jellies" => ("jelly", false, false),
        "relics" => ("relic", false, false),
        "skins" => ("skin", true, false),
        _ => return None,
    };
    Some(Kind { table, graded, dated })
}

/// The card projection every list shares: locale-fallback name, English name
/// beside it, ordered by `order`.
async fn select_cards(
    db: &Db,
    lang: &str,
    kind: &Kind,
    extra_select: &str,
    where_sql: &str,
    order: &str,
    limit: Option<i64>,
    start: Option<i64>,
    needle: Option<String>,
) -> Result<Vec<Card>> {
    let mut sql = format!(
        "SELECT record::id(id) AS id, image{extra_select},
                (tr[$lang].name ?? tr.en.name) AS name, tr.en.name AS en_name
           FROM type::table($tb)"
    );
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(where_sql);
    }
    sql.push_str(order);
    if let Some(l) = limit {
        sql.push_str(&format!(" LIMIT {l}"));
    }
    if let Some(s) = start {
        sql.push_str(&format!(" START {s}"));
    }
    let mut q = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .bind(("tb", kind.table.to_string()));
    if let Some(n) = needle {
        q = q.bind(("needle", n));
    }
    let rows: Vec<CardRow> = q.await?.take(0)?;
    Ok(rows.into_iter().map(Card::from).collect())
}

/// Newest release date first with the id as the tie-break. Without that
/// second key pages tied on release_date could repeat or skip between
/// offset=0 and offset=30 fetches.
pub async fn select_cookies(db: &Db, lang: &str, limit: i64, offset: i64) -> Result<Vec<Card>> {
    select_cards(
        db,
        lang,
        &kind_of("cookies").expect("static"),
        ", grade, release_date",
        "tr.en.name != NONE OR tr[$lang].name != NONE",
        " ORDER BY release_date DESC, id DESC",
        Some(limit),
        Some(offset),
        None,
    )
    .await
}

pub async fn select_pets(db: &Db, lang: &str, limit: i64, offset: i64) -> Result<Vec<Card>> {
    select_cards(
        db,
        lang,
        &kind_of("pets").expect("static"),
        ", grade, release_date",
        "tr.en.name != NONE OR tr[$lang].name != NONE",
        " ORDER BY release_date DESC, id DESC",
        Some(limit),
        Some(offset),
        None,
    )
    .await
}

/// Grade (highest first), then newest, then name. `tab` is all/normal/evo.
/// Rank comes from the maintained column: the enum ordinal is NOT the display
/// order (E outranks L).
pub async fn select_treasures(
    db: &Db,
    lang: &str,
    tab: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Card>> {
    let tab_where = match tab {
        "normal" => " AND is_evolved = false",
        "evo" => " AND is_evolved = true",
        _ => "",
    };
    let where_sql = format!(
        "(tr.en.name != NONE OR tr[$lang].name != NONE){tab_where}"
    );
    select_cards(
        db,
        lang,
        &kind_of("treasures").expect("static"),
        ", grade, is_evolved, rank, release_date",
        &where_sql,
        " ORDER BY rank DESC, release_date DESC, name ASC",
        Some(limit),
        Some(offset),
        None,
    )
    .await
}

/// The simple catalogs, which share a shape and are unpaginated in the V app.
/// Graded grids order rarest-first by display rank, not the raw column.
pub async fn select_simple(db: &Db, lang: &str, kind: &str) -> Result<Vec<Card>> {
    let kind = match kind {
        "episodes" => Kind { table: "episode", graded: false, dated: false },
        "ingredients" => Kind { table: "ingredient", graded: true, dated: false },
        "jellies" => Kind { table: "jelly", graded: false, dated: false },
        "skins" => Kind { table: "skin", graded: true, dated: false },
        "relics" => Kind { table: "relic", graded: false, dated: false },
        _ => return Ok(Vec::new()),
    };
    let order = if kind.graded {
        " ORDER BY rank DESC, id".to_string()
    } else {
        " ORDER BY id".to_string()
    };
    let extra = if kind.graded { ", grade, rank" } else { "" };
    select_cards(
        db,
        lang,
        &kind,
        extra,
        "tr.en.name != NONE OR tr[$lang].name != NONE",
        &order,
        None,
        None,
        None,
    )
    .await
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct DetailRow {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    en_name: String,
    image: Option<String>,
    grade: Option<i64>,
    #[serde(default)]
    abilities: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    power_plus: String,
    #[serde(default)]
    power_plus_requirement: String,
    #[serde(default)]
    unlock_goal: String,
    #[serde(default)]
    release_date: Option<i64>,
}

/// One entity's detail row. Prose falls back locale -> English inside the
/// nested translation object; fields a kind does not have come back empty.
pub async fn select_detail(db: &Db, lang: &str, section: &str, id: i64) -> Result<Option<Detail>> {
    let Some(kind) = kind_of(section) else {
        return Ok(None);
    };
    let sql = format!(
        "SELECT record::id(id) AS id, image{extra}, release_date,
                (tr[$lang].name ?? tr.en.name ?? '') AS name,
                (tr.en.name ?? '') AS en_name,
                (tr[$lang].abilities ?? tr.en.abilities ?? '') AS abilities,
                (tr[$lang].description ?? tr.en.description ?? '') AS description,
                (tr[$lang].power_plus ?? tr.en.power_plus ?? '') AS power_plus,
                (tr[$lang].power_plus_requirement ?? tr.en.power_plus_requirement ?? '') AS ppr,
                (tr[$lang].unlock_goal ?? tr.en.unlock_goal ?? '') AS unlock_goal
           FROM type::table($tb)
          WHERE record::id(id) = $id
          LIMIT 1",
        extra = if kind.graded { ", grade" } else { "" },
    );
    let mut rows: Vec<DetailRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .bind(("tb", kind.table.to_string()))
        .bind(("id", id))
        .await?
        .take(0)?;
    Ok(rows.pop().map(|r| Detail {
        id: r.id,
        name: r.name,
        en_name: r.en_name,
        image: r.image,
        grade: if kind.graded { r.grade } else { None },
        abilities: r.abilities,
        description: r.description,
        power_plus: r.power_plus,
        power_plus_requirement: r.power_plus_requirement,
        unlock_goal: r.unlock_goal,
        release_date: if kind.dated { r.release_date.unwrap_or(0) } else { 0 },
    }))
}

/// The treasure a cookie or pet unlocks, if any — the reverse of the link
/// the treasure page shows. `kind` is "cookie" or "pet".
pub async fn unlocked_treasure(
    db: &Db,
    lang: &str,
    kind: &str,
    id: i64,
) -> Result<Option<(i64, String, Option<String>)>> {
    let col = match kind {
        "cookie" => "unlock_cookie_id",
        "pet" => "unlock_pet_id",
        _ => return Ok(None),
    };
    let sql = format!(
        "SELECT record::id(id) AS id, image,
                (tr[$lang].name ?? tr.en.name ?? '') AS name
           FROM treasure
          WHERE {col} = $id
          LIMIT 1"
    );
    let mut rows: Vec<CardRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .bind(("id", id))
        .await?
        .take(0)?;
    Ok(rows.pop().map(|r| (r.id, r.name, r.image)))
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

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct EffectLineRow {
    state: i64,
    #[serde(default)]
    en: String,
    #[serde(default)]
    th: String,
    #[serde(default)]
    values: Vec<String>,
}

impl EffectLineRow {
    fn into_line(self, lang: &str) -> EffectLine {
        let text = match lang == "th" && !self.th.is_empty() {
            true => self.th.clone(),
            false => self.en.clone(),
        };
        EffectLine {
            text,
            values: self.values,
            blessed: self.state == 1,
        }
    }
}

/// A treasure's effect lines with their 0-9 ladders, normal and blessed,
/// deduped and in wiki order — all stored on the record itself.
pub async fn treasure_effects(db: &Db, lang: &str, id: i64) -> Result<Vec<EffectLine>> {
    #[derive(serde::Deserialize)]
    struct Row {
        #[serde(default)]
        effect_lines: Vec<EffectLineRow>,
    }
    let mut rows: Vec<Row> = db
        .query("SELECT effect_lines FROM type::thing(\"treasure\", $id)")
        .bind(("id", id))
        .await?
        .take(0)?;
    Ok(rows
        .pop()
        .map(|r| r.effect_lines.into_iter().map(|e| e.into_line(lang)).collect())
        .unwrap_or_default())
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

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct CombiRecord {
    /// the pairing's own numeric id, so the admin editor can delete one row
    id: i64,
    cookie_id: i64,
    pet_id: i64,
    #[serde(default)]
    en: String,
    #[serde(default)]
    th: String,
    #[serde(default)]
    is_hidden: bool,
}

/// Name lookup for a batch of ids in one query, locale fallback applied.
/// Returns a map keyed by id so callers stitch pairs together.
pub(crate) async fn names_for(
    db: &Db,
    lang: &str,
    kind: &Kind,
    ids: &[i64],
) -> Result<std::collections::HashMap<i64, (String, Option<String>)>> {
    let mut map = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    let list = ids.iter().map(|i| format!("{}:{}", kind.table, i)).collect::<Vec<_>>();
    let sql = format!(
        "SELECT record::id(id) AS id, image,
                (tr[$lang].name ?? tr.en.name ?? '') AS name
           FROM {}
          WHERE id IN [{}]",
        kind.table,
        list.join(", ")
    );
    let rows: Vec<CardRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take(0)?;
    for r in rows {
        map.insert(r.id, (r.name, r.image));
    }
    Ok(map)
}

/// The combo bonuses a cookie or pet takes part in, partner resolved.
async fn combis(db: &Db, lang: &str, own: &str, id: i64) -> Result<Vec<(CombiRecord, CombiRow)>> {
    if id <= 0 || own != "cookies" {
        // combi pairings hang off cookies only; other sections list none
        return Ok(Vec::new());
    }
    let mut records: Vec<CombiRecord> = db
        .query(&format!(
            "SELECT record::id(id) AS id, cookie_id, pet_id, en, th, is_hidden
               FROM combi WHERE cookie_id = $id ORDER BY id"
        ))
        .bind(("id", id))
        .await?
        .take(0)?;

    let partner_section = if own == "cookies" { "pets" } else { "cookies" };
    let partner_kind = kind_of(partner_section).expect("static");
    let ids: Vec<i64> = records
        .iter()
        .map(|c| if own == "cookies" { c.pet_id } else { c.cookie_id })
        .collect();
    let partners = names_for(db, lang, &partner_kind, &ids).await?;

    Ok(records
        .drain(..)
        .map(|c| {
            let partner_id = if own == "cookies" { c.pet_id } else { c.cookie_id };
            let (partner_name, partner_image) =
                partners.get(&partner_id).cloned().unwrap_or_default();
            let effect = match lang == "th" && !c.th.is_empty() {
                true => c.th.clone(),
                false => c.en.clone(),
            };
            let row = CombiRow {
                partner_id,
                partner_name,
                partner_image,
                effect,
                is_hidden: c.is_hidden,
            };
            (c, row)
        })
        .collect())
}

pub async fn combi_bonuses(db: &Db, lang: &str, kind: &str, id: i64) -> Result<Vec<CombiRow>> {
    Ok(combis(db, lang, kind, id)
        .await?
        .into_iter()
        .map(|(_, row)| row)
        .collect())
}

/// Name and sprite for a rich-text link, resolved like everywhere else:
/// requested locale first, English behind it.
pub async fn entity_link(db: &Db, lang: &str, kind: &str, id: i64) -> Option<(String, Option<String>)> {
    let section = match kind {
        "pet" => "pets",
        "treasure" => "treasures",
        _ => "cookies",
    };
    let k = kind_of(section)?;
    let mut rows: Vec<CardRow> = db
        .query(&format!(
            "SELECT record::id(id) AS id, image,
                    (tr[$lang].name ?? tr.en.name ?? '') AS name,
                    (tr.en.name ?? '') AS en_name
               FROM {}
              WHERE record::id(id) = $id
              LIMIT 1",
            k.table
        ))
        .bind(("lang", lang.to_string()))
        .bind(("id", id))
        .await
        .ok()?
        .take(0)
        .ok()?;
    let row = rows.pop()?;
    let name = if row.name.is_empty() { row.en_name } else { row.name };
    if name.is_empty() {
        None
    } else {
        Some((name, row.image))
    }
}

/// Every (section, id) pair the sitemap lists.
pub async fn sitemap_entries(db: &Db) -> Result<Vec<(String, i64)>> {
    let mut out = Vec::new();
    for (section, table) in [
        ("cookies", "cookie"),
        ("pets", "pet"),
        ("treasures", "treasure"),
        ("episodes", "episode"),
        ("ingredients", "ingredient"),
        ("jellies", "jelly"),
    ] {
        let ids: Vec<i64> = db
            .query(&format!("SELECT VALUE record::id(id) FROM {table}"))
            .await?
            .take(0)?;
        out.extend(ids.into_iter().map(|id| (section.to_string(), id)));
    }
    Ok(out)
}

/// Cross-entity search over the localized and English names. Substring
/// matching rather than a full-text index for the same reason the V app kept
/// LIKE: Thai has no word breaks, so a tokenizer misses terms a substring
/// finds. Both sides are lowercased because `~` is case-sensitive.
pub async fn search(db: &Db, lang: &str, q: &str, limit: i64) -> Result<Vec<(String, Card)>> {
    let q = q.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let needle = q.to_lowercase();
    let mut out = Vec::new();
    // the columns searched mirror the old FTS tables: a cookie is findable by
    // its abilities text, not just its name
    for (section, prose) in [
        ("cookies", vec!["abilities", "description"]),
        ("pets", vec!["description"]),
        ("treasures", vec!["description"]),
        ("relics", vec!["description"]),
        ("episodes", vec!["description"]),
        ("ingredients", vec!["description"]),
    ] {
        let kind = kind_of(section).expect("known section");
        let mut clauses = vec![
            "string::lowercase(tr.en.name) CONTAINS $needle".to_string(),
            format!("string::lowercase(tr[$lang].name ?? '') CONTAINS $needle"),
        ];
        for col in prose {
            clauses.push(format!(
                "string::lowercase(tr.en.{col} ?? '') CONTAINS $needle"
            ));
            clauses.push(format!(
                "string::lowercase(tr[$lang].{col} ?? '') CONTAINS $needle"
            ));
        }
        let extra = if kind.graded { ", grade" } else { "" };
        let cards = select_cards(
            db,
            lang,
            &kind,
            extra,
            &clauses.join(" OR "),
            " ORDER BY name ASC",
            Some(limit),
            None,
            Some(needle.clone()),
        )
        .await?;
        out.extend(cards.into_iter().map(|card| (section.to_string(), card)));
    }
    Ok(out)
}

/// A user row, for the session layer.
#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password: String,
    pub is_admin: bool,
}

#[derive(serde::Deserialize)]
struct UserRow {
    id: i64,
    username: String,
    password: String,
    #[serde(default)]
    is_admin: bool,
}

pub async fn find_user(db: &Db, username: &str) -> Result<Option<User>> {
    let mut rows: Vec<UserRow> = db
        .query("SELECT record::id(id) AS id, username, password, is_admin FROM user WHERE username = $u LIMIT 1")
        .bind(("u", username.to_string()))
        .await?
        .take(0)?;
    Ok(rows.pop().map(|r| User {
        id: r.id,
        username: r.username,
        password: r.password,
        is_admin: r.is_admin,
    }))
}

/// Creates a user with an already-hashed password. `None` when the username
/// is taken, which the unique index enforces rather than a prior SELECT.
pub async fn create_user(db: &Db, username: &str, password_hash: &str) -> Result<Option<User>> {
    let id = next_id(db, "user").await? + 1;
    let res = db
        .query("CREATE type::thing(\"user\", $id) SET username = $u, password = $p, is_admin = false, created_at = time::unix();")
        .bind(("id", id))
        .bind(("u", username.to_string()))
        .bind(("p", password_hash.to_string()))
        .await?;
    match res.check() {
        Ok(_) => Ok(Some(User {
            id,
            username: username.to_string(),
            password: password_hash.to_string(),
            is_admin: false,
        })),
        // a duplicate username trips the unique index; the form says taken
        Err(e) => {
            if e.to_string().contains("already contains") {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

/// Hydrates an explicit id list, preserving the caller's order — which is the
/// ranking, so it has to survive the round trip.
pub async fn cards_by_ids(db: &Db, lang: &str, kind: &str, ids: &[i64]) -> Result<Vec<Card>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let k = match kind_of(kind) {
        Some(k) => k,
        None => return Ok(Vec::new()),
    };
    let list = ids.iter().map(|i| format!("{}:{}", k.table, i)).collect::<Vec<_>>();
    let sql = format!(
        "SELECT record::id(id) AS id, image{}, (tr[$lang].name ?? tr.en.name ?? '') AS name,
                (tr.en.name ?? '') AS en_name
           FROM {}
          WHERE id IN [{}]",
        if k.graded { ", grade" } else { "" },
        k.table,
        list.join(", ")
    );
    let found: Vec<Card> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take::<Vec<CardRow>>(0)?
        .into_iter()
        .map(Card::from)
        .collect();
    let mut ordered = Vec::with_capacity(found.len());
    for id in ids {
        if let Some(card) = found.iter().find(|c| c.id == *id) {
            ordered.push(card.clone());
        }
    }
    Ok(ordered)
}

/// The ids that pair with `kind` id `id` for a combo bonus. Just the ids: the
/// picker floats them to the top of the other slot's grid and needs neither
/// the names, the sprites nor the effect text.
pub async fn combi_partner_ids(db: &Db, kind: &str, id: i64) -> Result<Vec<i64>> {
    if id <= 0 {
        return Ok(Vec::new());
    }
    let own_col = if kind == "cookies" { "cookie_id" } else { "pet_id" };
    let partner_col = if kind == "cookies" { "pet_id" } else { "cookie_id" };
    let mut res = db
        .query(&format!(
            "SELECT VALUE DISTINCT {partner_col} FROM combi WHERE {own_col} = $id"
        ))
        .bind(("id", id))
        .await?;
    res.take(0)
}

/// One draw pool with its disclosed odds, as the gacha page lists them.
#[derive(Debug, Clone)]
pub struct GachaPool {
    pub id: i64,
    /// the tier slug, which the .tr key is built from
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

#[derive(serde::Deserialize)]
struct GachaPoolRow {
    id: i64,
    #[serde(default)]
    tier: String,
    #[serde(default)]
    entries: Vec<GachaEntryRef>,
}

#[derive(serde::Deserialize)]
struct GachaEntryRef {
    t: Option<i64>,
    p: Option<i64>,
    odds: f64,
}

/// Every pool with its entries in the catalog's own order. The prize name is
/// resolved through the same locale fallback as everywhere else: pools store
/// references, names come from one batched lookup per prize kind.
pub async fn select_gacha(db: &Db, lang: &str) -> Result<Vec<GachaPool>> {
    let pools: Vec<GachaPoolRow> = db
        .query("SELECT record::id(id) AS id, tier, entries FROM gacha_pool ORDER BY id")
        .await?
        .take(0)?;

    let mut treasure_ids: Vec<i64> = Vec::new();
    let mut pet_ids: Vec<i64> = Vec::new();
    for p in &pools {
        for e in &p.entries {
            if let Some(t) = e.t.filter(|v| *v > 0) {
                if !treasure_ids.contains(&t) {
                    treasure_ids.push(t);
                }
            }
            if let Some(pet) = e.p.filter(|v| *v > 0) {
                if !pet_ids.contains(&pet) {
                    pet_ids.push(pet);
                }
            }
        }
    }

    #[derive(serde::Deserialize)]
    struct PrizeRow {
        id: i64,
        image: Option<String>,
        grade: Option<i64>,
        #[serde(default)]
        is_evolved: bool,
        #[serde(default)]
        name: String,
        #[serde(default)]
        en_name: String,
    }
    async fn prizes(db: &Db, lang: &str, table: &str, ids: &[i64]) -> Result<std::collections::HashMap<i64, PrizeRow>> {
        let mut map = std::collections::HashMap::new();
        if ids.is_empty() {
            return Ok(map);
        }
        let list = ids.iter().map(|i| format!("{table}:{i}")).collect::<Vec<_>>();
        let rows: Vec<PrizeRow> = db
            .query(&format!(
                "SELECT record::id(id) AS id, image, grade, (is_evolved ?? false) AS is_evolved,
                        (tr[$lang].name ?? tr.en.name ?? '') AS name,
                        (tr.en.name ?? '') AS en_name
                   FROM {table}
                  WHERE id IN [{}]",
                list.join(", ")
            ))
            .bind(("lang", lang.to_string()))
            .await?
            .take(0)?;
        for r in rows {
            map.insert(r.id, r);
        }
        Ok(map)
    }

    let treasures = prizes(db, lang, "treasure", &treasure_ids).await?;
    let pets = prizes(db, lang, "pet", &pet_ids).await?;

    let mut out = Vec::with_capacity(pools.len());
    for p in pools {
        let mut entries = Vec::with_capacity(p.entries.len());
        for e in p.entries {
            let (section, id) = match (e.t.filter(|v| *v > 0), e.p.filter(|v| *v > 0)) {
                (Some(t), _) => ("treasures", t),
                (_, Some(pet)) => ("pets", pet),
                _ => continue,
            };
            let prize = if section == "treasures" {
                treasures.get(&id)
            } else {
                pets.get(&id)
            };
            let Some(prize) = prize else { continue };
            entries.push(GachaEntry {
                section,
                id,
                name: prize.name.clone(),
                en_name: prize.en_name.clone(),
                image: prize.image.clone(),
                grade: prize.grade,
                odds: e.odds,
                is_evolved: prize.is_evolved,
            });
        }
        out.push(GachaPool { id: p.id, tier: p.tier, entries });
    }
    Ok(out)
}

/// Table names for one catalog section.
fn entity_table(section: &str) -> Option<&'static str> {
    match section {
        "cookies" => Some("cookie"),
        "pets" => Some("pet"),
        "treasures" => Some("treasure"),
        _ => None,
    }
}

/// Creates an entity and its translation in `lang`, returning the new id.
pub async fn insert_entity(
    db: &Db,
    lang: &str,
    section: &str,
    form: &crate::routes::admin::EntityForm,
) -> Result<i64> {
    let Some(table) = entity_table(section) else {
        return Ok(0);
    };
    let id = next_id(db, table).await? + 1;
    let now = now_unix();
    let image = clean_image(&form.image);

    let mut sets: Vec<String> = vec!["image = $image".to_string()];
    let graded = matches!(section, "cookies" | "pets" | "treasures" | "ingredients" | "skins");
    if graded {
        // display rank is maintained on write so reads can ORDER BY it:
        // the enum ordinal is not the display order (E outranks L)
        let g = form.grade.unwrap_or(1);
        sets.push(format!("rank = {}", grade::rank(g)));
        sets.push(format!("grade = {g}"));
        sets.push(format!("release_date = {now}"));
    }
    if section == "treasures" {
        sets.push("is_evolved = false".to_string());
    }
    db.query(&format!("CREATE {table}:{id} SET {}", sets.join(", ")))
        .bind(("image", image))
        .await?
        .check()?;
    write_translation(db, lang, section, table, id, form).await?;
    Ok(id)
}

/// Updates an entity and its translation in `lang`.
pub async fn update_entity(
    db: &Db,
    lang: &str,
    section: &str,
    id: i64,
    form: &crate::routes::admin::EntityForm,
) -> Result<bool> {
    let Some(table) = entity_table(section) else {
        return Ok(false);
    };
    let image = clean_image(&form.image);
    let mut sets = Vec::new();
    if let Some(g) = form.grade {
        sets.push(format!("rank = {}", grade::rank(g)));
        sets.push(format!("grade = {g}"));
    }
    if image.is_some() {
        sets.push("image = $image".to_string());
    }
    if !sets.is_empty() {
        db.query(&format!("UPDATE {table}:{id} SET {}", sets.join(", ")))
            .bind(("image", image))
            .await?
            .check()?;
    }
    write_translation(db, lang, section, table, id, form).await?;
    Ok(true)
}

fn clean_image(image: &str) -> Option<String> {
    let trimmed = image.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

/// Upserts the translation for one language INSIDE the nested tr object, so
/// editing in Thai cannot wipe the English text and the other way round.
async fn write_translation(
    db: &Db,
    lang: &str,
    section: &str,
    table: &str,
    id: i64,
    form: &crate::routes::admin::EntityForm,
) -> Result<()> {
    // merge client-side: fetch existing tr.$lang, overlay the submitted
    // fields, write the whole object back — deterministic regardless of how
    // many languages exist
    #[derive(Default, serde::Deserialize, serde::Serialize)]
    struct Tr {
        #[serde(default)]
        name: String,
        #[serde(default)]
        abilities: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        power_plus: String,
        #[serde(default)]
        power_plus_requirement: String,
        #[serde(default)]
        unlock_goal: String,
    }
    #[derive(Default, serde::Deserialize)]
    struct TrWrap {
        #[serde(default)]
        tr: std::collections::BTreeMap<String, Tr>,
    }
    let mut wrap: TrWrap = db
        .query("SELECT tr FROM type::thing($tb, $id)")
        .bind(("tb", table.to_string()))
        .bind(("id", id))
        .await?
        .take::<Vec<TrWrap>>(0)?
        .pop()
        .unwrap_or_default();
    let entry = wrap.tr.entry(lang.to_string()).or_default();
    entry.name = form.name.trim().to_string();
    match section {
        "cookies" => {
            entry.abilities = form.abilities.clone();
            entry.description = form.description.clone();
            entry.power_plus = form.power_plus.clone();
            entry.power_plus_requirement = form.power_plus_requirement.clone();
            entry.unlock_goal = form.unlock_goal.clone();
        }
        "pets" => {
            entry.abilities = form.abilities.clone();
            entry.description = form.description.clone();
        }
        _ => entry.description = form.description.clone(),
    }
    db.query("UPDATE type::thing($tb, $id) SET tr = $tr")
        .bind(("tb", table.to_string()))
        .bind(("id", id))
        .bind(("tr", wrap.tr))
        .await?
        .check()?;
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

pub async fn treasure_links(db: &Db, lang: &str, id: i64) -> Result<TreasureLinks> {
    #[derive(Default, serde::Deserialize)]
    struct Row {
        unlock_cookie_id: Option<i64>,
        unlock_pet_id: Option<i64>,
        base_treasure_id: Option<i64>,
    }
    let mut rows: Vec<Row> = db
        .query("SELECT unlock_cookie_id, unlock_pet_id, base_treasure_id FROM type::thing(\"treasure\", $id)")
        .bind(("id", id))
        .await?
        .take(0)?;
    let Some(row) = rows.pop() else {
        return Ok(TreasureLinks::default());
    };

    let mut out = TreasureLinks::default();
    if let Some(cid) = row.unlock_cookie_id.filter(|v| *v > 0) {
        if let Some((name, image)) = entity_link(db, lang, "cookie", cid).await {
            out.unlock_section = "cookies";
            out.unlock_id = cid;
            out.unlock_name = name;
            out.unlock_image = image;
        }
    } else if let Some(pid) = row.unlock_pet_id.filter(|v| *v > 0) {
        if let Some((name, image)) = entity_link(db, lang, "pet", pid).await {
            out.unlock_section = "pets";
            out.unlock_id = pid;
            out.unlock_name = name;
            out.unlock_image = image;
        }
    }
    if let Some(bid) = row.base_treasure_id.filter(|v| *v > 0) {
        if let Some((name, image)) = entity_link(db, lang, "treasure", bid).await {
            out.base_id = bid;
            out.base_name = name;
            out.base_image = image;
        }
    }
    Ok(out)
}

/// Whether a treasure's blessed effect set differs from its normal one, and
/// so is worth a toggle on the detail page.
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

pub async fn combi_edit_rows(
    db: &Db,
    lang: &str,
    kind: &str,
    id: i64,
) -> Result<Vec<CombiEditRow>> {
    let rows = combis(db, lang, kind, id).await?;
    Ok(rows
        .into_iter()
        .map(|(record, row)| CombiEditRow {
            id: record.id,
            partner_id: row.partner_id,
            partner_name: row.partner_name,
            effect: row.effect,
            is_hidden: row.is_hidden,
        })
        .collect())
}

/// Removes one combo pairing by record id.
pub async fn delete_combi(db: &Db, row_id: i64) -> Result<()> {
    db.query("DELETE type::thing(\"combi\", $id)")
        .bind(("id", row_id))
        .await?
        .check()?;
    Ok(())
}
