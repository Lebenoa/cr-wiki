//! One episode's detail page: the six child collections the row itself
//! does not carry, read with one grouped query each.

use surrealdb::types::SurrealValue;

use super::catalog::EpisodeListRow;
use super::types::tr;
use super::{Db, Result};

/// One stage pill on the episode detail page.
#[derive(Debug, Clone, Default)]
pub struct EpisodeStage {
    pub stage_no: i64,
    pub name: String,
}

/// One relic of the episode's completion set.
#[derive(Debug, Clone, Default)]
pub struct EpisodeRelicCard {
    pub name: String,
    pub image: Option<String>,
}

/// One quest row, grouped by its chain (`group`).
#[derive(Debug, Clone, Default)]
pub struct EpisodeQuest {
    pub name: String,
    pub reward: String,
}

/// A quest chain in source order, preserving the seed's group order.
#[derive(Debug, Clone, Default)]
pub struct EpisodeQuestGroup {
    pub group: String,
    pub quests: Vec<EpisodeQuest>,
}

/// One relic-draw reward with its disclosed odds.
#[derive(Debug, Clone, Default)]
pub struct DrawReward {
    pub rank: i64,
    pub reward: String,
    pub odds: f64,
}

impl DrawReward {
    /// The odds as the page prints them: no padding, so 7.69 reads "7.69%".
    pub fn odds_label(&self) -> String {
        odds_pct(self.odds)
    }
}

/// One mystery-box grade row with the three disclosed box columns.
#[derive(Debug, Clone, Default)]
pub struct BoxGrade {
    pub grade: String,
    pub first: f64,
    pub second: f64,
    pub third: f64,
}

impl BoxGrade {
    pub fn first_label(&self) -> String {
        odds_pct(self.first)
    }
    pub fn second_label(&self) -> String {
        odds_pct(self.second)
    }
    pub fn third_label(&self) -> String {
        odds_pct(self.third)
    }
}

