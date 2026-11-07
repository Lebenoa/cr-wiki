//! The news surface: the public list and its infinite-scroll fragment, one
//! post's page, the index teaser and the admin authoring pages.
//!
//! The authoring pages live under `/admin/news` and every one of them gates on
//! `ctx.is_admin()`, answering 404 rather than 403 in the same spirit as the
//! catalog editor: whether an admin page exists is not worth confirming to a
//! stranger. The editor ships with the app rather than living only in
//! `catalog`'s per-section forms, because news is not a catalog section: it is
//! a single table of dated, bilingual posts with a draft state.
//!
//! The public read (`/news/{id}`) is server-rendered; only the index teaser
//! and the list's next page are lazy htmx fragments.

use askama::Template;
use axum::extract::{Path, Query};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Form;
use axum::Router;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db;
use crate::news::{self, NewsCard, NewsDraft, NewsEdit, NewsFeedItem};
use crate::time::now_unix;

use super::errors::{self, AppError};
use super::CommonQuery;

/// Posts the index teaser shows.
const TEASER_LIMIT: i64 = 3;

/// Releases the index teaser shows above the posts: enough to fill the
/// Fresh column's grid (3 sections x 4 candidates merged down) without
/// turning the homepage into a second catalog page.
const LATEST_RELEASES: usize = 6;

/// Posts the feed carries. A subscriber wants the recent window, not the
/// archive: a feed that grows without bound is one every reader re-downloads
/// whole, and the oldest entries are the ones nobody is catching up on. It is
/// also what bounds the work one request does, now that every entry carries a
/// rendered article rather than a line of teaser.
const FEED_LIMIT: i64 = 50;

/// The news table. Merged before the catalog captures: `/news` and
/// `/news/{id}` would otherwise be read as a section detail page, and
/// `/admin/news` as a detail page for section "admin".
pub fn routes() -> Router {
    Router::new()
        .route("/news", get(list))
        .route("/news/latest", get(latest))
        .route("/news/featured", get(featured))
        // a static segment, like /news/latest: it is matched before the id
        // capture, so the feed is never read as a post id
        .route("/news/feed.xml", get(feed))
        .route("/news/{id}", get(show))
        .route("/admin/news", get(dashboard))
        .route("/admin/news/new", get(new_form).post(create))
        .route("/admin/news/{id}/edit", get(edit_form).post(update))
        .route("/admin/news/{id}/delete", post(delete))
        .route("/admin/news/{id}/publish", post(set_published))
}

#[derive(Template)]
#[template(path = "news.html")]
struct NewsPage {
    ctx: Ctx,
    posts: Vec<NewsCard>,
    next_url: String,
    page: i64,
}

/// The list body the infinite-scroll sentinel swaps in, and the fragment the
/// index teaser loads through `/news/latest`.
#[derive(Template)]
#[template(path = "news_cards.html")]
struct NewsCards {
    ctx: Ctx,
    posts: Vec<NewsCard>,
    next_url: String,
    page: i64,
}

#[derive(Template)]
#[template(path = "news_latest.html")]
struct NewsLatest {
    ctx: Ctx,
    posts: Vec<NewsCard>,
    /// the newest catalog releases (cookies, pets, treasures), shown above
    /// the posts — the homepage's "fresh" strip is releases first, editorials
    /// second
    releases: Vec<db::SectionCard>,
}

/// The featured hero the homepage's right column swaps in. `featured` is the
/// one flagged post, or None when no live post carries the flag — the
/// template then renders the evergreen fallback (the same markup the static
/// homepage includes before htmx answers).
#[derive(Template)]
#[template(path = "index_featured_static.html")]
struct NewsFeatured {
    ctx: Ctx,
    featured: Option<news::NewsPost>,
}

#[derive(Template)]
#[template(path = "news_detail.html")]
struct NewsDetailPage {
    ctx: Ctx,
    post: news::NewsPost,
    /// the body already rendered through the article markup
    body_html: String,
}

impl NewsDetailPage {
    fn description(&self) -> String {
        if self.post.summary.trim().is_empty() {
            self.ctx.page_desc("news_page_description")
        } else {
            self.post.summary.clone()
        }
    }

    fn article_json(&self) -> String {
        let mut value = serde_json::json!({
            "@context": "https://schema.org",
            "@type": "Article",
            "headline": self.post.title,
            "datePublished": format!("{}Z", crate::time::format_local_datetime(self.post.published_at, 0)),
            "mainEntityOfPage": self.ctx.canonical_url(),
        });
        if !self.post.author.is_empty() {
            value["author"] = serde_json::json!({ "@type": "Person", "name": self.post.author });
        }
        value.to_string().replace('<', "\\u003c")
    }
}

/// The authoring dashboard. It lists everything — drafts, queued and live —
/// and carries the clock, because "queued" is a comparison against now rather
/// than a stored column.
#[derive(Template)]
#[template(path = "news_admin.html")]
struct NewsAdminPage {
    ctx: Ctx,
    posts: Vec<NewsCard>,
    now: i64,
}

impl NewsAdminPage {
    /// Whether a card's own page exists yet, which is what the dashboard's
    /// View link asks about (a draft's and a queued post's page answer 404).
    /// A forwarder because askama hands a template field to a method by
    /// reference, while the rule itself stays on the card, taking the clock
    /// by value with the rest of the model.
    fn is_live(&self, card: &NewsCard) -> bool {
        card.is_live(self.now)
    }
}

