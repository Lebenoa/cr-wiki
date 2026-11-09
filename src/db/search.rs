//! Cross-entity search: full-text, name-gram, substring and fuzzy passes,
//! merged table by table.

use super::types::{tr, Card, CardRow};
use super::{Db, Result};

/// Cross-entity search. The needle goes four ways per table, merged here:
/// - the full-text index ranks whole-word hits (`@1@`), one query per
///   locale pair, `search::score` ordering — same tool the V app's FTS5
///   tables gave it;
/// - the name-gram index answers fragments and typos in one mechanism
///   (`name_gram`, seeded with three-character grams of every catalog
///   name, maintained by the `*_gram` ingest events): the needle's own
///   grams are looked up and entities ranked by gram overlap. Prefix
///   (`ginger`), infix, Thai runs and transposed typos (`gingerbrvae`)
///   all reduce to gram overlap — the same trick Meilisearch's typo
///   tolerance is built on. Overlap order is SQL-side (`GROUP BY ...
///   ORDER BY count() DESC`), so the raw group order cannot leak in;
/// - a substring pass over description/abilities prose for needles the
///   grams cannot see (grams cover names only; prose is word-level);
/// - a fuzzy pass, last resort, matching a single-word needle against
///   name and prose words within damerau-levenshtein 1 (whole word or
///   the word's needle-length prefix). Rescues the case the gram trick
///   cannot: a short needle whose one transposition destroys *all* its
///   3-grams (`sruf` vs `surf`, in prose "surfing" — no shared gram, no
///   shared substring).
///
/// The planner refuses `search::score()` in a WHERE that mixes MATCHES
/// with non-index clauses, so the passes cannot be OR'd in SQL; ranked
/// hits keep their order and the later passes fill the tail, deduped by id
/// within their table. The tables are independent, so the six slices run
/// concurrently — the latency is one table's round trips, not
/// twenty-four sequential ones.
pub async fn search(db: &Db, lang: &str, q: &str, limit: i64) -> Result<Vec<(String, Card)>> {
    let query = q.trim();
    // one bound at the edge: the three passes below interpolate the limit,
    // so a caller cannot widen the result work with a huge value
    let limit = limit.clamp(1, 100);
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let needle = query.to_lowercase();
    let is_thai = needle
        .chars()
        .any(|c| matches!(u32::from(c), 0x0E00..=0x0E7F));
    let (cookies, pets, treasures, relics, episodes, ingredients) = tokio::join!(
        search_table(
            db,
            lang,
            "cookies",
            "cookie",
            ", grade",
            "abilities",
            query,
            &needle,
            is_thai,
            limit
        ),
        search_table(
            db,
            lang,
            "pets",
            "pet",
            ", grade",
            "description",
            query,
            &needle,
            is_thai,
            limit
        ),
        search_table(
            db,
            lang,
            "treasures",
            "treasure",
            ", grade",
            "description",
            query,
            &needle,
            is_thai,
            limit
        ),
        search_table(
            db,
            lang,
            "relics",
            "relic",
            "",
            "description",
            query,
            &needle,
            is_thai,
            limit
        ),
        search_table(
            db,
            lang,
            "episodes",
            "episode",
            "",
            "description",
            query,
            &needle,
            is_thai,
            limit
        ),
        search_table(
            db,
            lang,
            "ingredients",
            "ingredient",
            ", grade",
            "description",
            query,
            &needle,
            is_thai,
            limit
        ),
    );
    // table order preserved: misc::search groups consecutive hits by section,
    // which only holds while each table's slice arrives whole. The array is
    // iteration order, not a tuple conversion — the six join results are
    // unrelated tables.
    let mut out = Vec::new();
    // iteration order, not a tuple conversion — the six join results are
    // unrelated tables.
    #[allow(clippy::tuple_array_conversions)]
    for part in [cookies, pets, treasures, relics, episodes, ingredients] {
        out.extend(part?);
    }
    // the fuzzy pass, last resort, only when the whole search found
    // nothing: a single-word latin needle matched against name and prose
    // words within damerau-levenshtein 1 (see `fuzzy_word_pass`). `sruf`
    // against "surfing" prose has no name gram and no substring match, so
    // without this it returns nothing. Gating on the merged result means
    // a partial match anywhere skips the six full-table scans entirely;
    // when they must run, all six go out concurrently like the slices
    // above (a sequential loop costs the sum of the scans, ~250 ms on
    // this catalog).
    if out.is_empty() && !is_thai && !needle.contains(' ') && needle.chars().count() >= 3 {
        out.extend(fuzzy_search(db, lang, &needle, limit).await?);
    }
    Ok(out)
}

