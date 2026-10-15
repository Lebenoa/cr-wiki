//! Per-entity detail reads: the detail row itself (strict and
//! locale-fallback), the kinds' own collections, treasure effects and
//! the admin editor's link rows.

use surrealdb::types::SurrealValue;

use crate::section::Section;

use super::combi::{entity_link, names_for};
use super::types::{tr, CardRow, Detail, Kind};
use super::{Db, Result};

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
/// locale -> English through one batched lookup per maker kind — the same
/// `names_for` the combi panel uses — rather than one query per maker.
pub async fn jelly_makers(db: &Db, lang: &str, jelly_id: i64) -> Result<Vec<JellyMaker>> {
    let rows: Vec<MakerRow> = db
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
    let mut cookie_ids: Vec<i64> = Vec::new();
    let mut pet_ids: Vec<i64> = Vec::new();
    let mut treasure_ids: Vec<i64> = Vec::new();
    for row in &rows {
        let list = match row.entity_kind.as_str() {
            "pet" => &mut pet_ids,
            "treasure" => &mut treasure_ids,
            _ => &mut cookie_ids,
        };
        if !list.contains(&row.entity_id) {
            list.push(row.entity_id);
        }
    }
    let (kc, kp, kt) = (
        Kind::of(Section::Cookies),
        Kind::of(Section::Pets),
        Kind::of(Section::Treasures),
    );
    let (cookies, pets, treasures) = tokio::join!(
        names_for(db, lang, &kc, &cookie_ids),
        names_for(db, lang, &kp, &pet_ids),
        names_for(db, lang, &kt, &treasure_ids),
    );
    // a failed lookup empties the map: the maker then renders without a
    // name and is skipped, exactly as a failed `entity_link` did
    let (cookies, pets, treasures) = (
        cookies.unwrap_or_default(),
        pets.unwrap_or_default(),
        treasures.unwrap_or_default(),
    );
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (section, names) = match row.entity_kind.as_str() {
            "pet" => ("pets", &pets),
            "treasure" => ("treasures", &treasures),
            _ => ("cookies", &cookies),
        };
        let Some((name, image)) = names.get(&row.entity_id) else {
            continue;
        };
        out.push(JellyMaker {
            kind: section.to_string(),
            id: row.entity_id,
            name: name.clone(),
            image: image.clone(),
        });
    }
    Ok(out)
}

/// A treasure row's evolution/unlock link ids, as the edit form preselects
/// them. Zeros mean "no link" — the form renders those selects with a `None`
/// option first.
#[derive(Debug, Clone, Copy, Default)]
pub struct TreasureEditLinks {
    pub base: i64,
    pub unlock_cookie: i64,
    pub unlock_pet: i64,
}

/// Reads the three link ids for the treasure editor in one query.
pub async fn treasure_edit_links(db: &Db, id: i64) -> Result<TreasureEditLinks> {
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct Row {
        base: i64,
        unlock_cookie: i64,
        unlock_pet: i64,
    }
    let mut rows: Vec<Row> = db
        .query(
            "SELECT (base_treasure_id ?? 0) AS base,
                    (unlock_cookie_id ?? 0) AS unlock_cookie,
                    (unlock_pet_id ?? 0) AS unlock_pet
               FROM type::record('treasure', $id)",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    Ok(rows
        .pop()
        .map(|r| TreasureEditLinks {
            base: r.base,
            unlock_cookie: r.unlock_cookie,
            unlock_pet: r.unlock_pet,
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

/// One treasure an ingredient crafts (from `ingredient_recipe`), with its
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
    // One message: the evolved row's base id is read in the same statement
    // that then fetches the variant, instead of one round trip to discover
    // the id and another to resolve it. A base of 0 (or a row missing its
    // link) matches no treasure, since ids start at 1.
    let sql = format!(
        "LET $base = (SELECT VALUE base_treasure_id FROM type::record(\"treasure\", $id))[0] ?? 0;
         SELECT record::id(id) AS id, image, {tr_name} AS name,
                (tr.en.name ?? '') AS en_name
           FROM treasure
          WHERE ($evolved AND record::id(id) = $base)
             OR ($evolved = false AND base_treasure_id = $id)
          LIMIT 1",
        tr_name = tr("name"),
    );
    let mut rows: Vec<CardRow> = db
        .query(sql)
        .bind(("lang", lang.to_string()))
        .bind(("id", id))
        .bind(("evolved", is_evolved))
        .await?
        // statement 0 is the LET, statement 1 the select
        .take(1)?;
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

/// One effect-line row type `options` shares: same shape on the record, one
/// decode one locale rule. `pub` inside this private module, so both read
/// paths decode it the same way.
#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
pub struct EffectLineRow {
    pub state: i64,
    pub en: String,
    pub th: String,
    pub values: Vec<String>,
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

/// The cookie or pet that unlocks a treasure at max level.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_field_names)] // the unlock_* prefix is the domain, not noise
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

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use surrealdb::types::{Object, Value};

    /// The regression a lint pass once hid: a row struct whose field names
    /// drift from the projected columns decodes every row as `None`, so a
    /// renamed or re-projected column shows up as silently absent data. These
    /// build the exact objects the queries project, with no database behind.
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
}
