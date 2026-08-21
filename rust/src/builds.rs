//! Community builds: the stored loadout plus everything the cards show.
//! Ported from the build half of database/select.v.

use rusqlite::params;

use crate::db::{cards_by_ids, Card, Db};

/// One build as the list and detail pages render it.
#[derive(Debug, Clone, Default)]
pub struct BuildCard {
    pub id: i64,
    pub ep: i64,
    pub ep_special: i64,
    pub tags: Vec<String>,
    pub boosts: Vec<String>,
    pub boost: String,
    pub score: i64,
    pub coin: i64,
    pub time_ms: i64,
    pub boxes: i64,
    pub description: String,
    pub youtube_url: String,
    pub author: String,
    /// the owner, which the edit/delete gate will compare against the session
    #[allow(dead_code)]
    pub user_id: i64,
    pub is_anon: bool,
    /// hours an anonymous build has left; 0 for a permanent one
    pub expires_in_h: i64,
    pub cookie: Option<Card>,
    pub cookie2: Option<Card>,
    pub pet: Option<Card>,
    pub treasures: Vec<Card>,
    pub treasure_levels: Vec<i64>,
    pub treasure_blessed: Vec<bool>,
}

impl BuildCard {
    /// "EP 5" or "Special EP 2", as the badge reads.
    pub fn ep_label(&self) -> String {
        if self.ep_special > 0 {
            format!("Special EP {}", self.ep_special)
        } else if self.ep > 0 {
            format!("EP {}", self.ep)
        } else {
            String::new()
        }
    }

    /// The run duration as m:ss, from the stored milliseconds.
    pub fn time_label(&self) -> String {
        if self.time_ms <= 0 {
            return String::new();
        }
        let secs = self.time_ms / 1000;
        format!("{}:{:02}", secs / 60, secs % 60)
    }

    /// The five loadout slots in display order, each already knowing which
    /// image directory it came from — the templates iterate one list rather
    /// than repeating the same block per slot.
    pub fn slots(&self) -> Vec<Slot> {
        let mut out = Vec::with_capacity(5);
        for (card, section) in [
            (self.cookie.as_ref(), "cookies"),
            (self.cookie2.as_ref(), "cookies"),
            (self.pet.as_ref(), "pets"),
        ] {
            if let Some(c) = card {
                out.push(Slot {
                    section,
                    name: c.name.clone(),
                    image: c.image.clone(),
                    level: -1,
                    blessed: false,
                });
            }
        }
        for (i, t) in self.treasures.iter().enumerate() {
            out.push(Slot {
                section: "treasures",
                name: t.name.clone(),
                image: t.image.clone(),
                level: self.treasure_levels.get(i).copied().unwrap_or(-1),
                blessed: self.treasure_blessed.get(i).copied().unwrap_or(false),
            });
        }
        out
    }

    pub fn has_stats(&self) -> bool {
        self.score > 0 || self.coin > 0 || self.time_ms > 0 || self.boxes > 0
    }
}

/// One filled loadout slot, flattened for the templates.
#[derive(Debug, Clone)]
pub struct Slot {
    pub section: &'static str,
    pub name: String,
    pub image: Option<String>,
    /// equipped level 0-9, or -1 for a slot that has none (cookie, pet)
    pub level: i64,
    pub blessed: bool,
}

fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The list, filtered and sorted the way the V query does. Expired anonymous
/// builds are excluded by the query rather than swept, so nothing has to run
/// on a timer.
pub fn select_builds(
    db: &Db,
    lang: &str,
    filter: (i64, i64, i64, i64, i64),
    sort: &str,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<BuildCard>> {
    let (f_cookie, f_pet, f_treasure, f_ep, f_ep_special) = filter;
    let mut where_sql = String::from("1=1");
    if f_cookie > 0 {
        where_sql.push_str(&format!(" AND cookie_id = {f_cookie}"));
    }
    if f_pet > 0 {
        where_sql.push_str(&format!(" AND pet_id = {f_pet}"));
    }
    if f_treasure > 0 {
        where_sql.push_str(&format!(
            " AND (treasure1_id = {f_treasure} OR treasure2_id = {f_treasure} OR treasure3_id = {f_treasure})"
        ));
    }
    if f_ep > 0 {
        where_sql.push_str(&format!(" AND ep = {f_ep} AND ep_special = 0"));
    }
    if f_ep_special > 0 {
        where_sql.push_str(&format!(" AND ep_special = {f_ep_special}"));
    }
    select_where(db, lang, &where_sql, sort, limit, offset)
}

/// The shared body: a WHERE clause the callers assemble, then the batched
/// entity lookups. `select_build` reuses it so the expiry rule lives once.
fn select_where(
    db: &Db,
    lang: &str,
    filter_sql: &str,
    sort: &str,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<BuildCard>> {
    let now = now_unix();
    let where_sql = format!("({filter_sql}) AND (expires_at IS NULL OR expires_at > {now})");

    // the id is the tie-break on every sort, so paging cannot repeat a row
    let order = match sort {
        "score" => "score DESC, build_id DESC",
        "coin" => "coin DESC, build_id DESC",
        "time" => "time ASC, build_id DESC",
        _ => "created_at DESC, build_id DESC",
    };

    let raw: Vec<(BuildCard, i64, Option<i64>, i64, [i64; 3])> = {
        let c = db
            .get()
            .map_err(|e| rusqlite::Error::InvalidParameterName(format!("pool: {e}")))?;
        let sql = format!(
            "SELECT build_id, cookie_id, cookie2_id, pet_id, treasure1_id, treasure2_id,
                    treasure3_id, treasure1_blessed, treasure2_blessed, treasure3_blessed,
                    treasure1_level, treasure2_level, treasure3_level, ep, ep_special, tag,
                    boosts, boost, score, coin, time, boxes, description, youtube_url,
                    author, user_id, expires_at
               FROM build WHERE {where_sql} ORDER BY {order} LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt.query_map(params![limit, offset], |r| {
            let expires_at: Option<i64> = r.get("expires_at")?;
            let card = BuildCard {
                id: r.get("build_id")?,
                ep: r.get("ep")?,
                ep_special: r.get("ep_special")?,
                tags: split_list(&r.get::<_, Option<String>>("tag")?.unwrap_or_default()),
                boosts: split_list(&r.get::<_, Option<String>>("boosts")?.unwrap_or_default()),
                boost: r.get::<_, Option<String>>("boost")?.unwrap_or_default(),
                score: r.get("score")?,
                coin: r.get("coin")?,
                time_ms: r.get("time")?,
                boxes: r.get("boxes")?,
                description: r.get::<_, Option<String>>("description")?.unwrap_or_default(),
                youtube_url: r.get::<_, Option<String>>("youtube_url")?.unwrap_or_default(),
                author: r.get::<_, Option<String>>("author")?.unwrap_or_default(),
                user_id: r.get("user_id")?,
                is_anon: expires_at.is_some(),
                expires_in_h: expires_at.map(|e| ((e - now) / 3600).max(0)).unwrap_or(0),
                treasure_levels: vec![
                    r.get("treasure1_level")?,
                    r.get("treasure2_level")?,
                    r.get("treasure3_level")?,
                ],
                treasure_blessed: vec![
                    r.get::<_, i64>("treasure1_blessed")? != 0,
                    r.get::<_, i64>("treasure2_blessed")? != 0,
                    r.get::<_, i64>("treasure3_blessed")? != 0,
                ],
                ..Default::default()
            };
            Ok((
                card,
                r.get("cookie_id")?,
                r.get("cookie2_id")?,
                r.get("pet_id")?,
                [
                    r.get("treasure1_id")?,
                    r.get("treasure2_id")?,
                    r.get("treasure3_id")?,
                ],
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    // the entities come back in three batched queries rather than one per
    // slot per build, which is what the V lookups do
    let mut cookie_ids: Vec<i64> = Vec::new();
    let mut pet_ids: Vec<i64> = Vec::new();
    let mut treasure_ids: Vec<i64> = Vec::new();
    for (_, cookie, cookie2, pet, treasures) in &raw {
        push_id(&mut cookie_ids, *cookie);
        if let Some(c2) = cookie2 {
            push_id(&mut cookie_ids, *c2);
        }
        push_id(&mut pet_ids, *pet);
        for t in treasures {
            push_id(&mut treasure_ids, *t);
        }
    }
    let cookies = cards_by_ids(db, lang, "cookies", &cookie_ids)?;
    let pets = cards_by_ids(db, lang, "pets", &pet_ids)?;
    let treasures = cards_by_ids(db, lang, "treasures", &treasure_ids)?;

    let find = |list: &[Card], id: i64| list.iter().find(|c| c.id == id).cloned();
    Ok(raw
        .into_iter()
        .map(|(mut card, cookie, cookie2, pet, tids)| {
            card.cookie = find(&cookies, cookie);
            card.cookie2 = cookie2.and_then(|id| find(&cookies, id));
            card.pet = find(&pets, pet);
            card.treasures = tids.iter().filter_map(|id| find(&treasures, *id)).collect();
            card
        })
        .collect())
}

fn push_id(list: &mut Vec<i64>, id: i64) {
    if id > 0 && !list.contains(&id) {
        list.push(id);
    }
}

/// One build by id, through the same query so the expiry rule cannot drift:
/// an expired anonymous build is simply not found.
pub fn select_build(db: &Db, lang: &str, id: i64) -> rusqlite::Result<Option<BuildCard>> {
    let found = select_where(db, lang, &format!("build_id = {id}"), "latest", 1, 0)?;
    Ok(found.into_iter().next())
}

/// A build about to be written. Separate from BuildCard because that one
/// carries resolved entities for rendering, while this carries the ids the
/// row actually stores.
#[derive(Debug, Clone)]
pub struct NewBuild {
    pub cookie: i64,
    pub cookie2: Option<i64>,
    pub pet: i64,
    pub treasures: [i64; 3],
    pub blessed: [bool; 3],
    pub levels: [i64; 3],
    pub ep: i64,
    pub ep_special: i64,
    pub tags: String,
    pub score: i64,
    pub coin: i64,
    pub time_ms: i64,
    pub boxes: i64,
    pub description: String,
    pub youtube_url: String,
    pub author: String,
    pub user_id: i64,
    /// unix seconds for an anonymous build, None for a permanent one
    pub expires_at: Option<i64>,
}

/// Inserts a build and returns its id, or 0 when the write failed.
pub fn insert_build(db: &Db, b: &NewBuild) -> rusqlite::Result<i64> {
    let c = db
        .get()
        .map_err(|e| rusqlite::Error::InvalidParameterName(format!("pool: {e}")))?;
    c.execute(
        "INSERT INTO build (cookie_id, cookie2_id, pet_id, combi_bonus_id,
                            treasure1_id, treasure2_id, treasure3_id,
                            treasure1_blessed, treasure2_blessed, treasure3_blessed,
                            treasure1_level, treasure2_level, treasure3_level,
                            ep, ep_special, tag, boosts, boost, power_effects,
                            score, coin, time, boxes, description, youtube_url,
                            author, user_id, created_at, expires_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 ?13, ?14, ?15, '', '', '', ?16, ?17, ?18, ?19, ?20, ?21,
                 ?22, ?23, ?24, ?25)",
        params![
            b.cookie,
            b.cookie2,
            b.pet,
            b.treasures[0],
            b.treasures[1],
            b.treasures[2],
            b.blessed[0] as i64,
            b.blessed[1] as i64,
            b.blessed[2] as i64,
            b.levels[0],
            b.levels[1],
            b.levels[2],
            b.ep,
            b.ep_special,
            b.tags,
            b.score,
            b.coin,
            b.time_ms,
            b.boxes,
            b.description,
            b.youtube_url,
            b.author,
            b.user_id,
            now_unix(),
            b.expires_at,
        ],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn delete_build(db: &Db, id: i64) -> rusqlite::Result<()> {
    let c = db
        .get()
        .map_err(|e| rusqlite::Error::InvalidParameterName(format!("pool: {e}")))?;
    c.execute("DELETE FROM build_review WHERE build_id = ?1", params![id])?;
    c.execute("DELETE FROM build WHERE build_id = ?1", params![id])?;
    Ok(())
}

/// Updates a build in place. Author, owner and expiry are untouched: an edit
/// must not turn an anonymous build permanent or change who owns it.
pub fn update_build(db: &Db, id: i64, b: &NewBuild) -> rusqlite::Result<()> {
    let c = db
        .get()
        .map_err(|e| rusqlite::Error::InvalidParameterName(format!("pool: {e}")))?;
    c.execute(
        "UPDATE build SET cookie_id = ?1, cookie2_id = ?2, pet_id = ?3,
                treasure1_id = ?4, treasure2_id = ?5, treasure3_id = ?6,
                treasure1_blessed = ?7, treasure2_blessed = ?8, treasure3_blessed = ?9,
                treasure1_level = ?10, treasure2_level = ?11, treasure3_level = ?12,
                ep = ?13, ep_special = ?14, tag = ?15, score = ?16, coin = ?17,
                time = ?18, boxes = ?19, description = ?20, youtube_url = ?21
          WHERE build_id = ?22",
        params![
            b.cookie,
            b.cookie2,
            b.pet,
            b.treasures[0],
            b.treasures[1],
            b.treasures[2],
            b.blessed[0] as i64,
            b.blessed[1] as i64,
            b.blessed[2] as i64,
            b.levels[0],
            b.levels[1],
            b.levels[2],
            b.ep,
            b.ep_special,
            b.tags,
            b.score,
            b.coin,
            b.time_ms,
            b.boxes,
            b.description,
            b.youtube_url,
            id,
        ],
    )?;
    Ok(())
}