/// The fuzzy fallback body: one damerau word-match scan per table, all six
/// concurrent (see `fuzzy_word_pass` for the match rule). Runs only when
/// the regular passes found nothing.
async fn fuzzy_search(
    db: &Db,
    lang: &str,
    needle: &str,
    limit: i64,
) -> Result<Vec<(String, Card)>> {
    let parts = tokio::join!(
        fuzzy_word_pass(db, lang, "cookie", ", grade", "abilities", needle, limit),
        fuzzy_word_pass(db, lang, "pet", ", grade", "description", needle, limit),
        fuzzy_word_pass(
            db,
            lang,
            "treasure",
            ", grade",
            "description",
            needle,
            limit
        ),
        fuzzy_word_pass(db, lang, "relic", "", "description", needle, limit),
        fuzzy_word_pass(db, lang, "episode", "", "description", needle, limit),
        fuzzy_word_pass(
            db,
            lang,
            "ingredient",
            ", grade",
            "description",
            needle,
            limit
        ),
    );
    let mut out = Vec::new();
    // cross-slice dedup: the slices run concurrently, so ids cannot repeat
    // within a table, but a pet may share a cookie's numeric id — the set
    // is keyed by section, exactly like the main passes'
    for (section, rows) in [
        ("cookies", parts.0?),
        ("pets", parts.1?),
        ("treasures", parts.2?),
        ("relics", parts.3?),
        ("episodes", parts.4?),
        ("ingredients", parts.5?),
    ] {
        for r in rows {
            let card = Card::from(r);
            if !out
                .iter()
                .any(|(s, c): &(String, Card)| *s == section && c.id == card.id)
            {
                out.push((section.to_string(), card));
            }
        }
    }
    Ok(out)
}

/// One table's slice of `search`: the passes in order, hits deduped by
/// id within the table (the passes can return the same row; ids are only
/// unique per table, so cross-table dedup would wrongly drop a pet that
/// shares a cookie's numeric id).
#[allow(clippy::too_many_arguments)]
async fn search_table(
    db: &Db,
    lang: &str,
    section: &str,
    table: &str,
    extra: &str,
    prose: &str,
    query: &str,
    needle: &str,
    is_thai: bool,
    limit: i64,
) -> Result<Vec<(String, Card)>> {
    let mut out: Vec<(String, Card)> = Vec::new();
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for r in fts_pass(db, lang, table, extra, prose, query, limit).await? {
        let card = Card::from(r);
        if seen.insert(card.id) {
            out.push((section.to_string(), card));
        }
    }

    // the name-gram pass: the needle's 3-grams against the index, most
    // overlapping entities first. The subquery is materialized through
    // LET (an inline `id IN (subquery)` replans per scanned row —
    // EXPLAIN: TableScan, 13 s on the seeded catalog; materialized,
    // ~30 ms). Ranked rows travel through LET and the hydration read
    // orders by `rank` (array::index_of over the id list), so the
    // overlap order survives without a second round trip.
    let grams = name_grams(needle);
    if !grams.is_empty() {
        let rows: Vec<CardRow> = db
            .query(format!(
                "LET $rows = (SELECT entity_id, count() AS hits FROM name_gram
                         WHERE section = $sec AND gram IN $grams
                         GROUP BY entity_id ORDER BY hits DESC LIMIT {limit});
                 LET $ids = array::map($rows, |$r| $r.entity_id);
                 SELECT record::id(id) AS id, image{extra}, {} AS name,
                        (tr.en.name ?? '') AS en_name,
                        array::index_of($ids, record::id(id)) AS rank
                   FROM {table}
                  WHERE record::id(id) IN $ids
                  ORDER BY rank",
                tr("name")
            ))
            .bind(("sec", table.to_string()))
            .bind(("grams", grams.clone()))
            .await?
            // statement 0 and 1 are the LETs, statement 2 the select
            .take(2)?;
        for r in rows {
            let card = Card::from(r);
            if seen.insert(card.id) {
                out.push((section.to_string(), card));
            }
        }
    }

    // the prose pass: substring over description (and abilities), for
    // needles the name grams cannot see. Thai needles search the th
    // columns (the en text cannot contain them), latin needles search
    // en plus the viewer's locale. The scan is the expensive one and its
    // hits only fill the tail, so it is skipped once the ranked passes
    // already produced a full page.
    if out.len() < usize::try_from(limit).unwrap_or(usize::MAX) {
        let columns: Vec<&str> = if is_thai {
            vec!["th"]
        } else {
            vec!["en", lang]
        };
        // at most one contains clause per prose column and locale; the vec
        // is two or three entries, no capacity hint needed
        let mut contains = Vec::new();
        for locale in columns {
            contains.push(format!(
                "string::lowercase(tr.{locale}.description ?? '') CONTAINS $frag"
            ));
            if prose == "abilities" {
                contains.push(format!(
                    "string::lowercase(tr.{locale}.abilities ?? '') CONTAINS $frag"
                ));
            }
        }
        let sql = format!(
            "SELECT record::id(id) AS id, image{extra}, {} AS name,
                    (tr.en.name ?? '') AS en_name
               FROM {table}
              WHERE {}
              ORDER BY name ASC
              LIMIT {limit}",
            tr("name"),
            contains.join(" OR "),
        );
        let rows: Vec<CardRow> = db
            .query(sql)
            .bind(("lang", lang.to_string()))
            .bind(("frag", needle.to_string()))
            .await?
            .take(0)?;
        for r in rows {
            let card = Card::from(r);
            if seen.insert(card.id) {
                out.push((section.to_string(), card));
            }
        }
    }

    // the fuzzy pass lives in `search` (its gate is the whole search's
    // emptiness, unknowable per table — see `fuzzy_search`).
    Ok(out)
}

