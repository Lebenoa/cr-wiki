//! The cookie/pet/treasure picker lists, cached per language.
//!
//! These are identical for every visitor of a language and only change when
//! an admin writes the catalog, yet the planner and its htmx partials would
//! otherwise rebuild them per request — the treasure list alone reads 813
//! treasures, their translations, their effect links and those translations,
//! then sorts. Built once per language here and dropped on a catalog write,
//! which is what app/options_cache.v does.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rusqlite::params;

use crate::db::Db;
use crate::grade;

/// One selectable entity, with the effect lines a treasure card prints.
#[derive(Debug, Clone, Default)]
pub struct PickerOption {
    pub id: i64,
    pub name: String,
    pub en_name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub is_evolved: bool,
    pub effects: Vec<EffectOption>,
    pub effects_blessed: Vec<EffectOption>,
}

impl PickerOption {
    pub fn grade_slug(&self) -> String {
        self.grade.map(grade::slug).unwrap_or("").to_string()
    }
    pub fn grade_label(&self) -> String {
        self.grade.map(grade::label).unwrap_or_default()
    }
    pub fn has_grade(&self) -> bool {
        self.grade.is_some()
    }
    /// A blessed set worth toggling between: present, and different from the
    /// normal one.
    pub fn has_blessed_toggle(&self) -> bool {
        !self.effects_blessed.is_empty()
            && !self.effects.is_empty()
            && self.effects_blessed != self.effects
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectOption {
    pub text: String,
    pub values: Vec<String>,
}

impl EffectOption {
    pub fn values_attr(&self) -> String {
        self.values.join("|")
    }
    pub fn top(&self) -> String {
        self.values.last().cloned().unwrap_or_default()
    }
    pub fn has_values(&self) -> bool {
        !self.values.is_empty()
    }
}

#[derive(Default)]
struct Cache {
    cookies: HashMap<String, Vec<PickerOption>>,
    pets: HashMap<String, Vec<PickerOption>>,
    treasures: HashMap<String, Vec<PickerOption>>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Drops every cached list. Called after any catalog write, since a new or
/// renamed entity has to show up in the pickers on the next request.
#[allow(dead_code)]
pub fn invalidate() {
    if let Ok(mut c) = cache().lock() {
        c.cookies.clear();
        c.pets.clear();
        c.treasures.clear();
    }
}

/// The picker list for `kind` in `lang`, built on first use.
pub fn options(db: &Db, lang: &str, kind: &str) -> Vec<PickerOption> {
    {
        let guard = cache().lock();
        if let Ok(c) = guard {
            let hit = match kind {
                "cookie" => c.cookies.get(lang),
                "pet" => c.pets.get(lang),
                _ => c.treasures.get(lang),
            };
            if let Some(list) = hit {
                return list.clone();
            }
        }
    }
    let built = match kind {
        "cookie" => build_simple(db, lang, "cookie"),
        "pet" => build_simple(db, lang, "pet"),
        _ => build_treasures(db, lang),
    }
    .unwrap_or_default();

    if let Ok(mut c) = cache().lock() {
        match kind {
            "cookie" => c.cookies.insert(lang.to_string(), built.clone()),
            "pet" => c.pets.insert(lang.to_string(), built.clone()),
            _ => c.treasures.insert(lang.to_string(), built.clone()),
        };
    }
    built
}

/// Cookies and pets: newest first, matching the catalog order so the picker
/// presents the same sequence as the list page.
fn build_simple(db: &Db, lang: &str, kind: &str) -> rusqlite::Result<Vec<PickerOption>> {
    let (table, id_col, owner_col, tr_table) = match kind {
        "pet" => ("pet", "pet_id", "pet_id", "pet_translation"),
        _ => ("cookie", "cookie_id", "owner_id", "cookie_translation"),
    };
    let c = db
        .get()
        .map_err(|e| rusqlite::Error::InvalidParameterName(format!("pool: {e}")))?;
    let sql = format!(
        "SELECT e.{id_col} AS id, e.image AS image, e.grade AS grade,
                COALESCE(tl.name, te.name) AS name, COALESCE(te.name, '') AS en_name
           FROM {table} e
           LEFT JOIN {tr_table} tl ON tl.{owner_col} = e.{id_col} AND tl.lang = ?1
           LEFT JOIN {tr_table} te ON te.{owner_col} = e.{id_col} AND te.lang = 'en'
          WHERE tl.name IS NOT NULL OR te.name IS NOT NULL
          ORDER BY e.{id_col} DESC"
    );
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt.query_map(params![lang], |r| {
        Ok(PickerOption {
            id: r.get("id")?,
            name: r.get("name")?,
            en_name: r.get("en_name")?,
            image: r.get("image")?,
            grade: r.get("grade")?,
            ..Default::default()
        })
    })?;
    rows.collect()
}

/// Treasures, with every effect line per state, ordered grade-then-newest-
/// then-name so the picker matches the /treasures list. Power+ treasures are
/// friendly-run bonuses that cannot be equipped, so they are left out.
fn build_treasures(db: &Db, lang: &str) -> rusqlite::Result<Vec<PickerOption>> {
    let c = db
        .get()
        .map_err(|e| rusqlite::Error::InvalidParameterName(format!("pool: {e}")))?;
    let sql = format!(
        "SELECT e.treasure_id AS id, e.image AS image, e.grade AS grade,
                e.is_evolved AS is_evolved,
                COALESCE(tl.name, te.name) AS name, COALESCE(te.name, '') AS en_name
           FROM treasure e
           LEFT JOIN treasure_translation tl ON tl.treasure_id = e.treasure_id AND tl.lang = ?1
           LEFT JOIN treasure_translation te ON te.treasure_id = e.treasure_id AND te.lang = 'en'
          WHERE (tl.name IS NOT NULL OR te.name IS NOT NULL) AND e.is_power_plus = 0
          ORDER BY {rank} DESC, e.release_date DESC, COALESCE(tl.name, te.name) ASC",
        rank = grade::rank_sql("e.grade")
    );
    let mut stmt = c.prepare(&sql)?;
    let mut list: Vec<PickerOption> = stmt
        .query_map(params![lang], |r| {
            Ok(PickerOption {
                id: r.get("id")?,
                name: r.get("name")?,
                en_name: r.get("en_name")?,
                image: r.get("image")?,
                grade: r.get("grade")?,
                is_evolved: r.get::<_, i64>("is_evolved")? != 0,
                ..Default::default()
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    // every effect link and every level row in two queries, then stitched —
    // one query per treasure would be 813 round trips
    let mut links = c.prepare(
        "SELECT te.treasure_id AS tid, te.effect_id AS eid, te.state AS state,
                COALESCE(el.name, ee.name, '') AS text
           FROM treasure_effect te
           LEFT JOIN effect_translation el ON el.effect_id = te.effect_id AND el.lang = ?1
           LEFT JOIN effect_translation ee ON ee.effect_id = te.effect_id AND ee.lang = 'en'
          ORDER BY te.treasure_effect_id",
    )?;
    let link_rows: Vec<(i64, i64, i64, String)> = links
        .query_map(params![lang], |r| {
            Ok((r.get("tid")?, r.get("eid")?, r.get("state")?, r.get("text")?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    drop(links);

    let mut levels = c.prepare(
        "SELECT treasure_id AS tid, effect_id AS eid, state AS state, [values] AS v
           FROM treasure_level ORDER BY level",
    )?;
    let mut ladders: HashMap<(i64, i64, i64), Vec<String>> = HashMap::new();
    let level_rows = levels.query_map([], |r| {
        Ok((
            r.get::<_, i64>("tid")?,
            r.get::<_, i64>("eid")?,
            r.get::<_, i64>("state")?,
            r.get::<_, String>("v")?,
        ))
    })?;
    for row in level_rows {
        let (tid, eid, state, v) = row?;
        ladders.entry((tid, eid, state)).or_default().push(v);
    }
    drop(levels);

    let mut by_treasure: HashMap<i64, (Vec<EffectOption>, Vec<EffectOption>)> = HashMap::new();
    let mut seen: Vec<(i64, i64, i64)> = Vec::new();
    for (tid, eid, state, text) in link_rows {
        // a treasure can link the same effect twice; the wiki shows it once
        if seen.contains(&(tid, eid, state)) {
            continue;
        }
        seen.push((tid, eid, state));
        let option = EffectOption {
            text,
            values: ladders.get(&(tid, eid, state)).cloned().unwrap_or_default(),
        };
        let slot = by_treasure.entry(tid).or_default();
        if state == 1 {
            slot.1.push(option);
        } else {
            slot.0.push(option);
        }
    }

    for opt in &mut list {
        if let Some((normal, blessed)) = by_treasure.remove(&opt.id) {
            opt.effects = normal;
            opt.effects_blessed = blessed;
        }
    }
    Ok(list)
}
