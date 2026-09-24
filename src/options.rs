//! The cookie/pet/treasure picker lists, cached per language.
//!
//! These are identical for every visitor of a language and only change when
//! an admin writes the catalog, yet the planner and its htmx partials would
//! otherwise rebuild them per request — the treasure list alone reads every
//! treasure with its effect ladder and then sorts. Built once per language
//! here and dropped on a catalog write, which is what `app/options_cache.v`
//! did.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use surrealdb::engine::any::Any;
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;

use crate::grade;

type Db = Surreal<Any>;

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
    /// A blessed set worth toggling between: present, and different from the
    /// normal one.
    pub fn has_blessed_toggle(&self) -> bool {
        !self.effects_blessed.is_empty()
            && !self.effects.is_empty()
            && self.effects_blessed != self.effects
    }
}

impl grade::Graded for PickerOption {
    fn grade(&self) -> Option<i64> {
        self.grade
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
    pub const fn has_values(&self) -> bool {
        !self.values.is_empty()
    }
}
#[derive(Default)]
struct Cache {
    cookies: HashMap<String, Arc<Vec<PickerOption>>>,
    pets: HashMap<String, Arc<Vec<PickerOption>>>,
    treasures: HashMap<String, Arc<Vec<PickerOption>>>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: std::sync::OnceLock<Mutex<Cache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// A poisoned cache means a builder panicked mid-write; the lists are
/// rebuildable, so drop the stale contents and start over. Every lock site
/// heals the same way — two different recoveries were two bugs waiting.
fn heal(p: PoisonError<MutexGuard<'_, Cache>>) -> MutexGuard<'_, Cache> {
    let mut c = p.into_inner();
    *c = Cache::default();
    c
}

/// Drops every cached list. Called after any catalog write, since a new or
/// renamed entity has to show up in the pickers on the next request.
pub fn invalidate() {
    if let Ok(mut c) = cache().lock() {
        *c = Cache::default();
    }
}

/// The picker list for `kind` in `lang`, built on first use. Hits hand back
/// a shared `Arc` — the grid clones single options, never the whole list —
/// and a build failure is an `Err`: an empty picker on a healthy page would
/// read as "no cookies exist".
pub async fn options(db: &Db, lang: &str, kind: &str) -> crate::db::Result<Arc<Vec<PickerOption>>> {
    let key = lang.to_string();
    let hit = {
        let c = cache().lock().unwrap_or_else(heal);
        match kind {
            "cookie" => c.cookies.get(&key).cloned(),
            "pet" => c.pets.get(&key).cloned(),
            _ => c.treasures.get(&key).cloned(),
        }
    };
    if let Some(list) = hit {
        return Ok(list);
    }
    // double-checked build: two concurrent misses build twice, last insert
    // wins, both lists are identical because they read the same snapshot
    let built: Vec<PickerOption> = match kind {
        "cookie" => build_simple(db, lang, "cookie").await?,
        "pet" => build_simple(db, lang, "pet").await?,
        _ => build_treasures(db, lang).await?,
    };
    let list = Arc::new(built);
    let mut c = cache().lock().unwrap_or_else(heal);
    match kind {
        "cookie" => c.cookies.insert(key, Arc::clone(&list)),
        "pet" => c.pets.insert(key, Arc::clone(&list)),
        _ => c.treasures.insert(key, Arc::clone(&list)),
    };
    Ok(list)
}

/// Cookies and pets: newest first, matching the catalog order so the picker
/// presents the same sequence as the list page.
async fn build_simple(db: &Db, lang: &str, kind: &str) -> crate::db::Result<Vec<PickerOption>> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct Row {
        id: i64,
        name: String,
        en_name: String,
        image: Option<String>,
        grade: Option<i64>,
    }
    let table = match kind {
        "pet" => "pet",
        _ => "cookie",
    };
    let rows: Vec<Row> = db
        .query(
            format!(
                "SELECT record::id(id) AS id, image, grade,
                        (tr[$lang].name ?? tr.en.name ?? '') AS name,
                        (tr.en.name ?? '') AS en_name
                   FROM {table}
                  WHERE tr.en.name != NONE OR tr[$lang].name != NONE
                  ORDER BY id DESC"
            )
            .as_str(),
        )
        .bind(("lang", lang.to_string()))
        .await?
        .take(0)?;
    Ok(rows
        .into_iter()
        .map(|r| PickerOption {
            id: r.id,
            name: r.name,
            en_name: r.en_name,
            image: r.image,
            grade: r.grade,
            ..Default::default()
        })
        .collect())
}

/// Treasures, with every effect line per state, ordered grade-then-newest-
/// then-name so the picker matches the /treasures list. Power+ treasures are
/// friendly-run bonuses that cannot be equipped, so they are left out. The
/// effect ladders ride along on each record — no second query needed.
async fn build_treasures(db: &Db, lang: &str) -> crate::db::Result<Vec<PickerOption>> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct EffectLineRow {
        state: i64,
        en: String,
        th: String,
        values: Vec<String>,
    }
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct Row {
        id: i64,
        name: String,
        en_name: String,
        image: Option<String>,
        grade: Option<i64>,
        is_evolved: bool,
        #[allow(dead_code)]
        rank: i64,
        #[allow(dead_code)]
        release_date: i64,
        effect_lines: Vec<EffectLineRow>,
    }

    let rows: Vec<Row> = db
        .query(
            "SELECT record::id(id) AS id, image, grade, is_evolved, rank, release_date,
                    (tr[$lang].name ?? tr.en.name ?? '') AS name,
                    (tr.en.name ?? '') AS en_name,
                    effect_lines
               FROM treasure
              WHERE (tr.en.name != NONE OR tr[$lang].name != NONE)
                AND (is_power_plus ?? false) = false
              ORDER BY rank DESC, release_date DESC, name ASC",
        )
        .bind(("lang", lang.to_string()))
        .await?
        .take(0)?;

    // effect_lines are already deduped by (effect, state) at write time, so
    // every line here is a distinct wiki row
    Ok(rows
        .into_iter()
        .map(|r| {
            let mut opt = PickerOption {
                id: r.id,
                name: r.name,
                en_name: r.en_name,
                image: r.image,
                grade: r.grade,
                is_evolved: r.is_evolved,
                ..Default::default()
            };
            for line in &r.effect_lines {
                let text = crate::i18n::pick_text(lang, &line.en, &line.th);
                let option = EffectOption {
                    text,
                    values: line.values.clone(),
                };
                if line.state == 1 {
                    opt.effects_blessed.push(option);
                } else {
                    opt.effects.push(option);
                }
            }
            opt
        })
        .collect())
}
