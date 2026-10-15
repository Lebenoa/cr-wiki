//! The disclosed draw pools and their odds.

use surrealdb::types::SurrealValue;

use crate::grade;

use super::types::tr;
use super::{Db, Result};

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
          WHERE record::id(id) IN $ids",
        tr("name"),
    );
    // ints, not "table:42" strings: v3 does not coerce a string to a record
    // id inside `IN`, so the string form silently matches nothing (the
    // same shape `cards_by_ids` projects and binds)
    let rows: Vec<PrizeRow> = db
        .query(sql.as_str())
        .bind(("lang", lang.to_string()))
        .bind(("ids", ids.to_vec()))
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

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use surrealdb::types::{Object, Value};

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
}
