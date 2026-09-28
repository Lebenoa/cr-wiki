//! `SurrealDB` access. The app never embeds a storage engine: every query goes
//! over the wire (`ws://` or `http://`) to an external server configured in
//! `Config.toml`'s `[surreal]` section.
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
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

use crate::config::SurrealConfig;
use crate::grade;
use crate::section::Section;
use crate::time::now_unix;

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

/// Ensures the indexes needed by search exist for seeded and upgraded databases.
/// The application authenticates with the deployment's configured root user.
///
/// Also defines the per-table name-gram ingest events: whenever an entity's
/// translation lands (CREATE with tr, or the UPDATE write_translation sends
/// right after), the event replaces that entity's name_gram rows — old
/// name's grams deleted, new name's 3-grams inserted. seed.surql seeds the
/// rows themselves (events never fire on `surreal import`); this keeps
/// admin-created and renamed entities fresh on live databases.
async fn ensure_search_schema(db: &Db) -> Result<()> {
    db.query("DEFINE ANALYZER IF NOT EXISTS cookie_search_en TOKENIZERS class, punct FILTERS lowercase;")
        .await?
        .check()?;
    db.query("DEFINE ANALYZER IF NOT EXISTS cookie_search_th TOKENIZERS class FILTERS lowercase, ngram(1,3);")
        .await?
        .check()?;
    let indexes = [
        ("cookie", ["tr.en.name", "tr.th.name", "tr.en.abilities", "tr.th.abilities", "tr.en.description", "tr.th.description"].as_slice()),
        ("pet", ["tr.en.name", "tr.th.name", "tr.en.description", "tr.th.description"].as_slice()),
        ("treasure", ["tr.en.name", "tr.th.name", "tr.en.description", "tr.th.description"].as_slice()),
        ("relic", ["tr.en.name", "tr.th.name", "tr.en.description", "tr.th.description"].as_slice()),
        ("episode", ["tr.en.name", "tr.th.name", "tr.en.description", "tr.th.description"].as_slice()),
        ("ingredient", ["tr.en.name", "tr.th.name", "tr.en.description", "tr.th.description"].as_slice()),
    ];
    for (table, fields) in indexes {
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
    // the name-gram ingest events. The guard accepts any event that leaves
    // an en name behind — CREATE-with-tr and write_translation's UPDATE —
    // and the body first deletes the entity's existing gram rows, so a
    // rename cannot leave stale grams behind. seed.surql defines the same
    // statements for fresh imports.
    for (table, _) in indexes {
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
        .chain(["user", "build"])
    {
        query.push_str(&format!(
            "DEFINE SEQUENCE IF NOT EXISTS {table}_seq START 1;\n"
        ));
    }
    db.query(query).await?.check()?;
    Ok(())
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

impl grade::Graded for Card {
    fn grade(&self) -> Option<i64> {
        self.grade
    }
}

#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
struct CardRow {
    id: i64,
    name: String,
    en_name: String,
    image: Option<String>,
    grade: Option<i64>,
    is_evolved: bool,
    score: Option<f64>,
}

impl From<CardRow> for Card {
    fn from(r: CardRow) -> Self {
        Self {
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
    /// treasures only: the evolved row links to its base, the base row to
    /// its evolution
    pub is_evolved: bool,
    /// treasures only: the row describes the Power+ variant
    pub is_power_plus: bool,
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

impl grade::Graded for Detail {
    fn grade(&self) -> Option<i64> {
        self.grade
    }
}

impl Detail {
    /// The release date as the edit form's `<input type=date>` wants it:
    /// YYYY-MM-DD, or empty when the row never got a date.
    pub fn release_date_input(&self) -> String {
        if self.release_date <= 0 {
            return String::new();
        }
        let days = self.release_date.div_euclid(86400);
        // inverse days-from-civil (Howard Hinnant's algorithm)
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}-{m:02}-{d:02}")
    }
}

/// Which table a catalog section reads, plus whether it grades its rows and
/// stamps release dates. Every list/detail/search query branches through
/// this instead of carrying per-section SQL.
pub struct Kind {
    pub table: &'static str,
    pub graded: bool,
    pub dated: bool,
}

impl Kind {
    /// The facts one section's queries need, read from the one place each
    /// section's table and flags live (`Section`).
    pub const fn of(section: Section) -> Kind {
        Kind {
            table: section.table(),
            graded: section.graded(),
            dated: section.dated(),
        }
    }

    /// The typed path's other half: an untyped `&str` (a path capture, a
    /// build row's column, a caller that has not parsed yet) resolves here.
    pub fn of_section(section: &str) -> Option<Kind> {
        Section::parse(section).map(Kind::of)
    }
}

/// The locale fallback every translated column reads through, as a SQL
/// fragment: the requested locale first, English behind it. Written once so
/// the fallback rule cannot drift between queries — it used to appear 23
/// times across two files. Pub(crate) because the picker lists read the same
/// tables through the same rule.
pub(crate) fn tr(field: &str) -> String {
    format!("(tr[$lang].{field} ?? tr.en.{field} ?? '')")
}

/// The card projection every list shares: locale-fallback name, English name
/// beside it, ordered by `order`. Every argument is one axis the callers
/// actually vary — whitelisted fragments, never request text.
#[allow(clippy::too_many_arguments)]
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
                {} AS name, tr.en.name AS en_name
           FROM type::table($tb)",
        tr("name")
    );
    if !where_sql.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(where_sql);
    }
    sql.push_str(order);
    if let Some(l) = limit {
        sql.push_str(" LIMIT ");
        sql.push_str(&l.to_string());
    }
    if let Some(s) = start {
        sql.push_str(" START ");
        sql.push_str(&s.to_string());
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
/// second key pages tied on `release_date` could repeat or skip between
/// `offset=0` and `offset=30` fetches. Unknown dates (0, the seed
/// sentinel) sort last via `date_unknown` — the list is "newest known
/// first", not "oldest first".
pub async fn select_cookies(db: &Db, lang: &str, limit: i64, offset: i64) -> Result<Vec<Card>> {
    cards(
        db,
        lang,
        Section::Cookies,
        ", grade, (release_date <= 0) AS date_unknown",
        " ORDER BY date_unknown ASC, release_date DESC, id DESC",
        Some((limit, offset)),
        "",
    )
    .await
}

pub async fn select_pets(db: &Db, lang: &str, limit: i64, offset: i64) -> Result<Vec<Card>> {
    cards(
        db,
        lang,
        Section::Pets,
        ", grade, (release_date <= 0) AS date_unknown",
        " ORDER BY date_unknown ASC, release_date DESC, id DESC",
        Some((limit, offset)),
        "",
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
    let tab_extra = match tab {
        "normal" => " AND is_evolved = false",
        "evo" => " AND is_evolved = true",
        _ => "",
    };
    cards(
        db,
        lang,
        Section::Treasures,
        ", grade, is_evolved, rank, release_date",
        " ORDER BY rank DESC, release_date DESC, name ASC",
        Some((limit, offset)),
        tab_extra,
    )
    .await
}

/// The simple catalogs, which share a shape and are unpaginated in the V app.
/// Graded grids order rarest-first by display rank, not the raw column.
pub async fn select_simple(db: &Db, lang: &str, kind: &str) -> Result<Vec<Card>> {
    // the paginated three list through select_cookies/pets/treasures and
    // must not fall into this unpaginated shape
    let Some(section) = Section::parse(kind).filter(|s| !s.paginated()) else {
        return Ok(Vec::new());
    };
    cards(db, lang, section, "", " ORDER BY id", None, "").await
}

// --- the five simple catalogs' own views ----------------------------------
//
// The shared `Card` projection carries what a cookie card needs. Episodes,
// ingredients, jellies, skins and relics each show more, so each has one
// view struct and one query here — same locale-fallback rule (`tr`), same
// "unknown field is NONE" reading the rest of the module uses.

/// One episode as the /episodes browser lists it: the display fields plus
/// the three collection counts the V page shows.
#[derive(Debug, Clone, Default)]
pub struct EpisodeList {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub kind: String,
    pub stars: i64,
    pub league_ranked: bool,
    pub entry_cost: String,
    pub stage_count: i64,
    pub quest_count: i64,
    pub relic_count: i64,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct EpisodeListRow {
    id: i64,
    name: String,
    en_name: String,
    image: Option<String>,
    kind: String,
    stars: i64,
    league_ranked: bool,
    entry_cost: String,
    stage_count: i64,
    quest_count: i64,
    relic_count: i64,
}

/// Episodes in id order, which is the kind order the browser wants: story
/// 1-7, then special 501+, then event 601+ (the id ranges encode it — same
/// invariant `episode_short` reads its abbreviation from).
pub async fn select_episode_list(db: &Db, lang: &str) -> Result<Vec<EpisodeList>> {
    // The child counts ride along as pre-grouped maps instead of correlated
    // subqueries. On SurrealDB v3 a subquery whose right-hand side mentions
    // `$parent` is re-planned per outer row and never touches an index, so the
    // old shape cost a full child-table scan per episode per child kind
    // (~92 ms against a 13-row episode table with ~3k quests). One grouped
    // scan per child table plus an in-memory array lookup is ~1 ms total.
    // `record::id(id)` inside the closures reads the row's id before the
    // `record::id(id) AS id` projection shadows it with the plain integer.
    let sql = format!(
        "LET $stages = (SELECT episode_id, count() AS c FROM episode_stage GROUP BY episode_id);
         LET $quests = (SELECT episode_id, count() AS c FROM quest GROUP BY episode_id);
         LET $relics = (SELECT episode_id, count() AS c FROM episode_relic GROUP BY episode_id);
         SELECT record::id(id) AS id, image, kind, (stars ?? 0) AS stars,
                (league_ranked ?? false) AS league_ranked, (entry_cost ?? '') AS entry_cost,
                {name} AS name, (tr.en.name ?? '') AS en_name,
                array::find($stages, |$x| $x.episode_id = record::id(id)).c ?? 0 AS stage_count,
                array::find($quests, |$x| $x.episode_id = record::id(id)).c ?? 0 AS quest_count,
                array::find($relics, |$x| $x.episode_id = record::id(id)).c ?? 0 AS relic_count
           FROM episode
          ORDER BY id",
        name = tr("name")
    );
    let rows: Vec<EpisodeListRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take(3)?;
    Ok(rows
        .into_iter()
        .map(|r| EpisodeList {
            id: r.id,
            name: r.name,
            en_name: r.en_name,
            image: r.image,
            kind: r.kind,
            stars: r.stars,
            league_ranked: r.league_ranked,
            entry_cost: r.entry_cost,
            stage_count: r.stage_count,
            quest_count: r.quest_count,
            relic_count: r.relic_count,
        })
        .collect())
}

/// One stage pill on the episode detail page.
#[derive(Debug, Clone, Default)]
pub struct EpisodeStage {
    pub stage_no: i64,
    pub name: String,
}

/// One relic of the episode's completion set.
#[derive(Debug, Clone, Default)]
pub struct EpisodeRelicCard {
    pub name: String,
    pub image: Option<String>,
}

/// One quest row, grouped by its chain (`group`).
#[derive(Debug, Clone, Default)]
pub struct EpisodeQuest {
    pub name: String,
    pub reward: String,
}

/// A quest chain in source order, preserving the seed's group order.
#[derive(Debug, Clone, Default)]
pub struct EpisodeQuestGroup {
    pub group: String,
    pub quests: Vec<EpisodeQuest>,
}

/// One relic-draw reward with its disclosed odds.
#[derive(Debug, Clone, Default)]
pub struct DrawReward {
    pub rank: i64,
    pub reward: String,
    pub odds: f64,
}

impl DrawReward {
    /// The odds as the page prints them: no padding, so 7.69 reads "7.69%".
    pub fn odds_label(&self) -> String {
        odds_pct(self.odds)
    }
}

/// One mystery-box grade row with the three disclosed box columns.
#[derive(Debug, Clone, Default)]
pub struct BoxGrade {
    pub box_grade: String,
    pub first: f64,
    pub second: f64,
    pub third: f64,
}

impl BoxGrade {
    pub fn first_label(&self) -> String {
        odds_pct(self.first)
    }
    pub fn second_label(&self) -> String {
        odds_pct(self.second)
    }
    pub fn third_label(&self) -> String {
        odds_pct(self.third)
    }
}

/// Row projections for the episode detail child queries.
#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct StageRow {
    stage_no: i64,
    name: String,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct RelicCardRow {
    rid: i64,
    name: String,
    image: Option<String>,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct QuestRow {
    grp: String,
    name: String,
    reward: String,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct DrawRow {
    rank: i64,
    reward: String,
    odds: f64,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct BoxRow {
    box_grade: String,
    box_no: i64,
    odds: f64,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct IngRow {
    id: i64,
    name: String,
    image: Option<String>,
    grade: Option<i64>,
    coin_value: i64,
}

/// Odds as "4.4%" — the stored value with trailing zeros trimmed.
fn odds_pct(v: f64) -> String {
    let mut s = format!("{}", v);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    format!("{s}%")
}

/// One ingredient that drops in this episode.
#[derive(Debug, Clone, Default)]
pub struct EpisodeIngredient {
    pub name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub coin_value: i64,
}

/// One episode's full detail page data: the row itself plus its six child
/// sections (stages, relic set, quest chains, draw rewards, box odds,
/// ingredient drops), as `templates/detail.html`'s episode branch renders
/// them. Mirrors the old `select_episode` shape.
#[derive(Debug, Clone, Default)]
pub struct EpisodeDetail {
    pub kind: String,
    pub stars: i64,
    pub league_ranked: bool,
    pub entry_cost: String,
    pub stages: Vec<EpisodeStage>,
    pub relics: Vec<EpisodeRelicCard>,
    pub quest_groups: Vec<EpisodeQuestGroup>,
    pub quest_count: i64,
    pub draw_rewards: Vec<DrawReward>,
    pub box_grades: Vec<BoxGrade>,
    pub ingredients: Vec<EpisodeIngredient>,
}

/// Everything the episode detail page shows besides name/description/image
/// (which `select_detail` already carries). Child tables are read with one
/// grouped query each; names resolve locale -> English.
pub async fn episode_extras(db: &Db, lang: &str, id: i64) -> Result<EpisodeDetail> {
    let mut out = EpisodeDetail::default();

    let mut parents: Vec<EpisodeListRow> = db
        .query(
            "SELECT (kind ?? '') AS kind, (stars ?? 0) AS stars,
                    (league_ranked ?? false) AS league_ranked, (entry_cost ?? '') AS entry_cost
               FROM type::record('episode', $id)",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    if let Some(p) = parents.pop() {
        out.kind = p.kind;
        out.stars = p.stars;
        out.league_ranked = p.league_ranked;
        out.entry_cost = p.entry_cost;
    }

    let mut stages: Vec<StageRow> = db
        .query(
            "SELECT stage_no, (name ?? '') AS name FROM episode_stage
              WHERE episode_id = $id ORDER BY stage_no",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    out.stages = stages
        .drain(..)
        .map(|s| EpisodeStage {
            stage_no: s.stage_no,
            name: s.name,
        })
        .collect();

    // the completion relic set: the junction's plain-int relic_ids point at
    // relic records whose record id IS that number (relic:500101), so the
    // names resolve with one query over the id list
    let relic_ids: Vec<i64> = db
        .query("SELECT VALUE relic_id FROM episode_relic WHERE episode_id = $id ORDER BY relic_id")
        .bind(("id", id))
        .await?
        .take(0)?;
    if !relic_ids.is_empty() {
        let mut relic_rows: Vec<RelicCardRow> = db
            .query(
                format!(
                    "SELECT record::id(id) AS rid, image,
                            {name} AS name
                       FROM relic WHERE record::id(id) IN $ids",
                    name = tr("name"),
                )
                .as_str(),
            )
            .bind(("lang", lang.to_string()))
            .bind(("ids", relic_ids))
            .await?
            .take(0)?;
        // junction order (relic_id ascending), not the query's return order
        relic_rows.sort_by_key(|r| r.rid);
        out.relics = relic_rows
            .into_iter()
            .map(|r| EpisodeRelicCard {
                name: r.name,
                image: r.image,
            })
            .collect();
    }

    // quest chains in source order, grouped by their `group` field
    let mut quests: Vec<QuestRow> = db
        .query(
            "SELECT (group ?? '') AS grp, (name ?? '') AS name, (reward ?? '') AS reward
               FROM quest WHERE episode_id = $id ORDER BY id",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    out.quest_count = quests.len() as i64;
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<EpisodeQuest>> =
        std::collections::HashMap::new();
    for q in quests.drain(..) {
        if !groups.contains_key(&q.grp) {
            order.push(q.grp.clone());
        }
        groups.entry(q.grp).or_default().push(EpisodeQuest {
            name: q.name,
            reward: q.reward,
        });
    }
    out.quest_groups = order
        .into_iter()
        .map(|g| EpisodeQuestGroup {
            quests: groups.remove(&g).unwrap_or_default(),
            group: g,
        })
        .collect();

    let mut draws: Vec<DrawRow> = db
        .query(
            "SELECT rank, (reward ?? '') AS reward, (odds ?? 0) AS odds
               FROM episode_draw_reward WHERE episode_id = $id ORDER BY rank",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    out.draw_rewards = draws
        .drain(..)
        .map(|d| DrawReward {
            rank: d.rank,
            reward: d.reward,
            odds: d.odds,
        })
        .collect();

    // one row per box grade with the three disclosed box columns; a grade
    // missing a box keeps 0 for it, which prints as "0%"
    let mut boxes: Vec<BoxRow> = db
        .query(
            "SELECT (box_grade ?? '') AS box_grade, box_no, (odds ?? 0) AS odds
               FROM episode_box_odds WHERE episode_id = $id ORDER BY box_no",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    let mut grade_order: Vec<String> = Vec::new();
    let mut by_grade: std::collections::HashMap<String, BoxGrade> =
        std::collections::HashMap::new();
    for b in boxes.drain(..) {
        if !by_grade.contains_key(&b.box_grade) {
            grade_order.push(b.box_grade.clone());
            by_grade.insert(
                b.box_grade.clone(),
                BoxGrade {
                    box_grade: b.box_grade.clone(),
                    first: 0.0,
                    second: 0.0,
                    third: 0.0,
                },
            );
        }
        let g = by_grade.get_mut(&b.box_grade).expect("just inserted");
        match b.box_no {
            1 => g.first = b.odds,
            2 => g.second = b.odds,
            _ => g.third = b.odds,
        }
    }
    out.box_grades = grade_order
        .into_iter()
        .filter_map(|g| by_grade.remove(&g))
        .collect();

    // the ingredients that drop in this episode, id order like the old page
    let mut ings: Vec<IngRow> = db
        .query(
            format!(
                "SELECT record::id(id) AS id, image, (grade ?? NONE) AS grade,
                        (coin_value ?? 0) AS coin_value,
                        {name} AS name
                   FROM ingredient WHERE drop_episode_id = $id ORDER BY id",
                name = tr("name"),
            )
            .as_str(),
        )
        .bind(("lang", lang.to_string()))
        .bind(("id", id))
        .await?
        .take(0)?;
    ings.sort_by_key(|i| i.id);
    out.ingredients = ings
        .into_iter()
        .map(|i| EpisodeIngredient {
            name: i.name,
            image: i.image,
            grade: i.grade,
            coin_value: i.coin_value,
        })
        .collect();

    Ok(out)
}
#[derive(Debug, Clone, Default)]
pub struct IngredientList {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub drop_episode_id: Option<i64>,
    pub recipe_count: i64,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct IngredientListRow {
    id: i64,
    name: String,
    en_name: String,
    image: Option<String>,
    grade: Option<i64>,
    drop_episode_id: Option<i64>,
    recipe_count: Option<i64>,
}

/// Grade first (rarest down to commonest, ungraded last), then drop episode
/// in play order (the id ranges ascend story -> special -> event), then id.
/// The ingredients that drop everywhere carry no episode id and sort after
/// the ones that name an episode, which needs the explicit NONE check.
///
/// Recipe counts arrive as their own grouped query and are joined in Rust:
/// a `$parent`-correlated subquery re-plans per row on v3 (~0.4 s for 242
/// ingredients), and an in-SQL `array::find` join costs ~130 ms because the
/// closure scan is linear per row. Two scans + a HashMap join stay in single
/// digits.
pub async fn select_ingredient_list(db: &Db, lang: &str) -> Result<Vec<IngredientList>> {
    let sql = format!(
        "SELECT record::id(id) AS id, image, grade, drop_episode_id,
                (drop_episode_id IS NONE) AS no_episode,
                (IF grade IS NONE THEN -1 ELSE rank END) AS display_rank,
                {name} AS name, (tr.en.name ?? '') AS en_name
           FROM ingredient
          ORDER BY display_rank DESC, no_episode, drop_episode_id, id",
        name = tr("name")
    );
    let mut rows: Vec<IngredientListRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take(0)?;
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct CountRow {
        ingredient_id: i64,
        c: i64,
    }
    let counts: Vec<CountRow> = db
        .query("SELECT ingredient_id, count() AS c FROM ingredient_recipe GROUP BY ingredient_id")
        .await?
        .take(0)?;
    let by_ingredient: std::collections::HashMap<i64, i64> =
        counts.into_iter().map(|c| (c.ingredient_id, c.c)).collect();
    for row in &mut rows {
        row.recipe_count = Some(by_ingredient.get(&row.id).copied().unwrap_or(0));
    }
    Ok(rows
        .into_iter()
        .map(|r| IngredientList {
            id: r.id,
            name: r.name,
            en_name: r.en_name,
            image: r.image,
            grade: r.grade,
            drop_episode_id: r.drop_episode_id,
            recipe_count: r.recipe_count.unwrap_or(0),
        })
        .collect())
}

/// One jelly as the /jellies grid lists it.
#[derive(Debug, Clone, Default)]
pub struct JellyList {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub score: f64,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct JellyListRow {
    id: i64,
    name: String,
    en_name: String,
    image: Option<String>,
    score: f64,
}

pub async fn select_jelly_list(db: &Db, lang: &str) -> Result<Vec<JellyList>> {
    let sql = format!(
        "SELECT record::id(id) AS id, image, (score ?? 0) AS score,
                {} AS name, (tr.en.name ?? '') AS en_name
           FROM jelly
          ORDER BY score DESC, id",
        tr("name")
    );
    let rows: Vec<JellyListRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take(0)?;
    Ok(rows
        .into_iter()
        .map(|r| JellyList {
            id: r.id,
            name: r.name,
            en_name: r.en_name,
            image: r.image,
            score: r.score,
        })
        .collect())
}

/// One skin as the /skins grid lists it.
#[derive(Debug, Clone, Default)]
pub struct SkinList {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub collab: bool,
    pub subtitle: String,
    pub owner_id: i64,
    pub owner_section: &'static str,
    pub owner_name: String,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct SkinListRow {
    id: i64,
    name: String,
    en_name: String,
    image: Option<String>,
    grade: Option<i64>,
    collab: bool,
    subtitle: String,
    cookie_id: Option<i64>,
    pet_id: Option<i64>,
    cookie_name: String,
    pet_name: String,
}

/// Skins in id order; the owner (a cookie or a pet) resolves its own name.
/// The owner names come from pre-built maps rather than per-row correlated
/// subqueries — see `select_relic_groups` for why: v3 re-plans a `$parent`
/// subquery per row, costing a full cookie/pet scan per skin.
pub async fn select_skin_list(db: &Db, lang: &str) -> Result<Vec<SkinList>> {
    let sql = format!(
        "LET $cn = (SELECT record::id(id) AS cid, {cname} AS name FROM cookie);
         LET $pn = (SELECT record::id(id) AS pid, {pname} AS name FROM pet);
         SELECT record::id(id) AS id, image, grade, (collab ?? false) AS collab,
                (subtitle ?? '') AS subtitle,
                {name} AS name, (tr.en.name ?? '') AS en_name,
                cookie_id, pet_id,
                (array::find($cn, |$x| $x.cid = cookie_id).name ?? '') AS cookie_name,
                (array::find($pn, |$x| $x.pid = pet_id).name ?? '') AS pet_name
           FROM skin
          ORDER BY id",
        name = tr("name"),
        cname = tr("name"),
        pname = tr("name")
    );
    let rows: Vec<SkinListRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take(2)?;
    Ok(rows
        .into_iter()
        .map(|r| SkinList {
            id: r.id,
            name: r.name,
            en_name: r.en_name,
            image: r.image,
            grade: r.grade,
            collab: r.collab,
            subtitle: r.subtitle,
            owner_id: r.cookie_id.unwrap_or(0).max(r.pet_id.unwrap_or(0)),
            owner_section: if r.cookie_id.unwrap_or(0) > 0 {
                "cookies"
            } else {
                "pets"
            },
            owner_name: if r.cookie_id.unwrap_or(0) > 0 {
                r.cookie_name
            } else {
                r.pet_name
            },
        })
        .collect())
}

/// One relic as the /relics catalog lists it: the display row plus, for
/// event relics, the cookie whose pass unlocked it.
#[derive(Debug, Clone, Default)]
pub struct RelicList {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub description: String,
    pub unlock_cookie_id: i64,
    pub unlock_cookie_name: String,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct RelicListRow {
    id: i64,
    name: String,
    en_name: String,
    image: Option<String>,
    description: String,
    episode_id: i64,
    unlock_cookie_id: i64,
    unlock_cookie_name: String,
}

/// Relics grouped under their owning episode, in episode order (which is the
/// kind order), event relics last under an empty episode name.
#[derive(Debug, Clone, Default)]
pub struct RelicGroup {
    pub episode_id: i64,
    pub episode_name: String,
    pub relics: Vec<RelicList>,
}

pub async fn select_relic_groups(db: &Db, lang: &str) -> Result<Vec<RelicGroup>> {
    // The unlocking cookie's name resolves through one pre-built map (id ->
    // name) instead of a per-relic correlated subquery: v3 re-plans a
    // `$parent` subquery per row and never uses an index, so 85 relics cost
    // ~45 ms of repeated cookie-table scans. The map lookup keeps it <20 ms
    // (94-row map scanned 85 times) without a second round trip.
    let sql = format!(
        "LET $cookie_names = (SELECT record::id(id) AS cid, {cname} AS name FROM cookie);
         SELECT record::id(id) AS id, image, (episode_id ?? 0) AS episode_id,
                (unlock_cookie_id ?? 0) AS unlock_cookie_id,
                {name} AS name, (tr.en.name ?? '') AS en_name,
                {description} AS description,
                (array::find($cookie_names, |$x| $x.cid = unlock_cookie_id).name ?? '')
                    AS unlock_cookie_name
           FROM relic
          ORDER BY id",
        name = tr("name"),
        cname = tr("name"),
        description = tr("description"),
    );
    let rows: Vec<RelicListRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take(1)?;
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct EpisodeName {
        id: i64,
        name: String,
    }
    let ep_sql = format!(
        "SELECT record::id(id) AS id, {} AS name FROM episode ORDER BY id",
        tr("name")
    );
    let ep_rows: Vec<EpisodeName> = db
        .query(&ep_sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take(0)?;
    let ep_names: std::collections::HashMap<i64, String> =
        ep_rows.into_iter().map(|e| (e.id, e.name)).collect();

    let mut groups: Vec<RelicGroup> = Vec::new();
    let mut by_ep: std::collections::HashMap<i64, Vec<RelicList>> =
        std::collections::HashMap::new();
    let mut event: Vec<RelicList> = Vec::new();
    for r in rows {
        let relic = RelicList {
            id: r.id,
            name: r.name,
            en_name: r.en_name,
            image: r.image,
            description: r.description,
            unlock_cookie_id: r.unlock_cookie_id,
            unlock_cookie_name: r.unlock_cookie_name,
        };
        if r.episode_id > 0 {
            by_ep.entry(r.episode_id).or_default().push(relic);
        } else {
            event.push(relic);
        }
    }
    let mut eps: Vec<i64> = by_ep.keys().copied().collect();
    eps.sort_unstable();
    for eid in eps {
        let relics = by_ep.remove(&eid).unwrap_or_default();
        groups.push(RelicGroup {
            episode_id: eid,
            episode_name: ep_names.get(&eid).cloned().unwrap_or_default(),
            relics,
        });
    }
    if !event.is_empty() {
        groups.push(RelicGroup {
            episode_id: 0,
            episode_name: String::new(),
            relics: event,
        });
    }
    Ok(groups)
}

/// The one card-list interface the read module presents: a section, the
/// locale, ordering, an optional page window, and an optional whitelist
/// conjunction beyond the always-on name filter. `Kind` (table, graded,
/// dated), the projection and the base WHERE all come from the section
/// inside, so callers never touch per-section SQL.
#[allow(clippy::too_many_arguments)]
async fn cards(
    db: &Db,
    lang: &str,
    section: Section,
    extra_select: &str,
    order: &str,
    page: Option<(i64, i64)>,
    extra_filter: &str,
) -> Result<Vec<Card>> {
    let kind = Kind::of(section);
    let extra = if kind.graded { ", grade, rank" } else { "" };
    // the base filter is parenthesized: SurrealQL binds AND tighter than OR,
    // so `A OR B AND tab` would read `A OR (B AND tab)` and leak every named
    // row into a tabbed query
    let filter = if extra_filter.is_empty() {
        "(tr.en.name != NONE OR tr[$lang].name != NONE)".to_string()
    } else {
        format!("(tr.en.name != NONE OR tr[$lang].name != NONE){extra_filter}")
    };
    select_cards(
        db,
        lang,
        &kind,
        &format!("{extra}{extra_select}"),
        &filter,
        order,
        page.map(|(l, _)| l),
        page.map(|(_, s)| s),
        None,
    )
    .await
}

#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
struct DetailRow {
    id: i64,
    name: String,
    en_name: String,
    image: Option<String>,
    grade: Option<i64>,
    is_evolved: Option<bool>,
    is_power_plus: Option<bool>,
    abilities: String,
    description: String,
    power_plus: String,
    power_plus_requirement: String,
    unlock_goal: String,
    release_date: Option<i64>,
}

/// One entity's detail row. Prose falls back locale -> English inside the
/// nested translation object; fields a kind does not have come back empty.
pub async fn select_detail(db: &Db, lang: &str, section: &str, id: i64) -> Result<Option<Detail>> {
    let Some(kind) = Kind::of_section(section) else {
        return Ok(None);
    };
    let sql = format!(
        "SELECT record::id(id) AS id, image{extra}, release_date,
                {name} AS name,
                (tr.en.name ?? '') AS en_name,
                {abilities} AS abilities,
                {description} AS description,
                {power_plus} AS power_plus,
                {ppr} AS ppr,
                {unlock_goal} AS unlock_goal
           FROM type::record($tb, $id)",
        name = tr("name"),
        abilities = tr("abilities"),
        description = tr("description"),
        power_plus = tr("power_plus"),
        ppr = tr("power_plus_requirement"),
        unlock_goal = tr("unlock_goal"),
        extra = if kind.graded {
            ", grade, (is_evolved ?? false) AS is_evolved, (is_power_plus ?? false) AS is_power_plus"
        } else {
            ""
        },
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
        is_evolved: r.is_evolved.unwrap_or(false),
        is_power_plus: r.is_power_plus.unwrap_or(false),
        abilities: r.abilities,
        description: r.description,
        power_plus: r.power_plus,
        power_plus_requirement: r.power_plus_requirement,
        unlock_goal: r.unlock_goal,
        release_date: if kind.dated {
            r.release_date.unwrap_or(0)
        } else {
            0
        },
    }))
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct MakerRow {
    entity_kind: String,
    entity_id: i64,
}

/// One entity that produces a jelly: kind plus resolved name/image, as the
/// jelly detail's Makers grid links it.
#[derive(Debug, Clone, Default)]
pub struct JellyMaker {
    /// "cookie" | "pet" | "treasure" — the sprite dir is kind + "s"
    pub kind: String,
    pub id: i64,
    pub name: String,
    pub image: Option<String>,
}

/// The entities that produce one jelly, in seed order. Names resolve
/// locale -> English through each table's translation object.
pub async fn jelly_makers(db: &Db, lang: &str, jelly_id: i64) -> Result<Vec<JellyMaker>> {
    let mut rows: Vec<MakerRow> = db
        .query(
            "SELECT entity_kind, entity_id FROM jelly_maker
              WHERE jelly_id = $id ORDER BY id",
        )
        .bind(("id", jelly_id))
        .await?
        .take(0)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows.drain(..) {
        let kind = row.entity_kind;
        let id = row.entity_id;
        let section = match kind.as_str() {
            "pet" => "pets",
            "treasure" => "treasures",
            _ => "cookies",
        };
        let Some((name, image)) = entity_link(db, lang, &kind, id).await else {
            continue;
        };
        out.push(JellyMaker {
            kind: section.to_string(),
            id,
            name,
            image,
        });
    }
    Ok(out)
}

/// A treasure row's evolution/unlock link ids, as the edit form preselects
/// them. Zeros mean "no link" — the form renders those selects with a None
/// option first.
#[derive(Debug, Clone, Copy, Default)]
pub struct TreasureEditLinks {
    pub base_treasure_id: i64,
    pub unlock_cookie_id: i64,
    pub unlock_pet_id: i64,
}

/// Reads the three link ids for the treasure editor in one query.
pub async fn treasure_edit_links(db: &Db, id: i64) -> Result<TreasureEditLinks> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct Row {
        base_treasure_id: i64,
        unlock_cookie_id: i64,
        unlock_pet_id: i64,
    }
    let mut rows: Vec<Row> = db
        .query(
            "SELECT (base_treasure_id ?? 0) AS base_treasure_id,
                    (unlock_cookie_id ?? 0) AS unlock_cookie_id,
                    (unlock_pet_id ?? 0) AS unlock_pet_id
               FROM type::record('treasure', $id)",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    Ok(rows
        .pop()
        .map(|r| TreasureEditLinks {
            base_treasure_id: r.base_treasure_id,
            unlock_cookie_id: r.unlock_cookie_id,
            unlock_pet_id: r.unlock_pet_id,
        })
        .unwrap_or_default())
}

/// The editor's row loader: strictly the requested locale's translation, no
/// English fallback. An admin opening the form for `th` must see what is
/// actually stored for `th` — silently showing the English text would make
/// the first save overwrite a locale with a translation that only looked
/// already-translated. Fields the locale has never saved come back empty.
pub async fn select_detail_strict(
    db: &Db,
    lang: &str,
    section: &str,
    id: i64,
) -> Result<Option<Detail>> {
    let Some(kind) = Kind::of_section(section) else {
        return Ok(None);
    };
    let sql = format!(
        "SELECT record::id(id) AS id, image{extra}, release_date,
                (tr[$lang].name ?? '') AS name,
                (tr.en.name ?? '') AS en_name,
                (tr[$lang].abilities ?? '') AS abilities,
                (tr[$lang].description ?? '') AS description,
                (tr[$lang].power_plus ?? '') AS power_plus,
                (tr[$lang].power_plus_requirement ?? '') AS power_plus_requirement,
                (tr[$lang].unlock_goal ?? '') AS unlock_goal
           FROM type::record($tb, $id)",
        extra = if kind.graded {
            ", grade, (is_evolved ?? false) AS is_evolved, (is_power_plus ?? false) AS is_power_plus"
        } else {
            ""
        },
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
        is_evolved: r.is_evolved.unwrap_or(false),
        is_power_plus: r.is_power_plus.unwrap_or(false),
        abilities: r.abilities,
        description: r.description,
        power_plus: r.power_plus,
        power_plus_requirement: r.power_plus_requirement,
        unlock_goal: r.unlock_goal,
        release_date: if kind.dated {
            r.release_date.unwrap_or(0)
        } else {
            0
        },
    }))
}

// --- the kinds' own detail collections ------------------------------------
//
// One detail page is one `Detail` plus whichever collections its kind
// shows. These queries fill them; the route gathers whichever its section
// needs and the template decides where they render.

/// The facts an ingredient's detail page shows beyond the shared prose:
/// the drop tile fields and the powder economy numbers.
#[derive(Debug, Clone, Default)]
pub struct IngredientFacts {
    pub drop_episode_id: Option<i64>,
    pub drop_location: String,
    pub coin_value: i64,
    pub breaks_into_powder: i64,
    pub craft_from_powder: i64,
    pub obtained_from: String,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct IngredientFactsRow {
    drop_episode_id: Option<i64>,
    drop_location: String,
    coin_value: i64,
    breaks_into_powder: i64,
    craft_from_powder: i64,
    obtained_from: String,
    drop_episode_name: String,
}

/// The drop-location text prefers the episode's localized name over the
/// catalog's English drop text, so a Thai page reads the episode title in
/// Thai; the raw text covers the ingredients that drop in every episode.
pub async fn ingredient_facts(db: &Db, lang: &str, id: i64) -> Result<IngredientFacts> {
    let sql = format!(
        "SELECT drop_episode_id, (drop_location ?? '') AS drop_location,
                (coin_value ?? 0) AS coin_value,
                (breaks_into_powder ?? 0) AS breaks_into_powder,
                (craft_from_powder ?? 0) AS craft_from_powder,
                (obtained_from ?? '') AS obtained_from,
                ((SELECT VALUE {ename} FROM episode
                  WHERE id = type::record(\"episode\", $parent.drop_episode_id)
                  LIMIT 1)[0] ?? '')
                    AS drop_episode_name
           FROM type::record(\"ingredient\", $id)",
        ename = tr("name")
    );
    let mut rows: Vec<IngredientFactsRow> = db
        .query(&sql)
        .bind(("id", id))
        .bind(("lang", lang.to_string()))
        .await?
        .take(0)?;
    let Some(row) = rows.pop() else {
        return Ok(IngredientFacts::default());
    };
    Ok(IngredientFacts {
        drop_episode_id: row.drop_episode_id,
        drop_location: if row.drop_episode_name.is_empty() {
            row.drop_location
        } else {
            row.drop_episode_name
        },
        coin_value: row.coin_value,
        breaks_into_powder: row.breaks_into_powder,
        craft_from_powder: row.craft_from_powder,
        obtained_from: row.obtained_from,
    })
}

/// One treasure an ingredient crafts (from ingredient_recipe), with its
/// localized name.
#[derive(Debug, Clone, Default)]
pub struct CraftRecipe {
    pub treasure_id: i64,
    pub name: String,
    pub image: Option<String>,
}

pub async fn ingredient_recipes(db: &Db, lang: &str, id: i64) -> Result<Vec<CraftRecipe>> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct Row {
        treasure_id: i64,
        name: String,
        image: Option<String>,
    }
    let sql = format!(
        "LET $tn = (SELECT record::id(id) AS tid, {tname} AS name, image FROM treasure);
         SELECT treasure_id,
                (array::find($tn, |$x| $x.tid = treasure_id).name ?? '') AS name,
                array::find($tn, |$x| $x.tid = treasure_id).image AS image
           FROM ingredient_recipe
          WHERE ingredient_id = $id
          ORDER BY treasure_id",
        tname = tr("name")
    );
    let rows: Vec<Row> = db
        .query(&sql)
        .bind(("id", id))
        .bind(("lang", lang.to_string()))
        .await?
        .take(1)?;
    Ok(rows
        .into_iter()
        .map(|r| CraftRecipe {
            treasure_id: r.treasure_id,
            name: r.name,
            image: r.image,
        })
        .collect())
}

/// One ingredient a treasure is crafted from: the catalog row plus its grade
/// and drop episode, as the craft panel shows.
#[derive(Debug, Clone, Default)]
pub struct CraftIngredient {
    pub ingredient_id: i64,
    pub name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub drop_episode_id: Option<i64>,
}

pub async fn treasure_craft_ingredients(
    db: &Db,
    lang: &str,
    id: i64,
) -> Result<Vec<CraftIngredient>> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct Row {
        ingredient_id: i64,
        name: String,
        image: Option<String>,
        grade: Option<i64>,
        drop_episode_id: Option<i64>,
    }
    let sql = format!(
        "LET $ing = (SELECT record::id(id) AS iid, {iname} AS name, image, grade,
                            drop_episode_id
                       FROM ingredient);
         SELECT ingredient_id,
                (array::find($ing, |$x| $x.iid = ingredient_id).name ?? '') AS name,
                array::find($ing, |$x| $x.iid = ingredient_id).image AS image,
                array::find($ing, |$x| $x.iid = ingredient_id).grade AS grade,
                array::find($ing, |$x| $x.iid = ingredient_id).drop_episode_id
                    AS drop_episode_id
           FROM ingredient_recipe
          WHERE treasure_id = $id
          ORDER BY ingredient_id",
        iname = tr("name")
    );
    let rows: Vec<Row> = db
        .query(&sql)
        .bind(("id", id))
        .bind(("lang", lang.to_string()))
        .await?
        .take(1)?;
    Ok(rows
        .into_iter()
        .map(|r| CraftIngredient {
            ingredient_id: r.ingredient_id,
            name: r.name,
            image: r.image,
            grade: r.grade,
            drop_episode_id: r.drop_episode_id,
        })
        .collect())
}

/// The linked variant of a treasure: the base for an evolved row, the
/// evolved form for a base row. Only evolved rows carry `base_treasure_id`,
/// so a base row's variant is the evolved row pointing back at it. The two
/// are mutually exclusive, so one optional suffices; the template labels
/// the panel from `is_evolved`.
pub async fn treasure_variant(
    db: &Db,
    lang: &str,
    id: i64,
    is_evolved: bool,
) -> Result<Option<(i64, String, Option<String>)>> {
    let variant_id = if is_evolved {
        let mut rows: Vec<i64> = db
            .query("SELECT VALUE base_treasure_id FROM type::record(\"treasure\", $id)")
            .bind(("id", id))
            .await?
            .take(0)?;
        rows.pop()
    } else {
        None
    };
    let where_clause = if is_evolved {
        "WHERE record::id(id) = $variant_id"
    } else {
        "WHERE base_treasure_id = $id"
    };
    let sql = format!(
        "SELECT record::id(id) AS id, image, {tr_name} AS name,
                (tr.en.name ?? '') AS en_name
           FROM treasure
          {where_clause}
          LIMIT 1",
        tr_name = tr("name"),
    );
    let mut rows: Vec<CardRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .bind(("id", id))
        .bind(("variant_id", variant_id.unwrap_or(0)))
        .await?
        .take(0)?;
    Ok(rows.pop().map(|r| {
        let name = if r.name.is_empty() { r.en_name } else { r.name };
        (r.id, name, r.image)
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
                {} AS name
           FROM treasure
          WHERE {col} = $id
          LIMIT 1",
        tr("name")
    );
    let mut rows: Vec<CardRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .bind(("id", id))
        .await?
        .take(0)?;
    Ok(rows.pop().map(|r| (r.id, r.name, r.image)))
}

/// The effect-line row type `options` shares: same shape on the record, one
/// decode one locale rule. Kept pub(crate) so both read paths decode it the
/// same way.
#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
pub(crate) struct EffectLineRow {
    pub(crate) state: i64,
    pub(crate) en: String,
    pub(crate) th: String,
    pub(crate) values: Vec<String>,
}

impl EffectLineRow {
    pub(crate) fn into_line(self, lang: &str) -> EffectLine {
        let text = crate::i18n::pick_text(lang, &self.en, &self.th);
        EffectLine {
            text,
            values: self.values,
            blessed: self.state == 1,
        }
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

/// A treasure's effect lines with their 0-9 ladders, normal and blessed,
/// deduped and in wiki order — all stored on the record itself.
pub async fn treasure_effects(db: &Db, lang: &str, id: i64) -> Result<Vec<EffectLine>> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct Row {
        effect_lines: Vec<EffectLineRow>,
    }
    let mut rows: Vec<Row> = db
        .query("SELECT effect_lines FROM type::record(\"treasure\", $id)")
        .bind(("id", id))
        .await?
        .take(0)?;
    Ok(rows
        .pop()
        .map(|r| {
            r.effect_lines
                .into_iter()
                .map(|e| e.into_line(lang))
                .collect()
        })
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

#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
struct CombiRecord {
    /// the pairing's own numeric id, so the admin editor can delete one row
    id: i64,
    cookie_id: i64,
    pet_id: i64,
    en: String,
    th: String,
    is_hidden: bool,
}

/// Name lookup for a batch of ids in one query, locale fallback applied.
/// Returns a map keyed by id so callers stitch pairs together.
pub async fn names_for(
    db: &Db,
    lang: &str,
    kind: &Kind,
    ids: &[i64],
) -> Result<std::collections::HashMap<i64, (String, Option<String>)>> {
    let mut map = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    let sql = format!(
        "SELECT record::id(id) AS id, image,
                {} AS name
           FROM {}
          WHERE record::id(id) IN $ids",
        tr("name"),
        kind.table,
    );
    let rows: Vec<CardRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .bind(("ids", ids.to_vec()))
        .await?
        .take(0)?;
    for r in rows {
        map.insert(r.id, (r.name, r.image));
    }
    for id in ids {
        if !map.contains_key(id) {
            tracing::warn!(table = kind.table, id, lang, "name lookup matched no row; callers render an empty name and no thumbnail");
        }
    }
    Ok(map)
}

/// The combo bonuses a cookie or pet takes part in, partner resolved.
async fn combis(db: &Db, lang: &str, own: &str, id: i64) -> Result<Vec<(CombiRecord, CombiRow)>> {
    if id <= 0 || own != "cookies" {
        // combi pairings hang off cookies only; other sections list none
        return Ok(Vec::new());
    }
    let records: Vec<CombiRecord> = db
        .query(
            "SELECT record::id(id) AS id, cookie_id, pet_id, en, th, is_hidden
               FROM combi WHERE cookie_id = $id ORDER BY id",
        )
        .bind(("id", id))
        .await?
        .take(0)?;

    // pairings hang off cookies only (checked above), so the partner is
    // always a pet
    let partner_kind = Kind::of(Section::Pets);
    let ids: Vec<i64> = records
        .iter()
        .map(|c| {
            if own == "cookies" {
                c.pet_id
            } else {
                c.cookie_id
            }
        })
        .collect();
    let partners = names_for(db, lang, &partner_kind, &ids).await?;

    Ok(records
        .into_iter()
        .map(|c| {
            let partner_id = if own == "cookies" {
                c.pet_id
            } else {
                c.cookie_id
            };
            let (partner_name, partner_image) =
                partners.get(&partner_id).cloned().unwrap_or_default();
            let effect = crate::i18n::pick_text(lang, &c.en, &c.th);
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
pub async fn entity_link(
    db: &Db,
    lang: &str,
    kind: &str,
    id: i64,
) -> Option<(String, Option<String>)> {
    let section = match kind {
        "pet" => "pets",
        "treasure" => "treasures",
        _ => "cookies",
    };
    let k = Kind::of_section(section)?;
    let mut rows: Vec<CardRow> = db
        .query(
            format!(
                "SELECT record::id(id) AS id, image,
                        {name} AS name,
                        (tr.en.name ?? '') AS en_name
                   FROM type::record($tb, $id)",
                name = tr("name"),
            )
            .as_str(),
        )
        .bind(("lang", lang.to_string()))
        .bind(("tb", k.table))
        .bind(("id", id))
        .await
        .map_err(|e| {
            tracing::warn!(table = k.table, id, error = %e, "rich-text link lookup failed");
            e
        })
        .ok()?
        .take(0)
        .map_err(|e| {
            tracing::warn!(table = k.table, id, error = %e, "rich-text link lookup failed");
            e
        })
        .ok()?;
    let row = rows.pop()?;
    let name = if row.name.is_empty() {
        row.en_name
    } else {
        row.name
    };
    if name.is_empty() {
        None
    } else {
        Some((name, row.image))
    }
}

/// Every (section, id) pair the sitemap lists.
pub async fn sitemap_entries(db: &Db) -> Result<Vec<(String, i64)>> {
    // every section from the one list: relics and skins were missing here
    // when this list was hand-maintained, reachable "only by luck"
    let mut out = Vec::new();
    for section in Section::ALL {
        let ids: Vec<i64> = db
            .query(format!("SELECT VALUE record::id(id) FROM {}", section.table()).as_str())
            .await?
            .take(0)?;
        out.extend(ids.into_iter().map(|id| (section.as_str().to_string(), id)));
    }
    Ok(out)
}

/// Cross-entity search. The needle goes three ways per table, merged here:
/// - the full-text index ranks whole-word hits (`@1@`), one query per
///   locale pair, `search::score` ordering — same tool the V app's FTS5
///   tables gave it;
/// - the name-gram index answers fragments and typos in one mechanism
///   (`name_gram`, seeded with three-character grams of every catalog
///   name, maintained by the `*_gram` ingest events): the needle's own
///   grams are looked up and entities ranked by gram overlap. Prefix
///   (`ginger`), infix, Thai runs and transposed typos (`gingerbrvae`)
///   all reduce to gram overlap — the same trick Meilisearch's typo
///   tolerance is built on;
/// - a substring pass over description/abilities prose for needles the
///   grams cannot see (grams cover names only; prose is word-level).
/// The planner refuses `search::score()` in a WHERE that mixes MATCHES
/// with non-index clauses, so the passes cannot be OR'd in SQL; ranked
/// hits keep their order and the later passes fill the tail, deduped by id.
pub async fn search(db: &Db, lang: &str, q: &str, limit: i64) -> Result<Vec<(String, Card)>> {
    let query = q.trim();
    // one bound at the edge: the three passes below interpolate the limit,
    // so a caller cannot widen the result work with a huge value
    let limit = limit.clamp(1, 100);
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let needle = query.to_lowercase();
    let is_thai = needle
        .chars()
        .any(|c| matches!(u32::from(c) as u32, 0x0E00..=0x0E7F));
    let mut out: Vec<(String, Card)> = Vec::new();
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for (section, table, extra, prose) in [
        ("cookies", "cookie", ", grade", "abilities"),
        ("pets", "pet", ", grade", "description"),
        ("treasures", "treasure", ", grade", "description"),
        ("relics", "relic", "", "description"),
        ("episodes", "episode", "", "description"),
        ("ingredients", "ingredient", ", grade", "description"),
    ] {
        let mut clauses = Vec::with_capacity(if prose == "abilities" { 6 } else { 4 });
        for locale in ["en", lang] {
            clauses.push(format!("tr.{locale}.name @1@ $q"));
            clauses.push(format!("tr.{locale}.description @1@ $q"));
            if prose == "abilities" {
                clauses.push(format!("tr.{locale}.abilities @1@ $q"));
            }
        }
        let sql = format!(
            "SELECT record::id(id) AS id, image{extra}, {} AS name,
                    (tr.en.name ?? '') AS en_name, search::score(1) AS score
               FROM {table}
              WHERE {}
              ORDER BY score DESC, name ASC
              LIMIT {limit}",
            tr("name"),
            clauses.join(" OR "),
        );
        let rows: Vec<CardRow> = db
            .query(sql)
            .bind(("lang", lang.to_string()))
            .bind(("q", query.to_string()))
            .await?
            .take(0)?;
        for r in rows {
            let card = Card::from(r);
            if seen.insert(card.id) {
                out.push((section.to_string(), card));
            }
        }

        // the name-gram pass: the needle's 3-grams against the index, most
        // overlapping entities first. Grams live in one table for all
        // sections; the composite index covers the (gram, section) pair.
        // The subquery is materialized through LET first: an inline
        // `id IN (subquery)` re-executes the subquery per scanned row
        // (EXPLAIN: TableScan with "unsupported predicate" pre-decode),
        // 13 s per search on the seeded catalog; materialized, ~30 ms.
        let grams = name_grams(&needle);
        if !grams.is_empty() {
            let rows: Vec<CardRow> = db
                .query(format!(
                    "LET $ids = (SELECT VALUE entity_id FROM name_gram
                             WHERE section = $sec AND gram IN $grams
                             GROUP BY entity_id LIMIT {limit});
                     SELECT record::id(id) AS id, image{extra}, {} AS name,
                            (tr.en.name ?? '') AS en_name
                       FROM {table}
                      WHERE record::id(id) IN $ids",
                    tr("name")
                ))
                .bind(("sec", table.to_string()))
                .bind(("grams", grams.clone()))
                .await?
                // statement 0 is the LET, statement 1 the select
                .take(1)?;
            for r in rows {
                let card = Card::from(r);
                if seen.insert(card.id) {
                    out.push((section.to_string(), card));
                }
            }
        }

        // the prose pass: substring over description (and abilities), for
        // needles the name grams cannot see. Thai needles search the th
        // columns (the en text cannot contain them), latin needles search
        // en plus the viewer's locale.
        let columns: Vec<&str> = if is_thai {
            vec!["th"]
        } else {
            vec!["en", &lang[..]]
        };
        let mut contains = Vec::with_capacity(columns.len() * (1 + usize::from(prose == "abilities")));
        for locale in columns {
            contains.push(format!(
                "string::lowercase(tr.{locale}.description ?? '') CONTAINS $frag"
            ));
            if prose == "abilities" {
                contains.push(format!(
                    "string::lowercase(tr.{locale}.abilities ?? '') CONTAINS $frag"
                ));
            }
        }
        let sql = format!(
            "SELECT record::id(id) AS id, image{extra}, {} AS name,
                    (tr.en.name ?? '') AS en_name
               FROM {table}
              WHERE {}
              LIMIT {limit}",
            tr("name"),
            contains.join(" OR "),
        );
        let rows: Vec<CardRow> = db
            .query(sql)
            .bind(("lang", lang.to_string()))
            .bind(("frag", needle.clone()))
            .await?
            .take(0)?;
        for r in rows {
            let card = Card::from(r);
            if seen.insert(card.id) {
                out.push((section.to_string(), card));
            }
        }
    }
    Ok(out)
}

/// The needle's name-gram vocabulary: distinct three-*character* slices of
/// the lowercased needle (char boundaries, not bytes — Thai chars are 3
/// bytes each); a needle shorter than three characters is its own gram so
/// one- and two-character queries still find names containing them.
fn name_grams(needle: &str) -> Vec<String> {
    let chars: Vec<char> = needle.chars().collect();
    if chars.len() < 3 {
        return vec![needle.to_string()];
    }
    (0..=chars.len() - 3)
        .map(|i| chars[i..i + 3].iter().collect())
        .collect()
}

/// A user row, for the session layer.
#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password: String,
    pub is_admin: bool,
}

#[derive(Default, SurrealValue)]
#[surreal(default)]
struct UserRow {
    id: i64,
    username: String,
    password: String,
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
    let mut res = db
        .query(
            "LET $id = sequence::nextval('user_seq'); \
             CREATE type::record('user', $id) SET username = $u, password = $p, is_admin = false, created_at = time::unix(); \
             RETURN $id;",
        )
        .bind(("u", username.to_string()))
        .bind(("p", password_hash.to_string()))
        .await?;
    // a duplicate username trips the unique index; the form says taken.
    // take_errors() leaves the Ok results in place, so the id reads after.
    if let Some(e) = res.take_errors().into_values().next() {
        if e.to_string().contains("already contains") {
            return Ok(None);
        }
        return Err(e);
    }
    let id = res
        .take::<Option<i64>>(2)?
        .ok_or_else(|| surrealdb::Error::internal("user insert returned no id".to_string()))?;
    Ok(Some(User {
        id,
        username: username.to_string(),
        password: password_hash.to_string(),
        is_admin: false,
    }))
}

/// Hydrates an explicit id list, preserving the caller's order — which is the
/// ranking, so it has to survive the round trip.
pub async fn cards_by_ids(db: &Db, lang: &str, kind: &str, ids: &[i64]) -> Result<Vec<Card>> {
    let k = if ids.is_empty() {
        return Ok(Vec::new());
    } else {
        match Kind::of_section(kind) {
            Some(k) => k,
            None => return Ok(Vec::new()),
        }
    };
    let sql = format!(
        "SELECT record::id(id) AS id, image{}, {} AS name,
                (tr.en.name ?? '') AS en_name
           FROM {}
          WHERE record::id(id) IN $ids",
        if k.graded { ", grade" } else { "" },
        tr("name"),
        k.table,
    );
    let found: Vec<Card> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .bind(("ids", ids.to_vec()))
        .await?
        .take::<Vec<CardRow>>(0)?
        .into_iter()
        .map(Card::from)
        .collect();
    let mut cards_by_id = std::collections::HashMap::with_capacity(found.len());
    for card in found {
        cards_by_id.insert(card.id, card);
    }
    let mut ordered = Vec::with_capacity(ids.len());
    for id in ids {
        match cards_by_id.get(id) {
            Some(card) => ordered.push(card.clone()),
            None => {
                tracing::warn!(table = k.table, id, lang, "card lookup matched no row; the slot renders without it");
            }
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
    let own_col = if kind == "cookies" {
        "cookie_id"
    } else {
        "pet_id"
    };
    let partner_col = if kind == "cookies" {
        "pet_id"
    } else {
        "cookie_id"
    };
    let mut res = db
        .query(
            format!("SELECT VALUE DISTINCT {partner_col} FROM combi WHERE {own_col} = $id")
                .as_str(),
        )
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

impl grade::Graded for GachaEntry {
    fn grade(&self) -> Option<i64> {
        self.grade
    }
}

#[derive(Default, SurrealValue)]
#[surreal(default)]
struct GachaPoolRow {
    id: i64,
    tier: String,
    entries: Vec<GachaEntryRef>,
}

#[derive(Default, SurrealValue)]
#[surreal(default)]
struct GachaEntryRef {
    t: Option<i64>,
    p: Option<i64>,
    odds: f64,
}

/// One gacha prize row: the batched lookup result behind an entry.
#[derive(Default, SurrealValue)]
#[surreal(default)]
struct PrizeRow {
    id: i64,
    image: Option<String>,
    grade: Option<i64>,
    is_evolved: bool,
    name: String,
    en_name: String,
}

async fn prizes(
    db: &Db,
    lang: &str,
    table: &str,
    ids: &[i64],
) -> Result<std::collections::HashMap<i64, PrizeRow>> {
    let mut map = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    // the table lands in query text, so it is whitelisted here rather than
    // trusted from the caller
    let table = match table {
        "treasure" | "pet" => table,
        _ => return Ok(map),
    };
    let sql = format!(
        "SELECT record::id(id) AS id, image, grade, (is_evolved ?? false) AS is_evolved,
                {} AS name,
                (tr.en.name ?? '') AS en_name
           FROM {table}
          WHERE id IN $ids",
        tr("name"),
    );
    let list: Vec<String> = ids
        .iter()
        .map(|i| format!("{table}:{i}"))
        .collect();
    let rows: Vec<PrizeRow> = db
        .query(sql.as_str())
        .bind(("lang", lang.to_string()))
        .bind(("ids", list))
        .await?
        .take(0)?;
    for r in rows {
        map.insert(r.id, r);
    }
    Ok(map)
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
        out.push(GachaPool {
            id: p.id,
            tier: p.tier,
            entries,
        });
    }
    Ok(out)
}

/// Table names for one catalog section.
fn entity_table(section: Section) -> Option<&'static str> {
    // only the sections the admin editor writes
    section.editable().then_some(section.table())
}

/// Creates an entity and its translation in `lang`, returning the new id.
/// 0 when the section is not one the editor writes. The picker-list cache is
/// dropped here, inside the write: callers cannot forget it.
pub async fn insert_entity(
    db: &Db,
    lang: &str,
    section: Section,
    form: &crate::routes::admin::EntityForm,
) -> Result<i64> {
    let Some(table) = entity_table(section) else {
        return Ok(0);
    };
    let now = now_unix();
    let image = clean_image(&form.image);

    let mut sets: Vec<String> = vec!["image = $image".to_string()];
    if section.graded() {
        // display rank is maintained on write so reads can ORDER BY it:
        // the enum ordinal is not the display order (E outranks L)
        let g = form.grade.unwrap_or(1);
        sets.push(format!("rank = {}", grade::rank(g)));
        sets.push(format!("grade = {g}"));
        let ts = parse_release_date(&form.release_date).unwrap_or(now);
        sets.push(format!("release_date = {ts}"));
    }
    if section == Section::Treasures {
        sets.push(format!(
            "is_evolved = {}",
            form.is_evolved.as_deref() == Some("true")
        ));
        sets.push(format!(
            "is_power_plus = {}",
            form.is_power_plus.as_deref() == Some("true")
        ));
        if let Some(b) = form.base_treasure_id.filter(|v| *v > 0) {
            sets.push(format!("base_treasure_id = {b}"));
        }
        if let Some(c) = form.unlock_cookie_id.filter(|v| *v > 0) {
            sets.push(format!("unlock_cookie_id = {c}"));
        }
        if let Some(p) = form.unlock_pet_id.filter(|v| *v > 0) {
            sets.push(format!("unlock_pet_id = {p}"));
        }
    }
    // allocate and insert in one message: the id comes from the table's
    // sequence (seed.surql / ensure_sequences), the CREATE consumes it
    let mut res = db
        .query(format!(
            "LET $id = sequence::nextval('{table}_seq'); \
             CREATE type::record('{table}', $id) SET {}; \
             RETURN $id;",
            sets.join(", ")
        ))
        .bind(("image", image))
        .await?
        .check()?;
    let id = res
        .take::<Option<i64>>(2)?
        .ok_or_else(|| surrealdb::Error::internal(format!("{table} insert returned no id")))?;
    write_translation(db, lang, section, table, id, form).await?;
    crate::options::invalidate();
    Ok(id)
}

/// Updates an entity and its translation in `lang`. The picker-list cache is
/// dropped here, inside the write: callers cannot forget it.
pub async fn update_entity(
    db: &Db,
    lang: &str,
    section: Section,
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
    // an empty date field means "no change"; the create path defaults to now
    if let Some(ts) = parse_release_date(&form.release_date) {
        sets.push(format!("release_date = {ts}"));
    }
    if section == Section::Treasures {
        if let Some(v) = form.is_evolved.as_deref() {
            sets.push(format!("is_evolved = {}", v == "true"));
        }
        if let Some(v) = form.is_power_plus.as_deref() {
            sets.push(format!("is_power_plus = {}", v == "true"));
        }
        // 0 / absent clears the link, matching the "None" option the form
        // renders for every select
        let clear_or_set = |name: &str, v: Option<i64>, sets: &mut Vec<String>| {
            let v = v.unwrap_or(0);
            if v > 0 {
                sets.push(format!("{name} = {v}"));
            } else {
                sets.push(format!("{name} = NONE"));
            }
        };
        clear_or_set("base_treasure_id", form.base_treasure_id, &mut sets);
        clear_or_set("unlock_cookie_id", form.unlock_cookie_id, &mut sets);
        clear_or_set("unlock_pet_id", form.unlock_pet_id, &mut sets);
    }
    if image.is_some() {
        sets.push("image = $image".to_string());
    }
    if !sets.is_empty() {
        db.query(format!("UPDATE {table}:{id} SET {}", sets.join(", ")).as_str())
            .bind(("image", image))
            .await?
            .check()?;
    }
    write_translation(db, lang, section, table, id, form).await?;
    crate::options::invalidate();
    Ok(true)
}

fn clean_image(image: &str) -> Option<String> {
    let trimmed = image.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// YYYY-MM-DD -> unix seconds, like the old `parse_release_date`. `None`
/// when the field is empty (update keeps the stored value) or malformed.
fn parse_release_date(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // a date input always sends YYYY-MM-DD; reject anything else
    let mut parts = raw.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d) = (y.parse::<i64>().ok()?, m.parse::<i64>().ok()?, d.parse::<i64>().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days-from-civil (Howard Hinnant's algorithm), then to unix seconds
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400)
}

/// Upserts the translation for one language INSIDE the nested tr object, so
/// editing in Thai cannot wipe the English text and the other way round.
async fn write_translation(
    db: &Db,
    lang: &str,
    section: Section,
    table: &str,
    id: i64,
    form: &crate::routes::admin::EntityForm,
) -> Result<()> {
    // merge client-side: fetch existing tr.$lang, overlay the submitted
    // fields, write the whole object back — deterministic regardless of how
    // many languages exist
    #[derive(Default, SurrealValue, serde::Serialize)]
    #[surreal(default)]
    struct Tr {
        name: String,
        abilities: String,
        description: String,
        power_plus: String,
        power_plus_requirement: String,
        unlock_goal: String,
    }
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct TrWrap {
        tr: std::collections::BTreeMap<String, Tr>,
    }
    let mut wrap: TrWrap = db
        .query("SELECT tr FROM type::record($tb, $id)")
        .bind(("tb", table.to_string()))
        .bind(("id", id))
        .await?
        .take::<Vec<TrWrap>>(0)?
        .pop()
        .unwrap_or_default();
    let entry = wrap.tr.entry(lang.to_string()).or_default();
    entry.name = form.name.trim().to_string();
    match section {
        Section::Cookies => {
            entry.abilities.clone_from(&form.abilities);
            entry.description.clone_from(&form.description);
            entry.power_plus.clone_from(&form.power_plus);
            entry
                .power_plus_requirement
                .clone_from(&form.power_plus_requirement);
            entry.unlock_goal.clone_from(&form.unlock_goal);
        }
        Section::Pets => {
            entry.abilities.clone_from(&form.abilities);
            entry.description.clone_from(&form.description);
        }
        _ => entry.description.clone_from(&form.description),
    }
    db.query("UPDATE type::record($tb, $id) SET tr = $tr")
        .bind(("tb", table.to_string()))
        .bind(("id", id))
        .bind(("tr", wrap.tr))
        .await?
        .check()?;
    Ok(())
}

/// The cookie or pet that unlocks a treasure at max level.
#[derive(Debug, Clone, Default)]
pub struct TreasureLinks {
    pub unlock_section: &'static str,
    pub unlock_id: i64,
    pub unlock_name: String,
    pub unlock_image: Option<String>,
}

impl TreasureLinks {
    pub const fn has_unlock(&self) -> bool {
        self.unlock_id > 0 && !self.unlock_name.is_empty()
    }
}

/// The unlock link columns a treasure carries.
#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct TreasureLinkRow {
    #[surreal(rename = "unlock_cookie_id")]
    unlock_cookie: Option<i64>,
    #[surreal(rename = "unlock_pet_id")]
    unlock_pet: Option<i64>,
}

pub async fn treasure_links(db: &Db, lang: &str, id: i64) -> Result<TreasureLinks> {
    let mut rows: Vec<TreasureLinkRow> = db
        .query("SELECT unlock_cookie_id, unlock_pet_id FROM type::record(\"treasure\", $id)")
        .bind(("id", id))
        .await?
        .take(0)?;
    let Some(row) = rows.pop() else {
        return Ok(TreasureLinks::default());
    };

    let mut out = TreasureLinks::default();
    if let Some(cid) = row.unlock_cookie.filter(|v| *v > 0) {
        if let Some((name, image)) = entity_link(db, lang, "cookie", cid).await {
            out.unlock_section = "cookies";
            out.unlock_id = cid;
            out.unlock_name = name;
            out.unlock_image = image;
        }
    } else if let Some(pid) = row.unlock_pet.filter(|v| *v > 0) {
        if let Some((name, image)) = entity_link(db, lang, "pet", pid).await {
            out.unlock_section = "pets";
            out.unlock_id = pid;
            out.unlock_name = name;
            out.unlock_image = image;
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
    /// the cookie/pet ids the pairing hangs between: the delete route
    /// verifies the row actually belongs to this pair before removing it
    pub cookie_id: i64,
    pub pet_id: i64,
    pub partner_id: i64,
    pub partner_name: String,
    pub partner_image: Option<String>,
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
            cookie_id: record.cookie_id,
            pet_id: record.pet_id,
            partner_id: row.partner_id,
            partner_name: row.partner_name,
            partner_image: row.partner_image,
            effect: row.effect,
            is_hidden: row.is_hidden,
        })
        .collect())
}

/// Removes one combo pairing by record id, only when it hangs off the given
/// cookie: the route passes the id from the editor's path, so a guessed row
/// id cannot delete a pairing another editor displays.
pub async fn delete_combi(db: &Db, row_id: i64, cookie_id: i64) -> Result<bool> {
    let mut res = db
        .query("DELETE type::record(\"combi\", $id) WHERE cookie_id = $cookie RETURN AFTER")
        .bind(("id", row_id))
        .bind(("cookie", cookie_id))
        .await?
        .check()?;
    let removed: Vec<CombiRecord> = res.take(0)?;
    Ok(!removed.is_empty())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use surrealdb::types::{Object, Value};

    /// The regression a lint pass once hid: a row struct whose field names
    /// drift from the projected columns decodes every row as `None` — no
    /// error, no missing-field complaint, just silently absent data. These
    /// tests build the exact objects the queries project and decode them the
    /// way `take` does, with no database behind them.
    #[test]
    fn treasure_link_row_reads_the_projected_columns() {
        let mut o = Object::new();
        o.insert("unlock_cookie_id", 7i64);
        // unlock_pet_id absent, as for a treasure a cookie unlocks
        let row = TreasureLinkRow::from_value(Value::Object(o)).expect("decodes");
        assert_eq!(row.unlock_cookie, Some(7));
        assert_eq!(row.unlock_pet, None);
    }

    #[test]
    fn card_row_reads_the_card_projection() {
        let mut o = Object::new();
        o.insert("id", 12i64);
        o.insert("name", "Choco");
        o.insert("en_name", "Choco");
        o.insert("grade", 4i64);
        o.insert("is_evolved", false);
        let row = CardRow::from_value(Value::Object(o)).expect("decodes");
        assert_eq!(row.id, 12);
        assert_eq!(row.grade, Some(4));
        assert!(row.image.is_none());
        assert!(!row.is_evolved);
    }

    #[test]
    fn detail_row_defaults_the_fields_a_kind_lacks() {
        let mut o = Object::new();
        o.insert("id", 3i64);
        o.insert("name", "Stone");
        o.insert("en_name", "Stone");
        let row = DetailRow::from_value(Value::Object(o)).expect("decodes");
        assert_eq!(row.abilities, "");
        assert_eq!(row.grade, None);
        assert_eq!(row.release_date, None);
    }

    #[test]
    fn effect_line_row_carries_the_ladder_and_locale() {
        let mut o = Object::new();
        o.insert("state", 1i64);
        o.insert("en", "Speed +5");
        o.insert("th", "thai speed");
        o.insert("values", vec!["1".to_string(), "5".to_string()]);
        let row = EffectLineRow::from_value(Value::Object(o)).expect("decodes");
        let line = row.into_line("th");
        assert!(line.blessed);
        assert_eq!(line.text, "thai speed");
        assert_eq!(line.values, vec!["1".to_string(), "5".to_string()]);

        let mut o2 = Object::new();
        o2.insert("state", 0i64);
        o2.insert("en", "Speed +5");
        o2.insert("th", "");
        let row2 = EffectLineRow::from_value(Value::Object(o2)).expect("decodes");
        assert_eq!(row2.into_line("th").text, "Speed +5");
    }

    #[test]
    fn combi_record_reads_its_columns() {
        let mut o = Object::new();
        o.insert("id", 5i64);
        o.insert("cookie_id", 89i64);
        o.insert("pet_id", 3i64);
        o.insert("en", "Combo");
        o.insert("th", "");
        o.insert("is_hidden", false);
        let row = CombiRecord::from_value(Value::Object(o)).expect("decodes");
        assert_eq!(row.cookie_id, 89);
        assert_eq!(row.pet_id, 3);
        assert!(!row.is_hidden);
    }

    #[test]
    fn user_row_reads_credentials_and_flag() {
        let mut o = Object::new();
        o.insert("id", 1i64);
        o.insert("username", "admin");
        o.insert("password", "$argon2id$v=19$fake");
        o.insert("is_admin", true);
        let row = UserRow::from_value(Value::Object(o)).expect("decodes");
        assert!(row.is_admin);
        assert_eq!(row.username, "admin");
    }

    #[test]
    fn prize_row_reads_the_batched_lookup_projection() {
        let mut o = Object::new();
        o.insert("id", 9i64);
        o.insert("image", "ring.png");
        o.insert("grade", 2i64);
        o.insert("is_evolved", false);
        o.insert("name", "Ring");
        o.insert("en_name", "Ring");
        let row = PrizeRow::from_value(Value::Object(o)).expect("decodes");
        assert_eq!(row.image.as_deref(), Some("ring.png"));
        assert_eq!(row.grade, Some(2));
    }

    #[test]
    fn gacha_rows_read_their_columns() {
        let mut o = Object::new();
        o.insert("id", 2i64);
        o.insert("tier", "epic");
        o.insert("entries", Vec::<i64>::new());
        let pool = GachaPoolRow::from_value(Value::Object(o)).expect("decodes");
        assert_eq!(pool.tier, "epic");
        assert!(pool.entries.is_empty());

        let mut e = Object::new();
        e.insert("t", 0i64);
        e.insert("p", 4i64);
        e.insert("odds", 0.42f64);
        let entry = GachaEntryRef::from_value(Value::Object(e)).expect("decodes");
        assert_eq!(entry.p, Some(4));
        assert!((entry.odds - 0.42).abs() < f64::EPSILON);
    }

    #[test]
    fn kind_facts_come_from_one_table() {
        // the triplication the lint pass left behind: `Kind::of` and the old
        // hand-written fallbacks must agree, or a fallback queries a wrong
        // table in silence
        let kind = Kind::of(Section::Cookies);
        assert_eq!(kind.table, "cookie");
        assert!(kind.graded);
        assert!(kind.dated);
        assert_eq!(Kind::of_section("cookies").map(|k| k.table), Some("cookie"));
        assert!(Kind::of_section("no-such-section").is_none());
        assert!(Section::parse("skins").is_some());
        assert_eq!(Section::Skins.table(), "skin");
        assert!(Section::Ingredients.graded());
        assert!(!Section::Ingredients.dated());
    }
}
