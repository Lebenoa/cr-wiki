//! Combo bonuses, the batched name lookup they share with the picker and
//! rich-text links, and the admin editor's combo rows.

use surrealdb::types::SurrealValue;

use crate::section::Section;

use super::types::{tr, CardRow, Kind};
use super::{Db, Result};

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
            tracing::warn!(
                table = kind.table,
                id,
                lang,
                "name lookup matched no row; callers render an empty name and no thumbnail"
            );
        }
    }
    Ok(map)
}

/// The combo bonuses a cookie or pet takes part in, partner resolved. Each
/// `combi` row hangs between one cookie and one pet, so a pet's page reads
/// the same rows through `pet_id` that the cookie's page reads through
/// `cookie_id`.
async fn combis(db: &Db, lang: &str, own: &str, id: i64) -> Result<Vec<(CombiRecord, CombiRow)>> {
    let (own_col, partner_kind) = match own {
        "cookies" => ("cookie_id", Kind::of(Section::Pets)),
        "pets" => ("pet_id", Kind::of(Section::Cookies)),
        // pairings only exist between a cookie and a pet
        _ => return Ok(Vec::new()),
    };
    if id <= 0 {
        return Ok(Vec::new());
    }
    let records: Vec<CombiRecord> = db
        .query(format!(
            "SELECT record::id(id) AS id, cookie_id, pet_id, en, th, is_hidden
               FROM combi WHERE {own_col} = $id ORDER BY id",
        ))
        .bind(("id", id))
        .await?
        .take(0)?;

    // the partner is the row's other side: a pet on a cookie's page, a
    // cookie on a pet's page
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

/// One combo row as the admin editor lists it: the pairing's own id, so a row
/// can be updated or removed, alongside the partner and the effect.
#[derive(Debug, Clone)]
pub struct CombiEditRow {
    pub id: i64,
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
            partner_name: row.partner_name,
            partner_image: row.partner_image,
            effect: row.effect,
            is_hidden: row.is_hidden,
        })
        .collect())
}

/// Removes one combo pairing by record id, only when it hangs off the given
/// cookie or pet: the route passes the id from the editor's path, so a
/// guessed row id cannot delete a pairing another editor displays.
pub async fn delete_combi(db: &Db, row_id: i64, own: &str, own_id: i64) -> Result<bool> {
    let own_col = match own {
        "cookies" => "cookie_id",
        "pets" => "pet_id",
        _ => return Ok(false),
    };
    let mut res = db
        .query(format!(
            "DELETE type::record(\"combi\", $id) WHERE {own_col} = $own RETURN AFTER"
        ))
        .bind(("id", row_id))
        .bind(("own", own_id))
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
}