/// The authoring form, flattened to plain fields so the template never has to
/// match an `Option` for each input; `post_id` is 0 for a new post.
#[derive(Template)]
#[template(path = "news_form.html")]
struct NewsFormPage {
    ctx: Ctx,
    post_id: i64,
    title: String,
    summary: String,
    body: String,
    published: bool,
    featured: bool,
    published_date: String,
    edit_lang: String,
    error: String,
    title_key: String,
    save_key: String,
}

impl NewsFormPage {
    fn is_edit(&self) -> bool {
        self.post_id > 0
    }

    fn timezone_label(&self) -> String {
        let offset = self.ctx.tz_offset;
        let minutes = offset.unsigned_abs();
        format!(
            "UTC{}{:02}:{:02}",
            if offset < 0 { '-' } else { '+' },
            minutes / 60,
            minutes % 60
        )
    }
}

/// The authoring submission. The checkbox is absent when unchecked, so
/// `published` is `Option<String>` and presence is the truth.
#[derive(Debug, Default, Deserialize)]
pub struct NewsForm {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub body: String,
    /// the locale this submission writes; empty falls back to the viewer's
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub published: Option<String>,
    /// Site-local YYYY-MM-DDTHH:MM[:SS]; empty means "now" on first
    /// publish, or "keep" on an edit.
    #[serde(default)]
    pub published_date: String,
    #[serde(default)]
    pub featured: Option<String>,
}

impl NewsForm {
    fn is_published(&self) -> bool {
        self.published.as_deref() == Some("true")
    }

    fn is_featured(&self) -> bool {
        self.featured.as_deref() == Some("true")
    }
}

/// The publish/unpublish toggle's body.
#[derive(Debug, Default, Deserialize)]
pub struct PublishForm {
    #[serde(default)]
    pub published: Option<String>,
}

/// The locale a submission writes: the form's own `lang` when it is a loaded
/// locale, else the viewer's language. Validated so an unknown value cannot
/// create a rogue `tr.xx`.
fn form_lang(form: &NewsForm, ctx: &Ctx) -> String {
    if crate::i18n::is_available(&form.lang) {
        form.lang.clone()
    } else {
        ctx.lang.clone()
    }
}

/// The locale the edit form shows: `?lang=` when it is a loaded locale, else
/// the viewer's resolved language.
fn edit_lang(q: &Query<CommonQuery>, ctx: &Ctx) -> String {
    q.lang
        .as_deref()
        .filter(|l| crate::i18n::is_available(l))
        .unwrap_or(&ctx.lang)
        .to_string()
}

/// An explicit site-local date and time wins; otherwise keep an existing
/// instant, stamp a first publication now, or leave an unpublished draft unset.
fn resolve_published_at(form: &NewsForm, existing: i64, tz_offset: i64) -> i64 {
    if let Some(ts) = crate::time::parse_local_datetime(&form.published_date, tz_offset) {
        return ts;
    }
    if existing > 0 {
        existing
    } else if form.is_published() {
        now_unix()
    } else {
        0
    }
}

fn draft_from(form: &NewsForm, existing: i64, tz_offset: i64) -> NewsDraft {
    NewsDraft {
        title: form.title.trim().to_string(),
        summary: form.summary.trim().to_string(),
        body: form.body.clone(),
        published: form.is_published(),
        published_at: resolve_published_at(form, existing, tz_offset),
        featured: form.is_featured(),
    }
}

/// The public list. Page 1 renders the whole document; a later page — and the
/// infinite-scroll sentinel — gets the cards fragment alone.
pub async fn list(ctx: Ctx, q: Query<CommonQuery>) -> Result<Html<String>, AppError> {
    let state = crate::state::state();
    let page = crate::pagination::clamp_page(q.page);
    let offset = page
        .saturating_sub(1)
        .saturating_mul(crate::section::PAGE_SIZE);
    let posts = news::select_published(
        &state.db,
        &ctx.lang,
        now_unix(),
        crate::section::PAGE_SIZE,
        offset,
    )
    .await?;
    let next_url = if i64::try_from(posts.len()).unwrap_or(0) == crate::section::PAGE_SIZE {
        format!("/news?page={}", page.saturating_add(1))
    } else {
        String::new()
    };
    let html = if ctx.is_fragment() {
        NewsCards {
            ctx,
            posts,
            next_url,
            page,
        }
        .render()
    } else {
        NewsPage {
            ctx,
            posts,
            next_url,
            page,
        }
        .render()
    };
    Ok(Html(
        html.unwrap_or_else(|e| format!("template error: {e}")),
    ))
}

