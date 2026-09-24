//! Search, the landing page and the SEO endpoints.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::Html;

use crate::ctx::Ctx;
use crate::db::{self, Card, GachaPool};
use crate::grade::Graded;
use crate::i18n;
use crate::state::AppState;

use super::{errors::AppError, CommonQuery};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    ctx: Ctx,
}

pub async fn index(ctx: Ctx) -> Html<String> {
    Html(
        IndexPage { ctx }
            .render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
}

#[derive(Template)]
#[template(path = "search.html")]
struct SearchPage {
    ctx: Ctx,
    q: String,
    hits: Vec<(String, Card)>,
}

/// The dropdown body the navbar swaps into `#search-results`: the same hits as
/// the full page, without the page shell. One template serves both so the two
/// views cannot drift.
#[derive(Template)]
#[template(path = "search_results.html")]
struct SearchResults {
    ctx: Ctx,
    hits: Vec<(String, Card)>,
}

pub async fn search(
    State(state): State<AppState>,
    ctx: Ctx,
    q: Query<CommonQuery>,
) -> Result<Html<String>, AppError> {
    let query = q.q.clone().unwrap_or_default();
    let hits = db::search(&state.db, &ctx.lang, &query, 20).await?;
    let html = if ctx.is_fragment() {
        SearchResults { ctx, hits }.render()
    } else {
        SearchPage {
            ctx,
            q: query,
            hits,
        }
        .render()
    };
    Ok(Html(
        html.unwrap_or_else(|e| format!("template error: {e}")),
    ))
}

#[derive(Template)]
#[template(path = "gacha.html")]
struct GachaPage {
    ctx: Ctx,
    pools: Vec<GachaPool>,
}

/// The disclosed draw pools with their odds.
pub async fn gacha(State(state): State<AppState>, ctx: Ctx) -> Result<Html<String>, AppError> {
    let pools = db::select_gacha(&state.db, &ctx.lang).await?;
    Ok(Html(
        GachaPage { ctx, pools }
            .render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    ))
}

/// Every list page plus every detail id, each with its locale alternates. A
/// section missing here is one crawlers only reach by luck.
pub async fn sitemap(
    State(state): State<AppState>,
    ctx: Ctx,
) -> ([(&'static str, &'static str); 1], String) {
    let base = ctx.site_url;
    let langs = i18n::available_langs();
    let mut paths: Vec<String> = [
        "/",
        "/cookies",
        "/pets",
        "/treasures",
        "/builds",
        "/changelog",
        "/episodes",
        "/ingredients",
        "/jellies",
        "/skins",
        "/relics",
        "/gacha",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    // deliberately lenient: a database failure shrinks the sitemap rather
    // than failing it — robots.txt always answers, and the partial sitemap
    // heals on the next request
    if let Ok(entries) = db::sitemap_entries(&state.db).await {
        paths.extend(
            entries
                .into_iter()
                .map(|(section, id)| format!("/{section}/{id}")),
        );
    }

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\n",
    );
    for path in &paths {
        xml.push_str("<url><loc>");
        xml.push_str(&base);
        xml.push_str(path);
        xml.push_str("</loc>");
        for lang in &langs {
            let suffix = if lang == i18n::DEFAULT_LANG {
                String::new()
            } else {
                format!("?lang={lang}")
            };
            xml.push_str("<xhtml:link rel=\"alternate\" hreflang=\"");
            xml.push_str(lang);
            xml.push_str("\" href=\"");
            xml.push_str(&base);
            xml.push_str(path);
            xml.push_str(&suffix);
            xml.push_str("\"/>");
        }
        xml.push_str("<xhtml:link rel=\"alternate\" hreflang=\"x-default\" href=\"");
        xml.push_str(&base);
        xml.push_str(path);
        xml.push_str("\"/></url>\n");
    }
    xml.push_str("</urlset>\n");
    ([("content-type", "application/xml; charset=utf-8")], xml)
}

/// The form, auth and fragment routes are noise for a crawler.
pub async fn robots(ctx: Ctx) -> ([(&'static str, &'static str); 1], String) {
    let base = ctx.site_url;
    let body = format!(
        "User-agent: *\nDisallow: /login\nDisallow: /register\nDisallow: /builds/new\nDisallow: /builds/preview\nDisallow: /builds/options/\nDisallow: /search\nAllow: /\n\nSitemap: {base}/sitemap.xml\n"
    );
    ([("content-type", "text/plain; charset=utf-8")], body)
}
