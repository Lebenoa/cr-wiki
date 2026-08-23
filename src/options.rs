//! The cookie/pet/treasure picker lists, cached per language.
//!
//! These are identical for every visitor of a language and only change when
//! an admin writes the catalog, yet the planner and its htmx partials would
//! otherwise rebuild them per request — the treasure list alone reads every
//! treasure with its effect ladder and then sorts. Built once per language
//! here and dropped on a catalog write, which is what app/options_cache.v
//! did.

use std::collections::HashMap;
use std::sync::Mutex;

use surrealdb::engine::any::Any;
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
    static CACHE: std::sync::OnceLock<Mutex<Cache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Drops every cached list. Called after any catalog write, since a new or
/// renamed entity has to show up in the pickers on the next request.
pub fn invalidate() {
    if let Ok(mut c) = cache().lock() {
        *c = Cache::default();
    }
}

/// The picker list for `kind` in `lang`, built on first use. Reads are the
/// hot path; the async mutex is only ever held by one builder at a time.
pub async fn options(db: &Db, lang: &str, kind: &str) -> Vec<PickerOption> {
    let key = lang.to_string();
    {
        let c = cache().lock().expect("picker cache poisoned");
        let hit = match kind {
            "cookie" => c.cookies.get(&key),
            "pet" => c.pets.get(&key),
            _ => c.treasures.get(&key),
        };
        if let Some(list) = hit {
            return list.clone();
        }
    }
    // double-checked build: two concurrent misses build twice, last insert
    // wins, both lists are identical because they read the same snapshot
    let built = match kind {
        "cookie" => build_simple(db, lang, "cookie").await.unwrap_or_default(),
        "pet" => build_simple(db, lang, "pet").await.unwrap_or_default(),
        _ => build_treasures(db, lang).await.unwrap_or_default(),
    };

    let mut c = cache().lock().expect("picker cache poisoned");
    match kind {
        "cookie" => c.cookies.insert(key.clone(), built.clone()),
        "pet" => c.pets.insert(key.clone(), built.clone()),
        _ => c.treasures.insert(key.clone(), built.clone()),
    };
    built
}

/// Cookies and pets: newest first, matching the catalog order so the picker
/// presents the same sequence as the list page.
async fn build_simple(
    db: &Db,
    lang: &str,
    kind: &str,
) -> crate::db::Result<Vec<PickerOption>> {
    let table = match kind {
        "pet" => "pet",
        _ => "cookie",
    };
    #[derive(serde::Deserialize)]
    struct Row {
        id: i64,
        name: String,
        en_name: String,
        image: Option<String>,
        grade: Option<i64>,
    }
    let rows: Vec<Row> = db
        .query(&format!(
            "SELECT record::id(id) AS id, image, grade,
                    (tr[$lang].name ?? tr.en.name ?? '') AS name,
                    (tr.en.name ?? '') AS en_name
               FROM {table}
              WHERE tr.en.name != NONE OR tr[$lang].name != NONE
              ORDER BY id DESC"
        ))
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
    #[derive(serde::Deserialize)]
    struct EffectLineRow {
        state: i64,
        #[serde(default)]
        en: String,
        #[serde(default)]
        th: String,
        #[serde(default)]
        values: Vec<String>,
    }
    #[derive(serde::Deserialize)]
    struct Row {
        id: i64,
        name: String,
        en_name: String,
        image: Option<String>,
        grade: Option<i64>,
        is_evolved: bool,
        #[serde(default)]
        #[allow(dead_code)]
        rank: i64,
        #[serde(default)]
        #[allow(dead_code)]
        release_date: i64,
        #[serde(default)]
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
                let text = match lang == "th" && !line.th.is_empty() {
                    true => line.th.clone(),
                    false => line.en.clone(),
                };
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