/// The index teaser fragment: the newest catalog releases and the newest few
/// published posts. Fetched by htmx with `hx-trigger="load"` so the landing
/// page itself stays free of database work; a direct (non-htmx) visit is sent
/// to the list.
pub async fn latest(ctx: Ctx) -> Result<Response, AppError> {
    if !ctx.is_fragment() {
        return Ok(Redirect::to("/news").into_response());
    }
    let state = crate::state::state();
    // the hero already shows the featured post; the teaser lists the rest
    let posts =
        news::select_published_filtered(&state.db, &ctx.lang, now_unix(), TEASER_LIMIT, 0, false)
            .await?;
    let releases = db::latest_releases(&state.db, &ctx.lang, LATEST_RELEASES, 4).await?;
    Ok(Html(
        NewsLatest {
            ctx,
            posts,
            releases,
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}

/// The homepage's featured hero, fetched by htmx with `hx-trigger="load"`.
/// A direct (non-htmx) visit redirects to the post itself — the hero is a
/// fragment, not a page.
pub async fn featured(ctx: Ctx) -> Result<Response, AppError> {
    if !ctx.is_fragment() {
        let state = crate::state::state();
        return match news::select_featured(&state.db, &ctx.lang, now_unix()).await? {
            Some(post) => Ok(Redirect::to(&format!("/news/{}", post.id)).into_response()),
            None => Ok(Redirect::to("/news").into_response()),
        };
    }
    let state = crate::state::state();
    let featured = news::select_featured(&state.db, &ctx.lang, now_unix()).await?;
    Ok(Html(
        NewsFeatured { ctx, featured }
            .render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}

/// The RSS feed of live posts in the reader's locale: absolute links, since a
/// feed reader has no page to resolve a relative one against, and the same
/// `?lang=` suffix the site's own alternates carry, so a reader who subscribed
/// in Thai keeps getting Thai posts.
///
/// Each entry's description is the post's whole article, rendered through the
/// same markup path the detail page uses, so a subscriber reads the post in
/// their reader instead of clicking through. What that costs is one article
/// render per entry, with a single link cache shared across the document and a
/// document-level cache window on the response.
///
/// That cache is why the two validators are here: a reader polling every ten
/// minutes re-downloads the whole feed for nothing when nothing was published,
/// and the tag — a digest of these very bytes — answers `304` instead. The
/// date is the newest moment the document reflects, so a reader that compares
/// only `If-Modified-Since` (no tag) still revalidates rather than re-fetches.
///
/// Deliberately strict about database failures, unlike the sitemap: an empty
/// feed answers 200 and quietly tells a reader every post was withdrawn, while
/// a 500 lets it keep what it already holds and come back later.
pub async fn feed(ctx: Ctx, headers: HeaderMap) -> Result<Response, AppError> {
    let state = crate::state::state();
    let posts = news::select_feed(&state.db, &ctx.lang, now_unix(), FEED_LIMIT).await?;
    // bound to locals: the channel borrows them, so they must outlive it
    let title = ctx.l.t("news_feed_title");
    let description = ctx.page_desc("news_page_description");
    let self_url = ctx.news_feed_url();
    let site_link = ctx.url_for("/news", &ctx.lang);
    let channel = crate::feed::Channel {
        title: &title,
        link: &site_link,
        description: &description,
        language: &ctx.lang,
        self_url: &self_url,
        site: &ctx.site_url,
        now: now_unix(),
    };
    let items = feed_items(&ctx, &state.db, &posts).await;
    let body = crate::feed::rss(&channel, &items);
    // the news stamp moves on every write to the table, which is what dates the
    // changes a post list cannot show: a post withdrawn, or one older than the
    // window edited so that a subscriber's “nothing new” answer is only ever a
    // missed re-fetch, never a missed change. Lenient about the failure — the
    // content dates below still date the document, and a reader that sends a
    // tag gets the exact answer either way.
    let stamp = crate::db::change_stamp(&state.db, &crate::db::FEED_SOURCES)
        .await
        .unwrap_or(None);
    Ok(crate::conditional::document(
        &headers,
        "application/rss+xml; charset=utf-8",
        body,
        stamp.max(feed_last_modified(&posts)),
    ))
}

/// The newest moment the feed reflects: the latest publication among the posts
/// it carries, or the latest edit of one of them — a corrected body is a
/// changed document even though the post keeps its date — and `None` for a
/// feed with no posts at all, which is a document with nothing to date it by
/// (its tag still answers a reader that sends one).
///
/// This half of the feed's date is the one that survives a database nothing has
/// been written to since the stamps were defined; the stamp is the half that
/// can see a post that is simply gone.
fn feed_last_modified(posts: &[NewsFeedItem]) -> Option<i64> {
    posts
        .iter()
        .map(NewsFeedItem::changed_at)
        .filter(|t| *t > 0)
        .max()
}

/// The feed's entries, in the order the query returned them — newest first.
/// Each body goes through the real article renderer, so what a subscriber reads
/// is what the post's own page shows, and one link cache serves the whole
/// document: an entity named in three posts is looked up once, not three times.
async fn feed_items(
    ctx: &Ctx,
    db: &crate::db::Db,
    posts: &[NewsFeedItem],
) -> Vec<crate::feed::Item> {
    let mut memo = crate::richtext::LinkCache::new();
    let mut items = Vec::with_capacity(posts.len());
    for post in posts {
        let rendered =
            crate::richtext::render_article_with(db, &ctx.lang, &post.body, &mut memo).await;
        items.push(feed_entry(ctx, post, &rendered));
    }
    items
}

/// One entry, its article already rendered. Split from the loop above so the
/// mapping — absolute localized permalinks, and a description that is the
/// article when there is one and the author's summary when there is not — is
/// testable without a database or a router.
fn feed_entry(ctx: &Ctx, post: &NewsFeedItem, rendered: &str) -> crate::feed::Item {
    crate::feed::Item {
        title: post.title.clone(),
        link: ctx.url_for(&format!("/news/{}", post.id), &ctx.lang),
        // an empty body renders to nothing, and a post with no article still
        // carries the summary its author wrote rather than an invented line
        description: if rendered.trim().is_empty() {
            post.summary.clone()
        } else {
            rendered.to_string()
        },
        // only live posts are read here, so this is always the first
        // publication date rather than a draft's last edit
        published: post.published_at,
        author: post.author.clone(),
    }
}

pub async fn show(ctx: Ctx, Path(id): Path<i64>) -> Result<Response, AppError> {
    let state = crate::state::state();
    let found = news::select_post(&state.db, &ctx.lang, now_unix(), id).await?;
    let Some(post) = found else {
        return Ok(errors::not_found(ctx));
    };
    let mut memo = crate::richtext::LinkCache::new();
    let body_html =
        crate::richtext::render_article_with(&state.db, &ctx.lang, &post.body, &mut memo).await;
    Ok(Html(
        NewsDetailPage {
            ctx,
            post,
            body_html,
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}

pub async fn dashboard(ctx: Ctx) -> Result<Response, AppError> {
    if !ctx.is_admin() {
        return Ok(errors::not_found(ctx));
    }
    let state = crate::state::state();
    let posts = news::select_all(&state.db, &ctx.lang).await?;
    Ok(Html(
        NewsAdminPage {
            ctx,
            posts,
            now: now_unix(),
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response())
}

pub async fn new_form(ctx: Ctx, q: Query<CommonQuery>) -> Response {
    if !ctx.is_admin() {
        return errors::not_found(ctx);
    }
    let lang = edit_lang(&q, &ctx);
    news_form(ctx, None, lang, String::new())
}

pub async fn edit_form(ctx: Ctx, q: Query<CommonQuery>, Path(id): Path<i64>) -> Response {
    if !ctx.is_admin() {
        return errors::not_found(ctx);
    }
    let state = crate::state::state();
    let lang = edit_lang(&q, &ctx);
    let found = match news::select_edit(&state.db, &lang, id).await {
        Ok(found) => found,
        Err(e) => {
            tracing::error!("news edit load {id}: {e}");
            None
        }
    };
    let Some(post) = found else {
        return errors::not_found(ctx);
    };
    news_form(ctx, Some(&post), lang, String::new())
}

/// Renders the authoring form from either a loaded post or a blank one.
fn news_form(ctx: Ctx, post: Option<&NewsEdit>, edit_lang: String, error: String) -> Response {
    let (post_id, title, summary, body, published, featured, published_date) = match &post {
        Some(p) => (
            p.id,
            p.text.title.clone(),
            p.text.summary.clone(),
            p.text.body.clone(),
            p.published,
            p.featured,
            p.published_input(ctx.tz_offset),
        ),
        None => (
            0,
            String::new(),
            String::new(),
            String::new(),
            false,
            false,
            crate::time::format_local_datetime(now_unix(), ctx.tz_offset),
        ),
    };
    let (title_key, save_key) = if post_id > 0 {
        ("news_edit_page_title", "news_save_button")
    } else {
        ("news_new_page_title", "news_create_button")
    };
    Html(
        NewsFormPage {
            ctx,
            post_id,
            title,
            summary,
            body,
            published,
            featured,
            published_date,
            edit_lang,
            error,
            title_key: title_key.to_string(),
            save_key: save_key.to_string(),
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response()
}

pub async fn create(ctx: Ctx, Form(form): Form<NewsForm>) -> Result<Response, AppError> {
    if !ctx.is_admin() {
        return Ok(errors::not_found(ctx));
    }
    let state = crate::state::state();
    let lang = form_lang(&form, &ctx);
    let author = ctx.username();
    let user_id = ctx.user.as_ref().map_or(0, |u| u.id);
    if form.title.trim().is_empty() {
        let response = news_form(ctx, None, lang, "news_error_title".to_string());
        return Ok((axum::http::StatusCode::BAD_REQUEST, response).into_response());
    }
    let draft = draft_from(&form, 0, ctx.tz_offset);
    match news::insert_post(&state.db, &lang, &draft, &author, user_id, now_unix()).await {
        Ok(id) if id > 0 => Ok(Redirect::to("/admin/news").into_response()),
        Ok(_) => Ok(server_error_form(ctx, 0, lang, &form, "news_error_save")),
        Err(e) => {
            tracing::error!("news insert failed: {e}");
            Ok(server_error_form(ctx, 0, lang, &form, "news_error_save"))
        }
    }
}

pub async fn update(
    ctx: Ctx,
    Path(id): Path<i64>,
    Form(form): Form<NewsForm>,
) -> Result<Response, AppError> {
    if !ctx.is_admin() {
        return Ok(errors::not_found(ctx));
    }
    let state = crate::state::state();
    let lang = form_lang(&form, &ctx);
    let existing = news::select_edit(&state.db, &lang, id).await?;
    let Some(existing) = existing else {
        return Ok(errors::not_found(ctx));
    };
    if form.title.trim().is_empty() {
        return Ok((
            axum::http::StatusCode::BAD_REQUEST,
            news_form(ctx, Some(&existing), lang, "news_error_title".to_string()),
        )
            .into_response());
    }
    let draft = draft_from(&form, existing.published_at, ctx.tz_offset);
    match news::update_post(&state.db, &lang, id, &draft, now_unix()).await {
        Ok(_) => Ok(Redirect::to("/admin/news").into_response()),
        Err(e) => {
            tracing::error!("news update {id} failed: {e}");
            Ok(server_error_form(ctx, id, lang, &form, "news_error_save"))
        }
    }
}

/// Re-renders the authoring form at `post_id` (0 for a new post) with the
/// submitted values and an error, rather than dropping the admin's writing on
/// a failed save.
fn server_error_form(
    ctx: Ctx,
    post_id: i64,
    lang: String,
    form: &NewsForm,
    error: &str,
) -> Response {
    let edit = NewsEdit {
        id: post_id,
        published: form.is_published(),
        // Reopen the submitted instant in the same site timezone.
        published_at: crate::time::parse_local_datetime(&form.published_date, ctx.tz_offset)
            .unwrap_or(0),
        featured: form.is_featured(),
        text: news::PostText {
            title: form.title.clone(),
            summary: form.summary.clone(),
            body: form.body.clone(),
        },
    };
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        news_form(ctx, Some(&edit), lang, error.to_string()),
    )
        .into_response()
}

pub async fn set_published(
    ctx: Ctx,
    Path(id): Path<i64>,
    Form(form): Form<PublishForm>,
) -> Response {
    if !ctx.is_admin() {
        return errors::not_found(ctx);
    }
    let state = crate::state::state();
    let published = form.published.as_deref() == Some("true");
    if let Err(e) = news::set_published(&state.db, id, published).await {
        tracing::error!("news publish toggle {id}: {e}");
    }
    Redirect::to("/admin/news").into_response()
}

pub async fn delete(ctx: Ctx, Path(id): Path<i64>) -> Response {
    if !ctx.is_admin() {
        return errors::not_found(ctx);
    }
    let state = crate::state::state();
    if let Err(e) = news::delete_post(&state.db, id).await {
        tracing::error!("news delete {id}: {e}");
    }
    Redirect::to("/admin/news").into_response()
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    fn form(title: &str, published: bool, date: &str) -> NewsForm {
        NewsForm {
            title: title.to_string(),
            summary: "s".into(),
            body: "b".into(),
            lang: "en".into(),
            published: published.then(|| "true".to_string()),
            published_date: date.to_string(),
            featured: None,
        }
    }

    /// An explicit date wins; a first publish with no date is stamped now; a
    /// draft that never had one stays at 0; an edit keeps the stored date.
    #[test]
    fn publication_date_resolution() {
        assert_eq!(
            resolve_published_at(&form("t", true, "2026-10-16T00:00"), 0, 0),
            1_792_108_800
        );
        assert_eq!(resolve_published_at(&form("t", false, ""), 0, 0), 0);
        let now = resolve_published_at(&form("t", true, ""), 0, 0);
        assert!(now > 1_700_000_000, "a first publish is stamped now");
        assert_eq!(
            resolve_published_at(&form("t", true, ""), 1_792_108_800, 0),
            1_792_108_800
        );
    }

    #[test]
    fn scheduled_time_round_trips_and_goes_live_at_the_exact_instant() {
        for (offset, expected) in [
            (420, 1_792_435_440),
            (-480, 1_792_489_440),
            (0, 1_792_460_640),
        ] {
            let chosen = form("t", true, "2026-10-20T01:44");
            let instant = resolve_published_at(&chosen, 0, offset);
            assert_eq!(instant, expected);
            let edit = NewsEdit {
                id: 7,
                published: true,
                published_at: instant,
                featured: false,
                text: news::PostText::default(),
            };
            assert_eq!(edit.published_input(offset), "2026-10-20T01:44:00");
            assert_eq!(
                resolve_published_at(
                    &form("t", true, &edit.published_input(offset)),
                    instant,
                    offset
                ),
                instant
            );
            let card = NewsCard {
                published: true,
                published_at: instant,
                ..Default::default()
            };
            assert_eq!(card.state(instant - 1), news::PostState::Queued);
            assert_eq!(card.state(instant), news::PostState::Live);
        }
    }

    /// A submission writes the locale it names, and an unknown one falls back
    /// to the viewer's rather than creating a rogue locale.
    #[test]
    fn submitted_locale_is_validated() {
        crate::i18n::load("translations");
        let ctx = crate::testutil::test_ctx("en");
        let mut f = form("t", false, "");
        f.lang = "th".into();
        assert_eq!(form_lang(&f, &ctx), "th");
        f.lang = "zz".into();
        assert_eq!(form_lang(&f, &ctx), "en");
    }

    /// The detail template escapes an author-controlled title (askama escapes
    /// interpolations) and trusts only the pre-rendered body.
    #[test]
    fn detail_template_escapes_the_title() {
        let html = NewsDetailPage {
            ctx: crate::testutil::test_ctx("en"),
            post: crate::news::NewsPost {
                id: 1,
                title: "<script>alert(1)</script>".into(),
                author: "a".into(),
                ..Default::default()
            },
            body_html: "<p class=\"mt-4 leading-relaxed text-foreground\">ok</p>".into(),
        }
        .render()
        .expect("renders");
        assert!(!html.contains("<script>alert(1)</script>"), "{html}");
        // askama escapes the angle brackets rather than emitting the tag
        assert!(html.contains("&#60;script&#62;"), "{html}");
        assert!(
            html.contains("<p class=\"mt-4"),
            "the rendered body is trusted"
        );
        // every page the base template frames advertises the feed, in the
        // locale being read
        assert!(
            html.contains("rel=\"alternate\" type=\"application/rss+xml\"")
                && html.contains("href=\"http://localhost:6785/news/feed.xml\""),
            "the head links the feed: {html}"
        );
    }

    fn card(id: i64, title: &str, published: bool) -> NewsCard {
        NewsCard {
            id,
            title: title.to_string(),
            summary: "S".into(),
            published,
            published_at: 1_792_108_800,
            updated_at: 1_792_108_800,
            author: "writer".into(),
            featured: false,
        }
    }

    /// The list, the fragment, the teaser and the dashboard each render —
    /// including the empty state — so a template change cannot ship a page
    /// that only fails at request time.
    #[test]
    fn list_teaser_and_dashboard_render() {
        crate::i18n::load("translations");
        let cards = NewsCards {
            ctx: crate::testutil::test_ctx("en"),
            posts: vec![card(3, "Launched", true)],
            next_url: "/news?page=2".into(),
            page: 1,
        }
        .render()
        .expect("cards render");
        assert!(cards.contains("href=\"/news/3\""), "{cards}");
        assert!(cards.contains("hx-get=\"/news?page=2\""), "{cards}");

        let page = NewsPage {
            ctx: crate::testutil::test_ctx("en"),
            posts: vec![card(3, "Launched", true)],
            next_url: String::new(),
            page: 1,
        }
        .render()
        .expect("page render");
        assert!(page.contains("id=\"news-list\""), "{page}");
        assert!(
            page.contains("href=\"/news/feed.xml\""),
            "the list offers the feed: {page}"
        );

        let empty = NewsCards {
            ctx: crate::testutil::test_ctx("en"),
            posts: Vec::new(),
            next_url: String::new(),
            page: 1,
        }
        .render()
        .expect("empty render");
        assert!(empty.contains("No news yet."), "{empty}");

        // the dashboard lists all three states and carries the clock, because
        // "queued" is a comparison rather than a stored column
        let now = 1_792_108_800;
        let queued = NewsCard {
            published_at: 1_792_972_800,
            ..card(11, "Later", true)
        };
        let dash = NewsAdminPage {
            ctx: crate::testutil::test_ctx("en"),
            posts: vec![card(9, "Draft", false), card(10, "Live", true), queued],
            now,
        }
        .render()
        .expect("dashboard render");
        assert!(dash.contains("/admin/news/9/publish"), "{dash}");
        assert!(dash.contains("/admin/news/9/delete"), "{dash}");
        assert!(dash.contains(">Draft<"), "{dash}");
        assert!(dash.contains(">Published<"), "{dash}");
        // a queued post reads as scheduled on the day it will appear, and the
        // view link is absent because its page does not exist yet
        assert!(dash.contains(">Scheduled<"), "{dash}");
        assert!(dash.contains("2026-10-26"), "{dash}");
        assert!(
            dash.contains("href=\"/news/10\""),
            "a live post keeps its link: {dash}"
        );
        assert!(!dash.contains("href=\"/news/11\""), "{dash}");
        assert!(!dash.contains("href=\"/news/9\""), "{dash}");

        let releases = vec![db::SectionCard {
            card: crate::db::Card {
                id: 94,
                name: "Mango Sticky Rice Cookie".into(),
                en_name: "Mango Sticky Rice Cookie".into(),
                image: Some("mango_sticky_rice_cookie.png".into()),
                grade: Some(4),
                is_evolved: false,
            },
            section: crate::section::Section::Cookies,
        }];
        let teaser = NewsLatest {
            ctx: crate::testutil::test_ctx("en"),
            posts: vec![card(1, "Fresh", true)],
            releases,
        }
        .render()
        .expect("teaser render");
        assert!(teaser.contains("View all news"), "{teaser}");
        assert!(
            teaser.contains("href=\"/cookies/94\""),
            "the release card links through its section: {teaser}"
        );
        assert!(
            teaser.contains("/static/img/cookies/mango_sticky_rice_cookie.png"),
            "the release card's image sits under the section dir: {teaser}"
        );
    }

    /// The feed's central claim, end to end against a real server: an entry's
    /// description is the post's whole article, rendered by the same renderer
    /// the detail page uses, and a post that has not come due yet is not in the
    /// feed at all. Double-gated like the other writing test, and pointed at a
    /// namespace of its own so it cannot touch anyone's data.
    #[tokio::test]
    async fn the_feed_carries_the_rendered_article() {
        let Some(db) = scratch_db("feed").await else {
            return;
        };
        db.query("DELETE news;").await.unwrap().check().unwrap();
        let now = 1_792_108_800; // 2026-10-16
        let draft = news::NewsDraft {
            title: "Launch & more".into(),
            summary: "a teaser".into(),
            body: "# Heading\n\nSee this.\n\n- alpha\n- beta".into(),
            published: true,
            published_at: now,
            featured: false,
        };
        let id = news::insert_post(&db, "en", &draft, "tester", 0, now)
            .await
            .expect("insert");
        // a second post released into the future: queued, so the feed is not
        // the one surface that leaks it
        let queued = news::NewsDraft {
            title: "Later".into(),
            published_at: now + 86_400,
            ..draft.clone()
        };
        let later = news::insert_post(&db, "en", &queued, "tester", 0, now)
            .await
            .expect("insert queued");

        let ctx = crate::testutil::test_ctx("en");
        let posts = news::select_feed(&db, "en", now, FEED_LIMIT).await.unwrap();
        assert_eq!(posts.len(), 1, "only the due post is in the feed");
        let items = feed_items(&ctx, &db, &posts).await;
        assert_eq!(items.len(), 1);
        let d = &items[0].description;
        assert!(
            d.contains("<h2 class=") && d.contains("<ul class=") && d.contains("<li>alpha</li>"),
            "the description is the rendered article, not the teaser: {d}"
        );
        assert!(
            !d.contains("a teaser"),
            "the summary is not the description: {d}"
        );
        assert_eq!(items[0].link, format!("http://localhost:6785/news/{id}"));
        // and it survives the XML layer as one entry an XML parser accepts
        let channel = crate::feed::Channel {
            title: "News",
            link: "http://localhost:6785/news",
            description: "d",
            language: "en",
            self_url: "http://localhost:6785/news/feed.xml",
            site: "http://localhost:6785",
            now,
        };
        let doc = crate::feed::rss(&channel, &items);
        assert_eq!(doc.matches("<item>").count(), 1, "{doc}");
        assert!(doc.contains("&lt;li&gt;alpha&lt;/li&gt;"), "{doc}");
        assert!(
            !doc.contains("<li>alpha</li>"),
            "the article stays escaped: {doc}"
        );

        news::delete_post(&db, id).await.unwrap();
        news::delete_post(&db, later).await.unwrap();
    }

    /// A subscriber that already holds the feed, end to end over real data:
    /// the handler's own steps, composed, with a live server behind them. The
    /// reader sends back what it was given — the tag, and the date alone — and
    /// is answered `304` with no body rather than the whole document again,
    /// which is the entire point of serving validators.
    #[tokio::test]
    async fn a_revalidating_reader_gets_a_304_instead_of_the_feed() {
        let Some(db) = scratch_db("revalidate").await else {
            return;
        };
        db.query("DELETE news;").await.unwrap().check().unwrap();
        let now = 1_792_108_800;
        let draft = news::NewsDraft {
            title: "Launch".into(),
            summary: "a teaser".into(),
            body: String::new(),
            published: true,
            published_at: now,
            featured: false,
        };
        let id = news::insert_post(&db, "en", &draft, "tester", 0, now)
            .await
            .expect("insert");

        // exactly what the handler composes: the post read, the rendered
        // entries, the document, and the date it reflects
        let ctx = crate::testutil::test_ctx("en");
        let posts = news::select_feed(&db, "en", now, FEED_LIMIT).await.unwrap();
        let items = feed_items(&ctx, &db, &posts).await;
        let channel = crate::feed::Channel {
            title: "News",
            link: "http://localhost:6785/news",
            description: "d",
            language: "en",
            self_url: "http://localhost:6785/news/feed.xml",
            site: "http://localhost:6785",
            now,
        };
        let document = |request: &HeaderMap| {
            crate::conditional::document(
                request,
                "application/rss+xml; charset=utf-8",
                crate::feed::rss(&channel, &items),
                feed_last_modified(&posts),
            )
        };

        let first = document(&HeaderMap::new());
        assert_eq!(first.status(), 200);
        let etag = first
            .headers()
            .get(axum::http::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .expect("a tag")
            .to_string();
        let modified = first
            .headers()
            .get(axum::http::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .expect("a date")
            .to_string();
        assert!(
            modified.ends_with("GMT"),
            "an HTTP-date, not the feed's own offset: {modified}"
        );
        let body = axum::body::to_bytes(first.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("a teaser"),
            "the entry is there"
        );

        let again = document(&headers(&[("if-none-match", &etag)]));
        assert_eq!(again.status(), 304, "the same bytes: nothing to download");
        let empty = axum::body::to_bytes(again.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(empty.is_empty());

        let dated = document(&headers(&[("if-modified-since", &modified)]));
        assert_eq!(dated.status(), 304, "a reader that keeps only the date");

        // and a reader holding an older copy still gets the document
        let stale = document(&headers(&[("if-none-match", "\"0-0000000000000000\"")]));
        assert_eq!(stale.status(), 200);

        news::delete_post(&db, id).await.unwrap();
    }

    /// The request headers for the composition above, built the way axum hands
    /// them in.
    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("name"),
                axum::http::HeaderValue::from_str(value).expect("value"),
            );
        }
        map
    }

    /// The opt-in scratch connection for the tests here that write, the same
    /// double gate `news`'s tests use — and a namespace per suite, because
    /// `cargo test` runs them in parallel and one shared `news` table would
    /// mean one suite deleting another's rows.
    async fn scratch_db(suite: &str) -> Option<crate::db::Db> {
        let url = std::env::var("CR_SURREAL_URL").ok()?;
        if std::env::var("CR_NEWS_WRITE_TEST").ok().as_deref() != Some("1") {
            eprintln!("skip: set CR_NEWS_WRITE_TEST=1 to run the feed write tests");
            return None;
        }
        let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
        let ns = format!("cookierun_feed_{suite}");
        match crate::db::connect_url(&url, &ns, &ns, &user, &pass).await {
            Ok(db) => Some(db),
            Err(e) => {
                eprintln!("skip: CR_SURREAL_URL is set but unusable ({e})");
                None
            }
        }
    }

    fn feed_post(id: i64, title: &str, body: &str) -> NewsFeedItem {
        NewsFeedItem {
            id,
            title: title.to_string(),
            summary: "S".into(),
            body: body.to_string(),
            published_at: 1_792_108_800,
            updated_at: 1_792_108_800,
            author: "writer".into(),
        }
    }

    /// The feed's entries carry absolute permalinks in the reader's own locale,
    /// and the description a subscriber reads is the rendered article itself —
    /// the whole post, not the teaser. A post with no article falls back to the
    /// summary its author wrote, and one with neither stays empty rather than
    /// becoming a placeholder.
    #[test]
    fn feed_entries_carry_the_article_in_the_reader_locale() {
        crate::i18n::load("translations");
        let en = crate::testutil::test_ctx("en");
        // what `render_article_with` hands back: markup, not pasted text
        let article = "<p class=\"mt-4 leading-relaxed text-foreground\">Launch day</p>";

        let entry = feed_entry(&en, &feed_post(3, "Launched", "body"), article);
        assert_eq!(entry.link, "http://localhost:6785/news/3");
        assert_eq!(entry.title, "Launched");
        assert_eq!(entry.author, "writer");
        assert_eq!(entry.published, 1_792_108_800);
        assert_eq!(entry.description, article, "the description is the article");

        let th = crate::testutil::test_ctx("th");
        let localized = feed_entry(&th, &feed_post(9, "ข่าว", "body"), article);
        assert_eq!(
            localized.link, "http://localhost:6785/news/9?lang=th",
            "a Thai reader's entry links to the Thai page"
        );

        // no article rendered: the author's summary stands in, and a post with
        // neither description is empty — which the feed emits no element for
        let summarized = NewsFeedItem {
            body: String::new(),
            ..feed_post(4, "Teaser only", "")
        };
        assert_eq!(feed_entry(&en, &summarized, "").description, "S");
        assert_eq!(
            feed_entry(&en, &summarized, "  \n").description,
            "S",
            "whitespace is not an article"
        );
        let bare = NewsFeedItem {
            summary: String::new(),
            author: String::new(),
            ..summarized
        };
        let empty = feed_entry(&en, &bare, "");
        assert!(empty.description.is_empty());
        assert!(empty.author.is_empty());
    }

    /// The feed's `Last-Modified` is the newest moment it reflects: a later
    /// publication moves it, editing an older post moves it too (a corrected
    /// body is a changed document, whatever the post's date), and a feed with
    /// nothing in it has no date to send at all rather than one it invented.
    #[test]
    fn the_feeds_date_follows_its_newest_change() {
        let older = NewsFeedItem {
            published_at: 1_700_000_000,
            updated_at: 1_700_000_000,
            ..feed_post(1, "Older", "b")
        };
        let newer = NewsFeedItem {
            published_at: 1_792_108_800,
            updated_at: 1_792_108_800,
            ..feed_post(2, "Newer", "b")
        };
        assert_eq!(
            feed_last_modified(&[newer.clone(), older.clone()]),
            Some(1_792_108_800),
            "the newest publication"
        );
        let edited = NewsFeedItem {
            updated_at: 1_800_000_000,
            ..older
        };
        assert_eq!(
            feed_last_modified(&[newer, edited]),
            Some(1_800_000_000),
            "an edit is a changed document"
        );
        assert_eq!(feed_last_modified(&[]), None, "an empty feed has no date");
        let undated = NewsFeedItem {
            published_at: 0,
            updated_at: 0,
            ..feed_post(3, "Undated", "b")
        };
        assert_eq!(
            feed_last_modified(&[undated]),
            None,
            "a zero date is no date"
        );
    }

    /// The authoring form posts to create or update depending on whether a
    /// post id came in, and the language select carries the edited locale.
    #[test]
    fn form_action_follows_the_post_id() {
        crate::i18n::load("translations");
        let blank = NewsFormPage {
            ctx: crate::testutil::test_ctx("en"),
            post_id: 0,
            title: String::new(),
            summary: String::new(),
            body: String::new(),
            published: false,
            featured: false,
            published_date: String::new(),
            edit_lang: "en".into(),
            error: String::new(),
            title_key: "news_new_page_title".into(),
            save_key: "news_create_button".into(),
        };
        let html = blank.render().expect("renders");
        assert!(html.contains("action=\"/admin/news/new\""), "{html}");
        assert!(
            html.contains("data-richtext=\"article\""),
            "the body opts into block preview"
        );
        // the date field says what a future date does, or the author has no way
        // to know a post can be queued at all
        assert!(html.contains("keeps the post hidden"), "{html}");

        let editing = NewsFormPage {
            post_id: 7,
            ..blank
        };
        let html = editing.render().expect("renders");
        assert!(html.contains("action=\"/admin/news/7/edit\""), "{html}");
    }

    /// The featured flag end to end against a real server: flagging a post
    /// strips the flag from the previous holder, a queued featured post is
    /// invisible to the hero until its moment, and the flagged post is not
    /// also listed in the teaser posts.
    #[tokio::test]
    async fn featured_is_exclusive_and_public_filtered() {
        let Some(db) = scratch_db("featured").await else {
            return;
        };
        db.query("DELETE news;").await.unwrap().check().unwrap();
        let now = 1_792_108_800; // 2026-10-16
        let draft = |title: &str, at: i64| news::NewsDraft {
            title: title.into(),
            summary: "s".into(),
            body: String::new(),
            published: true,
            published_at: at,
            featured: false,
        };
        let first = news::insert_post(&db, "en", &draft("First", now), "t", 0, now)
            .await
            .expect("insert");
        // flag it: the hero now answers with it
        news::update_post(
            &db,
            "en",
            first,
            &news::NewsDraft {
                featured: true,
                ..draft("First", now)
            },
            now,
        )
        .await
        .expect("flag");
        let hero = news::select_featured(&db, "en", now).await.unwrap();
        assert_eq!(hero.map(|p| p.id), Some(first));

        // a second post flagged later takes the flag away from the first
        let second = news::insert_post(&db, "en", &draft("Second", now), "t", 0, now)
            .await
            .expect("insert");
        news::update_post(
            &db,
            "en",
            second,
            &news::NewsDraft {
                featured: true,
                ..draft("Second", now)
            },
            now,
        )
        .await
        .expect("flag");
        let hero = news::select_featured(&db, "en", now).await.unwrap();
        assert_eq!(hero.map(|p| p.id), Some(second));
        // the archive list (include_featured = true, the select_published
        // default) still shows both posts; the teaser variant hides only
        // the featured one
        let archive = news::select_published(&db, "en", now, 10, 0).await.unwrap();
        assert!(
            archive.iter().any(|c| c.id == first) && archive.iter().any(|c| c.id == second),
            "the archive lists every live post, featured or not"
        );
        let teaser = news::select_published_filtered(&db, "en", now, 10, 0, false)
            .await
            .unwrap();
        assert!(
            teaser.iter().any(|c| c.id == first),
            "the demoted post is still listed as a normal post"
        );
        assert!(
            teaser.iter().all(|c| c.id != second),
            "the featured post is not also in the teaser list"
        );

        // a queued featured post: the hero stays with the live one until
        // its moment arrives
        let queued = news::insert_post(
            &db,
            "en",
            &news::NewsDraft {
                featured: true,
                ..draft("Later", now + 86_400)
            },
            "t",
            0,
            now,
        )
        .await
        .expect("insert queued");
        let hero = news::select_featured(&db, "en", now).await.unwrap();
        assert_eq!(hero.map(|p| p.id), Some(second), "queued stays hidden");
        let hero = news::select_featured(&db, "en", now + 86_400)
            .await
            .unwrap();
        assert_eq!(hero.map(|p| p.id), Some(queued), "revealed on time");
    }
}
