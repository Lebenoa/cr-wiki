//! The news surface's data half: the stored post, the projections the public
//! pages and the admin authoring page read, and the writes behind them.
//!
//! A post is bilingual the way every catalog record is: its text lives NESTED
//! inside the record as `tr: { en: {title, summary, body}, th: {...} }`, so a
//! read is a locale-fallback path read (`tr[$lang].title ?? tr.en.title`) and
//! editing Thai can never wipe the English text.
//!
//! Drafts are a `published` flag rather than a separate table: the public
//! reads filter on it, the admin authoring page does not. `published_at` is
//! the unix second a release TAKES EFFECT (0 while it has never been released),
//! so a date in the future queues the post rather than publishing it: the pair
//! of columns is what `PostState` derives draft/queued/live from, and every
//! public read filters `published_at <= now`. A queued post therefore goes
//! live on its own, with no second write and no scheduler to run — the only
//! thing "now" decides is which reads can see it, which is why the clock is a
//! parameter rather than a call to the wall clock buried in the query.
//! The public list orders by `published_at`, the admin list by id.

use std::collections::BTreeMap;

use surrealdb::types::SurrealValue;

use crate::db::{Db, Result};
use crate::time::now_unix;

/// One locale's text for a post. Empty strings mean "not translated yet", so
/// the locale fallback on read fills in the English text.
#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
pub struct PostText {
    pub title: String,
    pub summary: String,
    pub body: String,
}

/// One card in the public list and the admin dashboard.
#[derive(Debug, Clone, Default)]
pub struct NewsCard {
    pub id: i64,
    pub title: String,
    pub summary: String,
    pub published: bool,
    pub published_at: i64,
    pub updated_at: i64,
    pub author: String,
    /// Whether this post should occupy the homepage's featured hero. At most
    /// one post may carry it; the writes below enforce the exclusivity.
    pub featured: bool,
}

/// What a post is doing at a given moment. The two stored control fields
/// answer different questions — `published` is the author's decision,
/// `published_at` is when it takes effect — so the third state is derived
/// rather than stored: it depends on the clock, and the clock moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostState {
    /// Not released: invisible to visitors however it is dated.
    Draft,
    /// Released with a date still in the future: invisible until then.
    Queued,
    /// Released and past its date: the only state a visitor can read.
    Live,
}

impl PostState {
    /// Whether a visitor can read the post in this state — the one rule the
    /// public reads and the dashboard's actions both follow.
    pub const fn is_public(self) -> bool {
        matches!(self, Self::Live)
    }
}

impl NewsCard {
    /// The `YYYY-MM-DD` a card shows: when the post takes effect, or the last
    /// edit for a draft. A queued post therefore shows the day it appears, not
    /// the day it was written. Empty when there is neither.
    ///
    /// `tz_offset` (minutes east of UTC) is the site's own zone, so the day
    /// shown is the day the author picked: a post scheduled for local midnight
    /// is stored as the afternoon before in UTC, and reading that instant back
    /// in UTC would date every card a day early west of Greenwich.
    pub fn date_label(&self, tz_offset: i64) -> String {
        if self.published && self.published_at > 0 {
            crate::time::format_local_ymd(self.published_at, tz_offset)
        } else {
            crate::time::format_local_ymd(self.updated_at, tz_offset)
        }
    }

    /// The state at `now`. A released post with no date (0) counts as live: a
    /// missing moment must never hide a post forever, and the form stamps one
    /// on the first release anyway.
    pub fn state(&self, now: i64) -> PostState {
        if !self.published {
            PostState::Draft
        } else if self.published_at > now {
            PostState::Queued
        } else {
            PostState::Live
        }
    }

    /// Whether the post's own page exists yet; the admin dashboard offers a
    /// View link only for these, since a draft or queued page 404s.
    pub fn is_live(&self, now: i64) -> bool {
        self.state(now).is_public()
    }
}

/// One post as the feed carries it: the card's fields plus the article body,
/// because a subscriber reads the whole post rather than the teaser, and the
/// last edit beside the publication date, because the feed's `Last-Modified`
/// has to move when a post's text does — a corrected typo is a changed
/// document, however old the post.
#[derive(Debug, Clone, Default)]
pub struct NewsFeedItem {
    pub id: i64,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub published_at: i64,
    pub updated_at: i64,
    pub author: String,
}

impl NewsFeedItem {
    /// The newest moment this post's presence in the feed reflects.
    pub fn changed_at(&self) -> i64 {
        self.published_at.max(self.updated_at)
    }
}

/// One post as the public detail page renders it.
#[derive(Debug, Clone, Default)]
pub struct NewsPost {
    pub id: i64,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub published_at: i64,
    pub author: String,
}

