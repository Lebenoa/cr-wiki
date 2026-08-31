//! Community builds: the stored loadout plus everything the cards show.
//! Ported from the build half of database/select.v; queries now speak
//! SurrealQL against the external SurrealDB server.

use surrealdb::types::SurrealValue;

use crate::db::{cards_by_ids, Card, Db};

/// One build as the list and detail pages render it.
#[derive(Debug, Clone, Default)]
pub struct BuildCard {
    pub id: i64,
    pub cookie: Option<Card>,
    pub cookie2: Option<Card>,
    pub pet: Option<Card>,
    pub treasures: Vec<Card>,
    pub treasure_levels: Vec<i64>,
    pub treasure_blessed: Vec<bool>,
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
    /// stamped onto the record on insert; edits never touch it
    pub author: String,
    pub user_id: i64,
    /// anonymous builds expire; signed-in ones do not
    pub is_anon: bool,
    pub expires_in_h: i64,
}

impl BuildCard {
    /// "EP 5" or "Special EP 2", as the badge reads. Localized, so it takes
    /// the request context rather than baking English in.
    pub fn ep_label(&self, ctx: &crate::ctx::Ctx) -> String {
        if self.ep_special <= 0 && self.ep <= 0 {
            return String::new();
        }
        ctx.build_ep_label(self.ep, self.ep_special)
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

/// The raw build record as stored; entity slots are resolved afterwards in
/// batched lookups rather than one query per slot per build.
#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
struct BuildRow {
    id: i64,
    cookie_id: i64,
    cookie2_id: Option<i64>,
    pet_id: i64,
    treasure1_id: i64,
    treasure2_id: i64,
    treasure3_id: i64,
    treasure1_blessed: bool,
    treasure2_blessed: bool,
    treasure3_blessed: bool,
    treasure1_level: i64,
    treasure2_level: i64,
    treasure3_level: i64,
    ep: i64,
    ep_special: i64,
    tag: String,
    boosts: String,
    boost: String,
    score: i64,
    coin: i64,
    time: i64,
    boxes: i64,
    description: String,
    youtube_url: String,
    author: String,
    user_id: i64,
    expires_at: Option<i64>,
}

impl BuildRow {
    fn into_card(self, now: i64) -> BuildCard {
        BuildCard {
            id: self.id,
            ep: self.ep,
            ep_special: self.ep_special,
            tags: split_list(&self.tag),
            boosts: split_list(&self.boosts),
            boost: self.boost,
            score: self.score,
            coin: self.coin,
            time_ms: self.time,
            boxes: self.boxes,
            description: self.description,
            youtube_url: self.youtube_url,
            author: self.author,
            user_id: self.user_id,
            is_anon: self.expires_at.is_some(),
            expires_in_h: self
                .expires_at
                .map(|e| ((e - now) / 3600).max(0))
                .unwrap_or(0),
            treasure_levels: vec![
                self.treasure1_level,
                self.treasure2_level,
                self.treasure3_level,
            ],
            treasure_blessed: vec![
                self.treasure1_blessed,
                self.treasure2_blessed,
                self.treasure3_blessed,
            ],
            ..Default::default()
        }
    }
}

/// The list, filtered and sorted the way the V query does. Expired anonymous
/// builds are excluded by the query rather than swept, so nothing has to run
/// on a timer. Filters are integers parsed upstream, so they inline safely;
/// only free-text values ever travel as bound parameters.
pub async fn select_builds(
    db: &Db,
    lang: &str,
    filter: (i64, i64, i64, i64, i64),
    sort: &str,
    author: &str,
    limit: i64,
    offset: i64,
) -> crate::db::Result<Vec<BuildCard>> {
    let (f_cookie, f_pet, f_treasure, f_ep, f_ep_special) = filter;
    let mut where_sql = String::new();
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
    select_where(db, lang, &where_sql, sort, author, limit, offset).await
}

/// The shared body: a WHERE fragment the callers assemble, then the batched
/// entity lookups. `select_build` reuses it so the expiry rule lives once.
///
/// The expiry WHERE carries `expires_at = 0` because a permanent build stores
/// an explicit 0 (the insert binds `unwrap_or(0)`), and `IS NONE` never matches
/// a stored zero — the plain `> $now` test alone would hide every signed-in
/// author's builds.
async fn select_where(
    db: &Db,
    lang: &str,
    filter_sql: &str,
    sort: &str,
    author: &str,
    limit: i64,
    offset: i64,
) -> crate::db::Result<Vec<BuildCard>> {
    let now = now_unix();

    // the id is the tie-break on every sort, so paging cannot repeat a row.
    // The verified count rides the projection as an alias because SurrealDB v3
    // refuses an ORDER BY term that the projection does not name.
    let (order, ok_select) = match sort {
        "score" => ("score DESC, id DESC", ""),
        "coin" => ("coin DESC, id DESC", ""),
        "time" => ("time ASC, id DESC", ""),
        "verified" => (
            "verified_ok DESC, id DESC",
            ",
                array::len((SELECT id FROM review
                             WHERE build_id = record::id($parent.id)
                               AND verified = true)) AS verified_ok",
        ),
        _ => ("created_at DESC, id DESC", ""),
    };

    // free text filters travel as bound parameters, never inline
    let author_filter = if author.is_empty() { "" } else { " AND author = $author" };

    let sql = format!(
        "SELECT record::id(id) AS id, cookie_id, cookie2_id, pet_id,
                treasure1_id, treasure2_id, treasure3_id,
                treasure1_blessed, treasure2_blessed, treasure3_blessed,
                treasure1_level, treasure2_level, treasure3_level,
                ep, ep_special, tag, boosts, boost, score, coin, time, boxes,
                description, youtube_url, author, user_id, expires_at, created_at{ok_select}
           FROM build
          WHERE (expires_at IS NONE OR expires_at = 0 OR expires_at > $now){author_filter}{filter_sql}
          ORDER BY {order}
          LIMIT {limit} START {offset}"
    );
    let mut q = db.query(&sql).bind(("now", now));
    if !author.is_empty() {
        q = q.bind(("author", author.to_string()));
    }
    let rows: Vec<BuildRow> = q.await?.take(0)?;

    // the entities come back in three batched queries rather than one per
    // slot per build, which is what the V lookups do
    let mut cookie_ids: Vec<i64> = Vec::new();
    let mut pet_ids: Vec<i64> = Vec::new();
    let mut treasure_ids: Vec<i64> = Vec::new();
    for b in &rows {
        push_id(&mut cookie_ids, b.cookie_id);
        if let Some(c2) = b.cookie2_id {
            push_id(&mut cookie_ids, c2);
        }
        push_id(&mut pet_ids, b.pet_id);
        for t in [b.treasure1_id, b.treasure2_id, b.treasure3_id] {
            push_id(&mut treasure_ids, t);
        }
    }
    let cookies = cards_by_ids(db, lang, "cookies", &cookie_ids).await?;
    let pets = cards_by_ids(db, lang, "pets", &pet_ids).await?;
    let treasures = cards_by_ids(db, lang, "treasures", &treasure_ids).await?;

    let find = |list: &[Card], id: i64| list.iter().find(|c| c.id == id).cloned();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut card = row.clone().into_card(now);
        card.cookie = find(&cookies, row.cookie_id);
        card.cookie2 = row.cookie2_id.and_then(|id| find(&cookies, id));
        card.pet = find(&pets, row.pet_id);
        card.treasures = [row.treasure1_id, row.treasure2_id, row.treasure3_id]
            .iter()
            .filter_map(|id| find(&treasures, *id))
            .collect();
        out.push(card);
    }
    Ok(out)
}

fn push_id(list: &mut Vec<i64>, id: i64) {
    if id > 0 && !list.contains(&id) {
        list.push(id);
    }
}

/// One build by id, through the same query so the expiry rule cannot drift:
/// an expired anonymous build is simply not found.
pub async fn select_build(db: &Db, lang: &str, id: i64) -> crate::db::Result<Option<BuildCard>> {
    let found = select_where(db, lang, &format!(" AND record::id(id) = {id}"), "latest", "", 1, 0).await?;
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
    /// stamped onto the record on insert; edits never touch it
    #[allow(dead_code)]
    pub author: String,
    pub user_id: i64,
    /// unix seconds for an anonymous build, None for a permanent one
    pub expires_at: Option<i64>,
}

/// Builds take max+1 like everything else.
async fn next_build_id(db: &Db) -> crate::db::Result<i64> {
    let mut res = db
        .query("LET $ids = (SELECT VALUE record::id(id) FROM build); RETURN array::max($ids) ?? 0;")
        .await?;
    let max = res.take::<Option<i64>>(1)?;
    Ok(max.unwrap_or(0) + 1)
}

fn build_sets(b: &NewBuild) -> String {
    format!(
        concat!(
            "cookie_id = {c},",
            "cookie2_id = {c2},",
            "pet_id = {p},",
            "treasure1_id = {t1},",
            "treasure2_id = {t2},",
            "treasure3_id = {t3},",
            "treasure1_blessed = {b1},",
            "treasure2_blessed = {b2},",
            "treasure3_blessed = {b3},",
            "treasure1_level = {l1},",
            "treasure2_level = {l2},",
            "treasure3_level = {l3},",
            "ep = {ep},",
            "ep_special = {eps},",
            "score = {s},",
            "coin = {co},",
            "time = {t},",
            "boxes = {bx}",
        ),
        c = b.cookie,
        c2 = b.cookie2.map(|v| v.to_string()).unwrap_or_else(|| "NONE".into()),
        p = b.pet,
        t1 = b.treasures[0],
        t2 = b.treasures[1],
        t3 = b.treasures[2],
        b1 = b.blessed[0],
        b2 = b.blessed[1],
        b3 = b.blessed[2],
        l1 = b.levels[0],
        l2 = b.levels[1],
        l3 = b.levels[2],
        ep = b.ep,
        eps = b.ep_special,
        s = b.score,
        co = b.coin,
        t = b.time_ms,
        bx = b.boxes,
    )
}

/// Inserts a build and returns its id, or 0 when the write failed.
pub async fn insert_build(db: &Db, b: &NewBuild) -> crate::db::Result<i64> {
    let id = next_build_id(db).await?;
    let sql = format!(
        "CREATE build:{id} SET {}, created_at = {now},
            tag = $tags,
            description = $desc, youtube_url = $yt,
            author = $author, user_id = {uid}, expires_at = $exp",
        build_sets(b),
        uid = b.user_id,
        now = now_unix(),
    );
    // an explicit 0 reads back as "permanent" everywhere the expiry rule looks;
    // binding None would store NONE and read back identically
    let exp = b.expires_at.unwrap_or(0);
    match db
        .query(&sql)
        .bind(("tags", b.tags.clone()))
        .bind(("desc", b.description.clone()))
        .bind(("yt", b.youtube_url.clone()))
        .bind(("exp", exp))
        .await
        .and_then(|r| r.check().map(|_| ()))
    {
        Ok(_) => Ok(id),
        Err(_) => Ok(0),
    }
}

pub async fn delete_build(db: &Db, id: i64) -> crate::db::Result<()> {
    db.query("DELETE review WHERE build_id = $id")
        .bind(("id", id))
        .await?
        .check()?;
    db.query("DELETE type::record(\"build\", $id)")
        .bind(("id", id))
        .await?
        .check()?;
    Ok(())
}

/// Updates a build in place. Author, owner and expiry are untouched: an edit
/// must not turn an anonymous build permanent or change who owns it.
pub async fn update_build(db: &Db, id: i64, b: &NewBuild) -> crate::db::Result<()> {
    let sql = format!(
        "UPDATE build:{id} SET {},
            tag = $tags, description = $desc, youtube_url = $yt",
        build_sets(b),
    );
    db.query(&sql)
        .bind(("tags", b.tags.clone()))
        .bind(("desc", b.description.clone()))
        .bind(("yt", b.youtube_url.clone()))
        .await?
        .check()?;
    Ok(())
}

/// The verify tallies on a build: how many people confirmed it works and how
/// many reported an issue.
pub async fn review_counts(db: &Db, build_id: i64) -> crate::db::Result<(i64, i64)> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct Group {
        verified: bool,
        count: i64,
    }
    let groups: Vec<Group> = db
        .query(
            "SELECT verified, count() AS count FROM review WHERE build_id = $id GROUP BY verified",
        )
        .bind(("id", build_id))
        .await?
        .take(0)?;
    let mut ok = 0;
    let mut bad = 0;
    for g in groups {
        if g.verified {
            ok += g.count;
        } else {
            bad += g.count;
        }
    }
    Ok((ok, bad))
}

/// One person's verdict, replacing any earlier one from the same user. The
/// record id IS the (build, user) pair, so this is an upsert by construction
/// rather than a second vote.
pub async fn upsert_review(
    db: &Db,
    build_id: i64,
    user_id: i64,
    verified: bool,
    reason: &str,
) -> crate::db::Result<()> {
    let now = now_unix();
    let sql = format!(
        "UPSERT review:[{build_id},{user_id}] SET
            build_id = {build_id}, user_id = {user_id}, verified = {},
            reason = $reason, updated_at = {now}",
        verified as i64
    );
    db.query(&sql)
        .bind(("reason", reason.to_string()))
        .await?
        .check()?;
    Ok(())
}