/// Row projections for the episode detail child queries.
#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct StageRow {
    stage_no: i64,
    name: String,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct RelicCardRow {
    rid: i64,
    name: String,
    image: Option<String>,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct QuestRow {
    grp: String,
    name: String,
    reward: String,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct DrawRow {
    rank: i64,
    reward: String,
    odds: f64,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct BoxRow {
    box_grade: String,
    box_no: i64,
    odds: f64,
}

#[derive(Debug, Default, SurrealValue)]
#[surreal(default)]
struct IngRow {
    id: i64,
    name: String,
    image: Option<String>,
    grade: Option<i64>,
    coin_value: i64,
}

/// Odds as "4.4%" — the stored value with trailing zeros trimmed.
fn odds_pct(v: f64) -> String {
    let mut s = format!("{v}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    format!("{s}%")
}

/// One ingredient that drops in this episode.
#[derive(Debug, Clone, Default)]
pub struct EpisodeIngredient {
    pub name: String,
    pub image: Option<String>,
    pub grade: Option<i64>,
    pub coin_value: i64,
}

/// One episode's full detail page data: the row itself plus its six child
/// sections (stages, relic set, quest chains, draw rewards, box odds,
/// ingredient drops), as `templates/detail.html`'s episode branch renders
/// them. Mirrors the old `select_episode` shape.
#[derive(Debug, Clone, Default)]
pub struct EpisodeDetail {
    pub kind: String,
    pub stars: i64,
    pub league_ranked: bool,
    pub entry_cost: String,
    pub stages: Vec<EpisodeStage>,
    pub relics: Vec<EpisodeRelicCard>,
    pub quest_groups: Vec<EpisodeQuestGroup>,
    pub quest_count: i64,
    pub draw_rewards: Vec<DrawReward>,
    pub box_grades: Vec<BoxGrade>,
    pub ingredients: Vec<EpisodeIngredient>,
}

/// Everything the episode detail page shows besides name/description/image
/// (which `select_detail` already carries). Child tables are read with one
/// grouped query each; names resolve locale -> English.
#[allow(clippy::too_many_lines)] // six independent child reads, one block each
pub async fn episode_extras(db: &Db, lang: &str, id: i64) -> Result<EpisodeDetail> {
    let mut out = EpisodeDetail::default();

    let mut parents: Vec<EpisodeListRow> = db
        .query(
            "SELECT (kind ?? '') AS kind, (stars ?? 0) AS stars,
                    (league_ranked ?? false) AS league_ranked, (entry_cost ?? '') AS entry_cost
               FROM type::record('episode', $id)",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    if let Some(p) = parents.pop() {
        out.kind = p.kind;
        out.stars = p.stars;
        out.league_ranked = p.league_ranked;
        out.entry_cost = p.entry_cost;
    }

    let stages: Vec<StageRow> = db
        .query(
            "SELECT stage_no, (name ?? '') AS name FROM episode_stage
              WHERE episode_id = $id ORDER BY stage_no",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    out.stages = stages
        .into_iter()
        .map(|s| EpisodeStage {
            stage_no: s.stage_no,
            name: s.name,
        })
        .collect();

    // the completion relic set: the junction's plain-int relic_ids point at
    // relic records whose record id IS that number (relic:500101), so the
    // names resolve with one query over the id list
    let relic_ids: Vec<i64> = db
        .query("SELECT VALUE relic_id FROM episode_relic WHERE episode_id = $id ORDER BY relic_id")
        .bind(("id", id))
        .await?
        .take(0)?;
    if !relic_ids.is_empty() {
        let mut relic_rows: Vec<RelicCardRow> = db
            .query(
                format!(
                    "SELECT record::id(id) AS rid, image,
                            {name} AS name
                       FROM relic WHERE record::id(id) IN $ids",
                    name = tr("name"),
                )
                .as_str(),
            )
            .bind(("lang", lang.to_string()))
            .bind(("ids", relic_ids))
            .await?
            .take(0)?;
        // junction order (relic_id ascending), not the query's return order
        relic_rows.sort_by_key(|r| r.rid);
        out.relics = relic_rows
            .into_iter()
            .map(|r| EpisodeRelicCard {
                name: r.name,
                image: r.image,
            })
            .collect();
    }

    // quest chains in source order, grouped by their `group` field
    let quests: Vec<QuestRow> = db
        .query(
            "SELECT (group ?? '') AS grp, (name ?? '') AS name, (reward ?? '') AS reward
               FROM quest WHERE episode_id = $id ORDER BY id",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    out.quest_count = i64::try_from(quests.len()).unwrap_or(i64::MAX);
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<EpisodeQuest>> =
        std::collections::HashMap::new();
    for q in quests {
        if !groups.contains_key(&q.grp) {
            order.push(q.grp.clone());
        }
        groups.entry(q.grp).or_default().push(EpisodeQuest {
            name: q.name,
            reward: q.reward,
        });
    }
    out.quest_groups = order
        .into_iter()
        .map(|g| EpisodeQuestGroup {
            quests: groups.remove(&g).unwrap_or_default(),
            group: g,
        })
        .collect();

    let draws: Vec<DrawRow> = db
        .query(
            "SELECT rank, (reward ?? '') AS reward, (odds ?? 0) AS odds
               FROM episode_draw_reward WHERE episode_id = $id ORDER BY rank",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    out.draw_rewards = draws
        .into_iter()
        .map(|d| DrawReward {
            rank: d.rank,
            reward: d.reward,
            odds: d.odds,
        })
        .collect();

    // one row per box grade with the three disclosed box columns; a grade
    // missing a box keeps 0 for it, which prints as "0%"
    let boxes: Vec<BoxRow> = db
        .query(
            "SELECT (box_grade ?? '') AS box_grade, box_no, (odds ?? 0) AS odds
               FROM episode_box_odds WHERE episode_id = $id ORDER BY box_no",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    let mut grade_order: Vec<String> = Vec::new();
    let mut by_grade: std::collections::HashMap<String, BoxGrade> =
        std::collections::HashMap::new();
    for b in boxes {
        if !by_grade.contains_key(&b.box_grade) {
            grade_order.push(b.box_grade.clone());
            by_grade.insert(
                b.box_grade.clone(),
                BoxGrade {
                    grade: b.box_grade.clone(),
                    first: 0.0,
                    second: 0.0,
                    third: 0.0,
                },
            );
        }
        // the key was inserted two lines above, so the get cannot miss
        let Some(g) = by_grade.get_mut(&b.box_grade) else {
            continue;
        };
        match b.box_no {
            1 => g.first = b.odds,
            2 => g.second = b.odds,
            _ => g.third = b.odds,
        }
    }
    out.box_grades = grade_order
        .into_iter()
        .filter_map(|g| by_grade.remove(&g))
        .collect();

    // the ingredients that drop in this episode, id order like the old page
    let mut ings: Vec<IngRow> = db
        .query(
            format!(
                "SELECT record::id(id) AS id, image, (grade ?? NONE) AS grade,
                        (coin_value ?? 0) AS coin_value,
                        {name} AS name
                   FROM ingredient WHERE drop_episode_id = $id ORDER BY id",
                name = tr("name"),
            )
            .as_str(),
        )
        .bind(("lang", lang.to_string()))
        .bind(("id", id))
        .await?
        .take(0)?;
    ings.sort_by_key(|i| i.id);
    out.ingredients = ings
        .into_iter()
        .map(|i| EpisodeIngredient {
            name: i.name,
            image: i.image,
            grade: i.grade,
            coin_value: i.coin_value,
        })
        .collect();

    Ok(out)
}