impl NewsPost {
    pub fn date_label(&self, tz_offset: i64) -> String {
        crate::time::format_local_ymd(self.published_at, tz_offset)
    }
}

/// One post as the authoring form pre-fills it: the control fields plus the
/// text of the locale being edited (no English fallback — the form must show
/// the locale's own text or the first save would overwrite it).
#[derive(Debug, Clone, Default)]
pub struct NewsEdit {
    pub id: i64,
    pub published: bool,
    pub published_at: i64,
    pub featured: bool,
    pub text: PostText,
}

impl NewsEdit {
    /// Preserve the stored instant in the site's local date-time input.
    pub fn published_input(&self, tz_offset: i64) -> String {
        crate::time::format_local_datetime(self.published_at, tz_offset)
    }
}

/// The values one save writes. The route resolves the publication date before
/// building this, so the write path has no clock of its own to disagree with.
#[derive(Debug, Clone)]
pub struct NewsDraft {
    pub title: String,
    pub summary: String,
    pub body: String,
    pub published: bool,
    pub published_at: i64,
    pub featured: bool,
}

/// What every read projects, one plan for the list, the dashboard and the
/// detail page so their column names cannot drift.
const PROJECTION: &str = "SELECT record::id(id) AS id,\n\
        (tr[$lang].title ?? tr.en.title ?? '') AS title,\n\
        (tr[$lang].summary ?? tr.en.summary ?? '') AS summary,\n\
        published, published_at, updated_at, author, featured\n";

#[derive(Debug, Clone, Default, SurrealValue)]
#[surreal(default)]
struct CardRow {
    id: i64,
    title: String,
    summary: String,
    published: bool,
    published_at: i64,
    updated_at: i64,
    author: String,
    featured: bool,
}

impl From<CardRow> for NewsCard {
    fn from(r: CardRow) -> Self {
        Self {
            id: r.id,
            title: r.title,
            summary: r.summary,
            published: r.published,
            published_at: r.published_at,
            updated_at: r.updated_at,
            author: r.author,
            featured: r.featured,
        }
    }
}

/// The posts a visitor may read at `now`: released, and past their moment. A
/// released post dated in the future is queued, so this is the one filter that
/// both hides a scheduled post and reveals it later, unchanged.
///
/// The clock is a parameter, not a call: the app has one clock
/// (`crate::time::now_unix`) and the routes pass it in, which is what makes
/// "hidden until then" a testable rule instead of a function of when the test
/// happens to run. Newest first, with the id tie-break keeping paging from
/// repeating a row when two posts share a date.
pub async fn select_published(
    db: &Db,
    lang: &str,
    now: i64,
    limit: i64,
    offset: i64,
) -> Result<Vec<NewsCard>> {
    select_published_filtered(db, lang, now, limit, offset, true).await
}

/// [`select_published`] with control over whether a featured post appears.
/// The homepage teaser hides it (the hero already shows it, so listing it
/// again reads as a duplicate), while the archive list, the feed and the
/// sitemap include it — a visitor scanning /news must still find the one
/// post on a quiet day.
pub async fn select_published_filtered(
    db: &Db,
    lang: &str,
    now: i64,
    limit: i64,
    offset: i64,
    include_featured: bool,
) -> Result<Vec<NewsCard>> {
    let sql = format!(
        "{PROJECTION}  FROM news\n WHERE published = true AND published_at <= $now AND (featured != true OR $featured)\n ORDER BY published_at DESC, id DESC\n LIMIT {limit} START {offset}"
    );
    let rows: Vec<CardRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .bind(("now", now))
        .bind(("featured", include_featured))
        .await?
        .take(0)?;
    Ok(rows.into_iter().map(NewsCard::from).collect())
}

/// Every post — drafts included — newest created first, for the admin
/// dashboard. Bounded in the query so a runaway table cannot answer with
/// everything at once.
pub async fn select_all(db: &Db, lang: &str) -> Result<Vec<NewsCard>> {
    let sql = format!("{PROJECTION}  FROM news ORDER BY id DESC LIMIT 200");
    let rows: Vec<CardRow> = db
        .query(&sql)
        .bind(("lang", lang.to_string()))
        .await?
        .take(0)?;
    Ok(rows.into_iter().map(NewsCard::from).collect())
}

