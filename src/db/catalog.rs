//! The catalog reads: one card projection for every grid, the simple
//! one-shot catalogs, the episode browser list, and the batched id
//! hydration the build pages ask for.

use surrealdb::types::SurrealValue;

use crate::section::Section;

use super::types::{tr, Card, CardRow, Kind};
use super::{Db, Result};

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
pub(super) struct EpisodeListRow {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) en_name: String,
    pub(super) image: Option<String>,
    pub(super) kind: String,
    pub(super) stars: i64,
    pub(super) league_ranked: bool,
    pub(super) entry_cost: String,
    pub(super) stage_count: i64,
    pub(super) quest_count: i64,
    pub(super) relic_count: i64,
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
/// closure scan is linear per row. Two scans + a `HashMap` join stay in
/// single digits.
pub async fn select_ingredient_list(db: &Db, lang: &str) -> Result<Vec<IngredientList>> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct CountRow {
        ingredient_id: i64,
        c: i64,
    }
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
                ({subtitle} ?? subtitle ?? '') AS subtitle,
                {name} AS name, (tr.en.name ?? '') AS en_name,
                cookie_id, pet_id,
                (array::find($cn, |$x| $x.cid = cookie_id).name ?? '') AS cookie_name,
                (array::find($pn, |$x| $x.pid = pet_id).name ?? '') AS pet_name
           FROM skin
          ORDER BY id",
        name = tr("name"),
        cname = tr("name"),
        pname = tr("name"),
        // the localized owner attribution lives in skin_translation.description;
        // the bare `subtitle` column is English-only
        subtitle = tr("description")
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
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct EpisodeName {
        id: i64,
        name: String,
    }
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
                tracing::warn!(
                    table = k.table,
                    id,
                    lang,
                    "card lookup matched no row; the slot renders without it"
                );
            }
        }
    }
    Ok(ordered)
}

/// Every (section, id) pair the sitemap lists — every section from the one
/// list, so a section can never go missing the way relics and skins once
/// did when the list was hand-maintained. All eight tables' statements ride
/// one message, and the result is cached against `stamp` rather than for a few
/// minutes: the sitemap serves crawlers, the catalog changes only through
/// writes, and every write moves the stamp (`db::stamp`) — so the cache is
/// invalidated by the very thing the document dates itself by. A time window
/// cannot make that promise: an entity created inside one left both the
/// document and the cache on the previous list, and a crawler that sent the
/// tag it had just been given was answered `304` for a sitemap that had
/// changed. That is a lie a window cannot avoid and a stamp cannot tell.
///
/// `stamp` is read by the caller *before* this query for that reason: reading
/// it afterwards would let a list from before a write be cached under the
/// stamp from after it, which is the one order that freezes a change out of
/// the cache for good.
pub async fn sitemap_entries(db: &Db, stamp: Option<i64>) -> Result<Vec<(&'static str, i64)>> {
    type Entries = std::sync::Arc<Vec<(&'static str, i64)>>;
    /// The list as it stood under one stamp.
    struct Cached {
        stamp: Option<i64>,
        entries: Entries,
    }
    static CACHE: std::sync::OnceLock<parking_lot::Mutex<Option<Cached>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| parking_lot::Mutex::new(None));
    if let Some(cached) = cache.lock().as_ref() {
        if cached.stamp == stamp {
            return Ok((*cached.entries).clone());
        }
    }
    let mut sql = String::new();
    for s in Section::ALL {
        sql.push_str("SELECT VALUE record::id(id) FROM ");
        sql.push_str(s.table());
        sql.push(';');
    }
    sql.push_str("SELECT VALUE record::id(id) FROM news WHERE published = true AND published_at <= time::unix();");
    let mut res = db.query(&sql).await?;
    let mut out: Vec<(&'static str, i64)> = Vec::new();
    for (i, section) in Section::ALL.iter().enumerate() {
        let ids: Vec<i64> = res.take(i)?;
        out.extend(ids.into_iter().map(|id| (section.as_str(), id)));
    }
    let news_ids: Vec<i64> = res.take(Section::ALL.len())?;
    out.extend(news_ids.into_iter().map(|id| ("news", id)));
    *cache.lock() = Some(Cached {
        stamp,
        entries: Entries::new(out.clone()),
    });
    Ok(out)
}
