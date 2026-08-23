//! Search, the landing page and the SEO endpoints.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::Html;

use crate::ctx::Ctx;
use crate::db::{self, Card, GachaPool};
use crate::i18n;
use crate::state::AppState;

use super::CommonQuery;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    ctx: Ctx,
}

pub async fn index(ctx: Ctx) -> Html<String> {
    Html(IndexPage { ctx }.render().unwrap_or_else(|e| format!("template error: {e}")))
}

#[derive(Template)]
#[template(path = "search.html")]
struct SearchPage {
    ctx: Ctx,
    q: String,
    hits: Vec<(String, Card)>,
}

pub async fn search(
    State(state): State<AppState>,
    ctx: Ctx,
    q: Query<CommonQuery>,
) -> Html<String> {
    let query = q.q.clone().unwrap_or_default();
    let lang = ctx.lang.clone();
    let db = state.db.clone();
    let needle = query.clone();
    let hits = tokio::task::spawn_blocking(move || db::search(&db, &lang, &needle, 20))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
        .unwrap_or_default();
    Html(
        SearchPage { ctx, q: query, hits }
            .render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
}

#[derive(Template)]
#[template(path = "gacha.html")]
struct GachaPage {
    ctx: Ctx,
    pools: Vec<GachaPool>,
}

/// The disclosed draw pools with their odds.
pub async fn gacha(State(state): State<AppState>, ctx: Ctx) -> Html<String> {
    let lang = ctx.lang.clone();
    let db = state.db.clone();
    let pools = tokio::task::spawn_blocking(move || db::select_gacha(&db, &lang))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
        .unwrap_or_default();
    Html(
        GachaPage { ctx, pools }
            .render()
            .unwrap_or_else(|e| format!("template error: {e}")),
    )
}

/// Every list page plus every detail id, each with its locale alternates. A
/// section missing here is one crawlers only reach by luck.
pub async fn sitemap(State(state): State<AppState>, ctx: Ctx) -> ([(&'static str, &'static str); 1], String) {
    let base = ctx.site_url.clone();
    let langs = i18n::available_langs();
    let mut paths: Vec<String> = [
        "/", "/cookies", "/pets", "/treasures", "/builds", "/changelog", "/episodes",
        "/ingredients", "/jellies", "/skins", "/relics", "/gacha",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let db = state.db.clone();
    if let Ok(Ok(entries)) = tokio::task::spawn_blocking(move || db::sitemap_entries(&db)).await {
        paths.extend(entries.into_iter().map(|(section, id)| format!("/{section}/{id}")));
    }

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\n",
    );
    for path in &paths {
        xml.push_str(&format!("<url><loc>{base}{path}</loc>"));
        for lang in &langs {
            let suffix = if lang == i18n::DEFAULT_LANG { String::new() } else { format!("?lang={lang}") };
            xml.push_str(&format!(
                "<xhtml:link rel=\"alternate\" hreflang=\"{lang}\" href=\"{base}{path}{suffix}\"/>"
            ));
        }
        xml.push_str(&format!(
            "<xhtml:link rel=\"alternate\" hreflang=\"x-default\" href=\"{base}{path}\"/></url>\n"
        ));
    }
    xml.push_str("</urlset>\n");
    ([("content-type", "application/xml; charset=utf-8")], xml)
}

/// The form, auth and fragment routes are noise for a crawler.
pub async fn robots(ctx: Ctx) -> ([(&'static str, &'static str); 1], String) {
    let base = ctx.site_url.clone();
    let body = format!(
        "User-agent: *\nDisallow: /login\nDisallow: /register\nDisallow: /builds/new\nDisallow: /builds/preview\nDisallow: /builds/options/\nDisallow: /search\nAllow: /\n\nSitemap: {base}/sitemap.xml\n"
    );
    ([("content-type", "text/plain; charset=utf-8")], body)
}