/// The one featured post a visitor may read at `now`, for the homepage's
/// featured hero. The write path keeps at most one `featured` row, but a
/// queued or newly flagged post must not surface before its moment, so the
/// same public filter applies; the newest flagged post wins when history
/// ever left two behind.
pub async fn select_featured(db: &Db, lang: &str, now: i64) -> Result<Option<NewsPost>> {
    #[derive(Debug, Clone, Default, SurrealValue)]
    #[surreal(default)]
    struct PostRow {
        id: i64,
        title: String,
        summary: String,
        body: String,
        published_at: i64,
        author: String,
    }
    let rows: Vec<PostRow> = db
        .query(
            "SELECT record::id(id) AS id,\n\
             (tr[$lang].title ?? tr.en.title ?? '') AS title,\n\
             (tr[$lang].summary ?? tr.en.summary ?? '') AS summary,\n\
             (tr[$lang].body ?? tr.en.body ?? '') AS body,\n\
             published_at, author\n\
             FROM news WHERE featured = true AND published = true \
             AND published_at <= $now\n\
             ORDER BY published_at DESC, id DESC LIMIT 1",
        )
        .bind(("lang", lang.to_string()))
        .bind(("now", now))
        .await?
        .take(0)?;
    Ok(rows.into_iter().next().map(|r| NewsPost {
        id: r.id,
        title: r.title,
        summary: r.summary,
        body: r.body,
        published_at: r.published_at,
        author: r.author,
    }))
}

/// The posts a visitor may read at `now`, bodies included, for the feed: the
/// same filter and order as [`select_published`], with the article every entry
/// carries. A projection of its own rather than the card one plus a second
/// read, because the feed is the only surface that wants every article's full
/// text — the list, the teaser and the dashboard must not drag it over the
/// wire to throw it away.
pub async fn select_feed(db: &Db, lang: &str, now: i64, limit: i64) -> Result<Vec<NewsFeedItem>> {
    #[derive(Debug, Clone, Default, SurrealValue)]
    #[surreal(default)]
    struct FeedRow {
        id: i64,
        title: String,
        summary: String,
        body: String,
        published_at: i64,
        updated_at: i64,
        author: String,
    }
    let rows: Vec<FeedRow> = db
        .query(format!(
            "SELECT record::id(id) AS id,\n\
             (tr[$lang].title ?? tr.en.title ?? '') AS title,\n\
             (tr[$lang].summary ?? tr.en.summary ?? '') AS summary,\n\
             (tr[$lang].body ?? tr.en.body ?? '') AS body,\n\
             published_at, updated_at, author\n\
             FROM news WHERE published = true AND published_at <= $now\n\
             ORDER BY published_at DESC, id DESC\n\
             LIMIT {limit}"
        ))
        .bind(("lang", lang.to_string()))
        .bind(("now", now))
        .await?
        .take(0)?;
    Ok(rows
        .into_iter()
        .map(|r| NewsFeedItem {
            id: r.id,
            title: r.title,
            summary: r.summary,
            body: r.body,
            published_at: r.published_at,
            updated_at: r.updated_at,
            author: r.author,
        })
        .collect())
}

/// One post by id that a visitor may read at `now`, for the public detail page.
/// A draft, and a queued post that has not come due, read as absent — only the
/// authoring form reaches those, so a scheduled post's url is not a leak.
pub async fn select_post(db: &Db, lang: &str, now: i64, id: i64) -> Result<Option<NewsPost>> {
    #[derive(Debug, Clone, Default, SurrealValue)]
    #[surreal(default)]
    struct PostRow {
        id: i64,
        title: String,
        summary: String,
        body: String,
        published_at: i64,
        author: String,
    }
    let rows: Vec<PostRow> = db
        .query(
            "SELECT record::id(id) AS id,\n\
             (tr[$lang].title ?? tr.en.title ?? '') AS title,\n\
             (tr[$lang].summary ?? tr.en.summary ?? '') AS summary,\n\
             (tr[$lang].body ?? tr.en.body ?? '') AS body,\n\
             published_at, author\n\
             FROM news WHERE record::id(id) = $id AND published = true \
             AND published_at <= $now",
        )
        .bind(("lang", lang.to_string()))
        .bind(("id", id))
        .bind(("now", now))
        .await?
        .take(0)?;
    Ok(rows.into_iter().next().map(|r| NewsPost {
        id: r.id,
        title: r.title,
        summary: r.summary,
        body: r.body,
        published_at: r.published_at,
        author: r.author,
    }))
}