/// The full-text pass: whole-word hits (`@1@`) ranked by `search::score`,
/// one query per locale pair — same tool the V app's FTS5 tables gave it.
#[allow(clippy::too_many_arguments)]
async fn fts_pass(
    db: &Db,
    lang: &str,
    table: &str,
    extra: &str,
    prose: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<CardRow>> {
    let mut clauses = Vec::with_capacity(if prose == "abilities" { 6 } else { 4 });
    for locale in ["en", lang] {
        clauses.push(format!("tr.{locale}.name @1@ $q"));
        clauses.push(format!("tr.{locale}.description @1@ $q"));
        if prose == "abilities" {
            clauses.push(format!("tr.{locale}.abilities @1@ $q"));
        }
    }
    let sql = format!(
        "SELECT record::id(id) AS id, image{extra}, {} AS name,
                (tr.en.name ?? '') AS en_name, search::score(1) AS score
           FROM {table}
          WHERE {}
          ORDER BY score DESC, name ASC
          LIMIT {limit}",
        tr("name"),
        clauses.join(" OR "),
    );
    db.query(sql)
        .bind(("lang", lang.to_string()))
        .bind(("q", query.to_string()))
        .await?
        .take(0)
}

/// One table's rows whose name/description/abilities contain a word within
/// damerau-levenshtein 1 of the needle — the last-resort typo pass. `sruf`
/// vs "surfing" prose: no catalog name contains `urf` at all, so no gram
/// overlap and no exact substring can see it. A word matches as the whole
/// word OR as its needle-length prefix (so `sruf` reaches `surfing` via the
/// `surf` prefix — one transposition; whole-word distance ignores length,
/// which is why the prefix clause only fires on words longer than the
/// needle). Damerau, not jaro-winkler: at the 0.75 threshold jaro also
/// admits `syrup`/`sprout` for `sruf`; damerau ≤1 admits exactly the
/// surf-class words (whole-word `surf` = 1) plus genuinely-one-edit
/// neighbors like `stuff` (prefix `stuf`, t↔r) — the price of typo
/// tolerance. Scans name + description + abilities in the locales the
/// substring pass uses; multi-word needles have no single comparable word
/// and never reach here.
#[allow(clippy::too_many_arguments)]
async fn fuzzy_word_pass(
    db: &Db,
    lang: &str,
    table: &str,
    extra: &str,
    prose: &str,
    needle: &str,
    limit: i64,
) -> Result<Vec<CardRow>> {
    const WORD_MATCH: &str = "array::any(string::words(string::lowercase({col} ?? '')), |$w|
                        string::len($w) >= 3 AND (
                            string::distance::damerau_levenshtein($w, $frag) <= 1 OR
                            (string::len($w) > string::len($frag) AND
                             string::distance::damerau_levenshtein(
                                 string::slice($w, 0, string::len($frag)), $frag) <= 1)))";
    let mut clauses = Vec::new();
    // the locale list stays deduped: for lang=en, ["en", lang] would emit
    // every column twice and double the scan cost — the common case
    let locales: Vec<&str> = if lang == "en" {
        vec!["en"]
    } else {
        vec!["en", lang]
    };
    for locale in locales {
        clauses.push(WORD_MATCH.replace("{col}", &format!("tr.{locale}.name")));
        clauses.push(WORD_MATCH.replace("{col}", &format!("tr.{locale}.description")));
        if prose == "abilities" {
            clauses.push(WORD_MATCH.replace("{col}", &format!("tr.{locale}.abilities")));
        }
    }
    let sql = format!(
        "SELECT record::id(id) AS id, image{extra}, {} AS name,
                (tr.en.name ?? '') AS en_name
           FROM {table}
          WHERE {}
          ORDER BY name ASC
          LIMIT {limit}",
        tr("name"),
        clauses.join(" OR "),
    );
    db.query(sql)
        .bind(("lang", lang.to_string()))
        .bind(("frag", needle.to_string()))
        .await?
        .take(0)
}

/// The needle's name-gram vocabulary: distinct three-*character* slices of
/// the lowercased needle (char boundaries, not bytes — Thai chars are 3
/// bytes each); a needle shorter than three characters is its own gram so
/// one- and two-character queries still find names containing them. `windows`
/// yields the same slices without index arithmetic over the char count.
fn name_grams(needle: &str) -> Vec<String> {
    let chars: Vec<char> = needle.chars().collect();
    if chars.len() < 3 {
        return vec![needle.to_string()];
    }
    chars.windows(3).map(|w| w.iter().collect()).collect()
}
