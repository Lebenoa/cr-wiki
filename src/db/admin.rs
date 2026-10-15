//! The editor's writes: creating and updating an entity together with its
//! translation.

use surrealdb::types::SurrealValue;

use crate::grade;
use crate::section::Section;
use crate::time::now_unix;

use super::types::entity_table;
use super::{Db, Result};

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
// The Hinnant days-from-civil arithmetic is provably in-range: the parts
// are validated to YYYY-MM-DD just above.
#[allow(clippy::arithmetic_side_effects)]
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
    let (y, m, d) = (
        y.parse::<i64>().ok()?,
        m.parse::<i64>().ok()?,
        d.parse::<i64>().ok()?,
    );
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
    let days = era * 146_097 + doe - 719_468;
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