/// One post for the authoring form, whether published or a draft, with the
/// edited locale's own (un-fallback) text.
pub async fn select_edit(db: &Db, lang: &str, id: i64) -> Result<Option<NewsEdit>> {
    #[derive(Debug, Clone, Default, SurrealValue)]
    #[surreal(default)]
    struct EditRow {
        id: i64,
        published: bool,
        published_at: i64,
        featured: bool,
        // `Option` because a record that was created without text — or whose
        // first save failed before the nested write — carries `tr: NONE`,
        // which a bare map refuses to decode
        tr: Option<BTreeMap<String, PostText>>,
    }
    let rows: Vec<EditRow> = db
        .query(
            "SELECT record::id(id) AS id, published, published_at, featured, tr FROM news \
             WHERE record::id(id) = $id",
        )
        .bind(("id", id))
        .await?
        .take(0)?;
    Ok(rows.into_iter().next().map(|r| {
        let text =
            r.tr.unwrap_or_default()
                .get(lang)
                .cloned()
                .unwrap_or_default();
        NewsEdit {
            id: r.id,
            published: r.published,
            published_at: r.published_at,
            featured: r.featured,
            text,
        }
    }))
}

/// The newest publication date among the posts live at `now`, or `None` when
/// there are none: the timestamp half of the sitemap's conditional GET.
///
/// `published_at` alone, not the feed's edit-aware stamp, because that is all
/// the sitemap can be dated by: its urls are ids, so editing a post's text
/// changes nothing there, while publishing one adds a url. A catalog row has
/// no timestamp at all, which is why a crawler that revalidates on the date
/// rather than the tag can be behind on a newly created entity until the next
/// post goes live; the tag is what sees that immediately.
pub async fn newest_publication(db: &Db, now: i64) -> Result<Option<i64>> {
    #[derive(Debug, Clone, Default, SurrealValue)]
    #[surreal(default)]
    struct StampRow {
        published_at: i64,
    }
    let rows: Vec<StampRow> = db
        .query(
            "SELECT published_at FROM news \
             WHERE published = true AND published_at <= $now \
             ORDER BY published_at DESC LIMIT 1",
        )
        .bind(("now", now))
        .await?
        .take(0)?;
    Ok(rows
        .into_iter()
        .next()
        .map(|r| r.published_at)
        .filter(|t| *t > 0))
}

/// The ids of every post live at `now`, for the sitemap: a queued post's url
/// must not be advertised before its page exists.
pub async fn published_ids(db: &Db, now: i64) -> Result<Vec<i64>> {
    #[derive(Debug, Clone, Default, SurrealValue)]
    #[surreal(default)]
    struct IdRow {
        id: i64,
    }
    let rows: Vec<IdRow> = db
        .query(
            "SELECT record::id(id) AS id FROM news \
             WHERE published = true AND published_at <= $now LIMIT 1000",
        )
        .bind(("now", now))
        .await?
        .take(0)?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

/// Creates a post and its text for `lang`, returning the new id. The id comes
/// from the table's sequence (see `db::ensure_news_schema`), allocated and
/// consumed in one message.
pub async fn insert_post(
    db: &Db,
    lang: &str,
    draft: &NewsDraft,
    author: &str,
    user_id: i64,
    now: i64,
) -> Result<i64> {
    let mut res = db
        .query(
            "LET $id = sequence::nextval('news_seq'); \
             CREATE type::record('news', $id) SET \
                published = $published, published_at = $published_at, \
                created_at = $now, updated_at = $now, \
                author = $author, user_id = $uid, featured = $featured; \
             RETURN $id;",
        )
        .bind(("published", draft.published))
        .bind(("published_at", draft.published_at))
        .bind(("now", now))
        .bind(("author", author.to_string()))
        .bind(("uid", user_id))
        .bind(("featured", draft.featured))
        .await?
        .check()?;
    let id = res
        .take::<Option<i64>>(2)?
        .ok_or_else(|| surrealdb::Error::internal("news insert returned no id".to_string()))?;
    if draft.featured {
        // "live" is the row's own state against the same clock the reads
        // use: a queued post must never demote the live hero before its
        // moment, and a fixed test date must not silently become "past"
        // because the wall clock moved.
        let live = draft.published && draft.published_at <= now;
        clear_other_featured(db, id, live).await?;
    }
    write_locale(db, id, lang, draft).await?;
    Ok(id)
}

/// Enforces "at most one featured post": strips the flag from every other
/// row that still carries it. Called after the write that (re)points the
/// flag, so a crash between the two leaves at worst two featured rows —
/// and the reads above still answer with the newest one.
///
/// A queued post can also carry the flag (an author schedules the next
/// featured piece), but it must not demote the live one before its moment:
/// when the incoming row is still queued, no live holder loses the flag, so
/// the homepage hero never flickers empty between a queued post's creation
/// and its reveal. The queued flag takes over on time by itself — the reads
/// order by `published_at DESC`, and the queued post is the newer one.
async fn clear_other_featured(db: &Db, keep: i64, keep_live: bool) -> Result<()> {
    if !keep_live {
        return Ok(());
    }
    db.query(
        "UPDATE news SET featured = false \
         WHERE featured = true AND record::id(id) != $keep \
         AND published = true AND published_at <= $now",
    )
    .bind(("keep", keep))
    .bind(("now", now_unix()))
    .await?
    .check()?;
    Ok(())
}

/// Updates a post's control fields and its `lang` text.
pub async fn update_post(
    db: &Db,
    lang: &str,
    id: i64,
    draft: &NewsDraft,
    now: i64,
) -> Result<bool> {
    db.query(
        "UPDATE type::record('news', $id) SET \
            published = $published, published_at = $published_at, updated_at = $now, \
            featured = $featured",
    )
    .bind(("id", id))
    .bind(("published", draft.published))
    .bind(("published_at", draft.published_at))
    .bind(("now", now))
    .bind(("featured", draft.featured))
    .await?
    .check()?;
    if draft.featured {
        let live = draft.published && draft.published_at <= now;
        clear_other_featured(db, id, live).await?;
    }
    write_locale(db, id, lang, draft).await?;
    Ok(true)
}

/// Flips one post's published state, stamping the first publication date when
/// it had none. The old date is read in the same message, so a draft going
/// live gets `now` exactly once.
pub async fn set_published(db: &Db, id: i64, published: bool) -> Result<()> {
    db.query(
        "LET $old = (SELECT published_at FROM type::record('news', $id))[0].published_at ?? 0; \
         UPDATE type::record('news', $id) SET \
            published = $p, \
            published_at = IF $p AND $old = 0 THEN $now ELSE $old END, \
            updated_at = $now;",
    )
    .bind(("id", id))
    .bind(("p", published))
    .bind(("now", now_unix()))
    .await?
    .check()?;
    Ok(())
}

/// Removes a post and its text.
pub async fn delete_post(db: &Db, id: i64) -> Result<()> {
    db.query("DELETE type::record('news', $id)")
        .bind(("id", id))
        .await?
        .check()?;
    Ok(())
}

/// Upserts one language's text INSIDE the nested tr object, mirroring the
/// catalog editor: the other locales are read back and written unchanged.
async fn write_locale(db: &Db, id: i64, lang: &str, draft: &NewsDraft) -> Result<()> {
    // `Option` is load-bearing, not tidiness: a `SELECT tr` on a record that
    // has no text yet returns an explicit `tr: NONE`, and a bare map field
    // then fails to decode ("Expected object, got none") — which made the
    // very first save of a new row fail. The option decodes that as None and
    // the write proceeds from an empty map.
    #[derive(Default, SurrealValue)]
    #[surreal(default)]
    struct TrWrap {
        tr: Option<BTreeMap<String, PostText>>,
    }
    let mut wrap: TrWrap = db
        .query("SELECT tr FROM type::record('news', $id)")
        .bind(("id", id))
        .await?
        .take::<Vec<TrWrap>>(0)?
        .pop()
        .unwrap_or_default();
    let mut map = wrap.tr.take().unwrap_or_default();
    map.insert(
        lang.to_string(),
        PostText {
            title: draft.title.trim().to_string(),
            summary: draft.summary.trim().to_string(),
            body: draft.body.clone(),
        },
    );
    db.query("UPDATE type::record('news', $id) SET tr = $tr")
        .bind(("id", id))
        .bind(("tr", map))
        .await?
        .check()?;
    Ok(())
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;
    use surrealdb::types::{Object, Value};

    /// The clock the round trip reads through: the publication date it writes,
    /// so a read at `NOW` is live and a read a second earlier is not. Every
    /// public read is a comparison against this, never against the wall clock,
    /// which is what stops the suite from depending on the day it runs.
    const NOW: i64 = 1_792_108_800; // 2026-10-16
    /// Ten days later, for the queued post: the same rows are read at three
    /// clocks and only the clock changes. `JUST_BEFORE` is the second before
    /// it, so the boundary is pinned from both sides without arithmetic inside
    /// an assertion.
    const SOON: i64 = 1_792_972_800; // 2026-10-26
    const JUST_BEFORE: i64 = 1_792_972_799;

    /// A full authoring round trip against a live server, double-gated: the
    /// `CR_SURREAL_URL` a reader must set for any DB test, plus
    /// `CR_NEWS_WRITE_TEST=1` to consent to writes. It always connects to its
    /// own namespace and database, so it can never touch the catalog the URL
    /// actually names; without both variables `cargo test` skips it.
    ///
    /// This is the one DB test that writes, because the news writes are the
    /// part a unit test cannot reach: the nested `tr` merge, the sequence,
    /// and the first-publish date stamp all live in `SurrealQL`.
    #[tokio::test]
    async fn authoring_round_trip_in_a_scratch_namespace() {
        let Some(db) = scratch_db("roundtrip").await else {
            return;
        };
        // whoever ran before left rows behind; the ids need not be 1
        db.query("DELETE news;").await.unwrap().check().unwrap();

        let body = "Line one\n\n# Heading\n\n- alpha\n- beta";
        let draft = NewsDraft {
            title: "Hello".into(),
            summary: "short".into(),
            body: body.into(),
            published: false,
            published_at: 0,
            featured: false,
        };
        let id = insert_post(&db, "en", &draft, "tester", 7, NOW)
            .await
            .expect("insert");
        assert!(id > 0, "the sequence hands out a real id");

        // a draft is invisible to every public read
        assert!(select_published(&db, "en", NOW, 10, 0)
            .await
            .unwrap()
            .is_empty());
        assert!(published_ids(&db, NOW).await.unwrap().is_empty());
        let all = select_all(&db, "en").await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "Hello");
        assert!(!all[0].published);
        assert_eq!(all[0].author, "tester");

        // a Thai edit must not disturb the English text, nor add an English
        // row when the locale is read back for editing
        let edit = NewsDraft {
            title: "สวัสดี".into(),
            summary: "สรุป".into(),
            body: "เนื้อหา".into(),
            published: true,
            published_at: 1_792_108_800,
            featured: false,
        };
        update_post(&db, "th", id, &edit, NOW).await.expect("update th");
        let en = select_edit(&db, "en", id).await.unwrap().expect("exists");
        assert_eq!(en.text.title, "Hello", "English survives a Thai edit");
        let th = select_edit(&db, "th", id).await.unwrap().expect("exists");
        assert_eq!(th.text.title, "สวัสดี");
        // a locale with no text of its own is empty for editing (no fallback)
        let fr = select_edit(&db, "fr", id).await.unwrap().expect("exists");
        assert!(fr.text.title.is_empty());

        // publishing reveals it, and the public read falls back to English
        set_published(&db, id, true).await.unwrap();
        let public = select_published(&db, "en", NOW, 10, 0).await.unwrap();
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].title, "Hello");
        assert_eq!(published_ids(&db, NOW).await.unwrap(), vec![id]);
        let posted = select_post(&db, "th", NOW, id)
            .await
            .unwrap()
            .expect("published");
        assert_eq!(posted.title, "สวัสดี");
        assert_eq!(posted.date_label(0), "2026-10-16");
        assert_eq!(
            select_post(&db, "fr", NOW, id)
                .await
                .unwrap()
                .expect("fallback")
                .title,
            "Hello",
            "an untranslated locale reads the English text"
        );

        // the feed's read carries the article itself, not a teaser — the one
        // projection that fetches every body
        let feed = select_feed(&db, "en", NOW, 50).await.unwrap();
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].id, id);
        assert_eq!(feed[0].body, body);
        assert_eq!(feed[0].author, "tester");
        // the edit stamp is load-bearing for the feed's `Last-Modified`, and a
        // projection that stopped selecting it would default to 0 in silence
        assert!(feed[0].updated_at > 1_700_000_000, "{:?}", feed[0]);
        assert_eq!(feed[0].changed_at(), feed[0].updated_at.max(1_792_108_800));
        // and the sitemap's date is the newest publication
        assert_eq!(
            newest_publication(&db, NOW).await.unwrap(),
            Some(1_792_108_800)
        );

        // the stored body renders through the real article path
        let mut memo = crate::richtext::LinkCache::new();
        let html = crate::richtext::render_article_with(&db, "en", &draft.body, &mut memo).await;
        assert!(html.contains("<h2 class="), "{html}");
        assert!(html.contains("<ul class="), "{html}");
        assert!(html.contains("<li>alpha</li>"), "{html}");

        // a first publish with no date is stamped now, exactly once
        let second = NewsDraft {
            title: "Second".into(),
            published: false,
            published_at: 0,
            ..draft.clone()
        };
        let id2 = insert_post(&db, "en", &second, "tester", 0, NOW).await.unwrap();
        set_published(&db, id2, true).await.unwrap();
        let p2 = select_post(&db, "en", now_unix(), id2)
            .await
            .unwrap()
            .expect("published");
        assert!(p2.published_at > 1_700_000_000, "stamped now");
        // and unpublishing keeps that date rather than clearing it
        set_published(&db, id2, false).await.unwrap();
        let back = select_edit(&db, "en", id2).await.unwrap().expect("exists");
        assert!(back.published_at > 1_700_000_000);
        assert!(!back.published);

        delete_post(&db, id).await.unwrap();
        delete_post(&db, id2).await.unwrap();
        assert!(select_post(&db, "en", NOW, id).await.unwrap().is_none());
        assert!(select_all(&db, "en").await.unwrap().is_empty());
    }

    /// The three states derive from the two control fields, and the boundary
    /// second is already live — a queued post must not need a write to become
    /// readable. A released post with no date counts as live rather than hidden
    /// forever, and an unreleased one never goes live however it is dated.
    #[test]
    fn post_state_follows_the_flag_and_the_clock() {
        let live = NewsCard {
            published: true,
            published_at: NOW,
            ..Default::default()
        };
        assert_eq!(live.state(NOW), PostState::Live);
        assert!(live.is_live(NOW));

        let queued = NewsCard {
            published: true,
            published_at: SOON,
            ..Default::default()
        };
        assert_eq!(queued.state(NOW), PostState::Queued);
        assert_eq!(queued.state(JUST_BEFORE), PostState::Queued);
        assert_eq!(queued.state(SOON), PostState::Live);
        assert!(!queued.is_live(JUST_BEFORE));
        assert!(queued.is_live(SOON));

        let undated = NewsCard {
            published: true,
            published_at: 0,
            ..Default::default()
        };
        assert_eq!(undated.state(NOW), PostState::Live);

        let draft = NewsCard {
            published: false,
            published_at: SOON,
            ..Default::default()
        };
        assert_eq!(draft.state(NOW), PostState::Draft);
        assert!(!draft.is_live(SOON), "an unreleased post never goes live");
    }

    /// Queuing is a comparison rather than a stored column, so it gets its own
    /// test: the same row is read at three clocks and only the clock changes.
    /// Nothing here writes a second time — a queued post becomes readable
    /// because its moment arrived, which is the whole point of deriving the
    /// state instead of storing it.
    #[tokio::test]
    async fn a_scheduled_post_is_hidden_until_its_moment() {
        let Some(db) = scratch_db("scheduled").await else {
            return;
        };
        db.query("DELETE news;").await.unwrap().check().unwrap();
        let queued = NewsDraft {
            title: "Queued".into(),
            summary: "later".into(),
            body: String::new(),
            published: true,
            published_at: SOON,
            featured: false,
        };
        let id = insert_post(&db, "en", &queued, "tester", 0, NOW).await.unwrap();

        // before its moment: absent from every public read, the sitemap
        // included, so its url is not a leak
        assert!(select_published(&db, "en", NOW, 10, 0)
            .await
            .unwrap()
            .is_empty());
        assert!(select_feed(&db, "en", NOW, 50).await.unwrap().is_empty());
        assert!(select_post(&db, "en", NOW, id).await.unwrap().is_none());
        assert!(published_ids(&db, NOW).await.unwrap().is_empty());
        assert_eq!(
            newest_publication(&db, NOW).await.unwrap(),
            None,
            "a post that has not come due cannot date the sitemap"
        );

        // the author still sees it, marked queued and dated for the day it
        // appears rather than the day it was written
        let dashboard = select_all(&db, "en").await.unwrap();
        let card = dashboard.iter().find(|c| c.id == id).expect("listed");
        assert_eq!(card.state(NOW), PostState::Queued);
        assert_eq!(card.date_label(0), "2026-10-26");

        // the second before is still queued; the date itself is live, which is
        // the inclusive boundary every public read shares
        assert!(select_post(&db, "en", JUST_BEFORE, id)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            select_post(&db, "en", SOON, id)
                .await
                .unwrap()
                .expect("due")
                .title,
            "Queued"
        );
        assert_eq!(
            select_published(&db, "en", SOON, 10, 0)
                .await
                .unwrap()
                .len(),
            1
        );
        let due = select_feed(&db, "en", SOON, 50).await.unwrap();
        assert_eq!(due.len(), 1, "the feed reveals it with everything else");
        assert_eq!(due[0].title, "Queued");
        assert_eq!(published_ids(&db, SOON).await.unwrap(), vec![id]);
        assert_eq!(
            newest_publication(&db, SOON).await.unwrap(),
            Some(SOON),
            "the date moves with the url the sitemap now carries"
        );

        // publishing a draft with no date is still immediate: the clock only
        // ever hides a post whose author dated it ahead
        set_published(&db, id, false).await.unwrap();
        assert!(select_post(&db, "en", SOON, id).await.unwrap().is_none());
        let back = select_edit(&db, "en", id).await.unwrap().expect("exists");
        assert!(!back.published, "unpublishing keeps the queued date");
        assert_eq!(back.published_at, SOON);

        delete_post(&db, id).await.unwrap();
    }

    /// The same opt-in, for the catalog editor: creating a brand-new entity
    /// writes its nested `tr` for the first time, the exact path that used to
    /// fail because `SELECT tr` answers `tr: NONE` on a row that has none.
    #[tokio::test]
    async fn admin_entity_create_writes_its_translation() {
        let Some(db) = scratch_db("catalog").await else {
            return;
        };
        // the seed creates this table on a real deployment; the search-event
        // ingest a catalog CREATE fires would otherwise fail on it
        db.query("DEFINE TABLE IF NOT EXISTS name_gram SCHEMALESS;")
            .await
            .unwrap()
            .check()
            .unwrap();
        let form = crate::routes::admin::EntityForm {
            name: "Scratch Cookie".into(),
            description: "desc".into(),
            grade: Some(1),
            release_date: "2026-10-16".into(),
            ..Default::default()
        };
        let id = crate::db::insert_entity(&db, "en", crate::section::Section::Cookies, &form)
            .await
            .expect("insert entity");
        assert!(id > 0);
        let found = crate::db::select_detail_strict(&db, "en", "cookies", id)
            .await
            .expect("read back");
        assert_eq!(
            found.expect("created row reads back").name,
            "Scratch Cookie"
        );
        db.query("DELETE type::record('cookie', $id)")
            .bind(("id", id))
            .await
            .unwrap()
            .check()
            .unwrap();
    }

    /// The opt-in scratch connection. Skips (with a notice) when either the
    /// server is unnamed or the write consent is absent, and gives each suite
    /// its own namespace and database: these tests write, `cargo test` runs
    /// them in parallel, and one shared `news` table would mean one suite
    /// deleting another's rows.
    async fn scratch_db(suite: &str) -> Option<Db> {
        let url = std::env::var("CR_SURREAL_URL").ok()?;
        if std::env::var("CR_NEWS_WRITE_TEST").ok().as_deref() != Some("1") {
            eprintln!("skip: set CR_NEWS_WRITE_TEST=1 to run the news write test");
            return None;
        }
        let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
        let ns = format!("cookierun_news_{suite}");
        match crate::db::connect_url(&url, &ns, &ns, &user, &pass).await {
            Ok(db) => Some(db),
            Err(e) => {
                eprintln!("skip: CR_SURREAL_URL is set but unusable ({e})");
                None
            }
        }
    }

    /// The card projection decodes the way `take` decodes it — the same
    /// silent-default failure mode the catalog rows guard against, locked
    /// down without a database.
    #[test]
    fn card_row_reads_the_news_projection() {
        let mut o = Object::new();
        o.insert("id", 12i64);
        o.insert("title", "Launch");
        o.insert("summary", "s");
        o.insert("published", true);
        o.insert("published_at", 1_792_108_800i64);
        o.insert("updated_at", 1_792_108_800i64);
        o.insert("author", "GingerBrave");
        let row = CardRow::from_value(Value::Object(o)).expect("decodes");
        let card = NewsCard::from(row);
        assert_eq!(card.id, 12);
        assert_eq!(card.title, "Launch");
        assert!(card.published);
        assert_eq!(card.date_label(0), "2026-10-16");
    }

    /// A draft with no date falls back to its last edit, and never shows the
    /// epoch.
    #[test]
    fn draft_cards_date_from_the_last_edit() {
        let draft = NewsCard {
            published: false,
            published_at: 0,
            updated_at: 1_792_108_800,
            ..Default::default()
        };
        assert_eq!(draft.date_label(0), "2026-10-16");
        let blank = NewsCard::default();
        assert_eq!(blank.date_label(0), "");
    }

    /// A publication date the site's own zone decides: a post scheduled at the
    /// start of 2026-10-20 in `Asia/Bangkok` is stored as the instant that day
    /// begins there, every surface reads that same day back, and the author's
    /// form pre-fills the day they picked rather than the UTC day it fell on.
    ///
    /// The whole point of the change is in the third assertion: no surface may
    /// show a date the author did not choose, in either direction.
    #[test]
    fn publication_days_read_in_the_sites_own_zone() {
        // the instant 2026-10-16 begins in Asia/Bangkok, which is still the
        // evening of the 15th in UTC — the case a UTC read gets wrong
        const LOCAL_DAY_START: i64 = 1_792_108_800 - 7 * 3600;
        let card = NewsCard {
            published: true,
            published_at: LOCAL_DAY_START,
            ..Default::default()
        };
        assert_eq!(card.date_label(0), "2026-10-15", "UTC is a day behind here");
        assert_eq!(card.date_label(420), "2026-10-16");
        assert_eq!(card.date_label(-480), "2026-10-15");

        let edit = NewsEdit {
            id: 1,
            published: true,
            published_at: LOCAL_DAY_START,
            featured: false,
            text: PostText::default(),
        };
        assert_eq!(edit.published_input(420), "2026-10-16T00:00:00");
        assert_eq!(edit.published_input(0), "2026-10-15T17:00:00");
    }
}
