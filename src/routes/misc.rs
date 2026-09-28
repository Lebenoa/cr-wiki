//! Search, the landing page and the SEO endpoints.

use askama::Template;
use axum::extract::Query;
use axum::response::Html;

use crate::ctx::Ctx;
use crate::db::{self, Card, GachaPool};
use crate::grade::Graded;
use crate::i18n;

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
    groups: Vec<(String, Vec<Card>)>,
}

/// The dropdown body the navbar swaps into `#search-results` and the full
/// page's result section share one template so the two views cannot drift.
/// Section-grouped hits: grouping lives here, not in the template —
/// askama's `let` shadows rather than assigns, so a running key inside the
/// loop re-prints the header for every hit.
#[derive(Template)]
#[template(path = "search_results.html")]
struct SearchResults {
    ctx: Ctx,
    groups: Vec<(String, Vec<Card>)>,
}

pub async fn search(
    ctx: Ctx,
    q: Query<CommonQuery>,
) -> Result<Html<String>, AppError> {
    let state = crate::state::state();
    let query = q.q.clone().unwrap_or_default();
    let hits = db::search(&state.db, &ctx.lang, &query, 20).await?;
    let mut groups: Vec<(String, Vec<Card>)> = Vec::new();
    for (section, card) in hits {
        match groups.last_mut() {
            Some((last, cards)) if *last == section => cards.push(card),
            _ => groups.push((section, vec![card])),
        }
    }
    let html = if ctx.is_fragment() {
        SearchResults { ctx, groups }.render()
    } else {
        SearchPage {
            ctx,
            q: query,
            groups,
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
pub async fn gacha(ctx: Ctx) -> Result<Html<String>, AppError> {
    let state = crate::state::state();
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
    ctx: Ctx,
) -> ([(&'static str, &'static str); 1], String) {
    let state = crate::state::state();
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

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// Full-text search matches localized and English names and searches prose.
    #[tokio::test]
    async fn search_matches_localized_english_and_prose() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let en = db::search(&pool, "en", "kaymak", 20).await.unwrap();
        assert!(en
            .iter()
            .any(|(section, c)| section == "cookies" && c.name.contains("Kaymak")));

        let th = db::search(&pool, "th", "wizard", 20).await.unwrap();
        assert!(!th.is_empty());

        let th_partial = db::search(&pool, "th", "กล้า", 20).await.unwrap();
        assert!(th_partial.iter().any(|(section, c)| section == "cookies" && c.id == 1));
        let prose = db::search(&pool, "en", "abilities", 20).await.unwrap();
        assert!(prose.iter().any(|(section, _)| section == "cookies"));
        assert!(db::search(&pool, "en", "   ", 20).await.unwrap().is_empty());
    }

    /// The gacha pools carry their prizes and odds.
    #[tokio::test]
    async fn gacha_pools() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let pools = db::select_gacha(&pool, "en").await.expect("gacha");
        assert!(!pools.is_empty());
        let entries: usize = pools.iter().map(|p| p.entries.len()).sum();
        assert_eq!(entries, 304, "every disclosed entry is listed");
        let first = pools.iter().find(|p| !p.entries.is_empty()).unwrap();
        assert!(first.entries.iter().all(|e| e.odds > 0.0));
        assert!(first.entries.iter().all(|e| !e.name.is_empty()));
        assert!(first.entries[0].odds_label().ends_with('%'));
    }

    /// The sitemap covers every detail id the six sections hold.
    #[tokio::test]
    async fn sitemap_covers_the_catalog() {
        let Some(pool) = live_db().await else {
            eprintln!("skip: CR_SURREAL_URL not set");
            return;
        };

        let entries = db::sitemap_entries(&pool).await.unwrap();
        for section in [
            "cookies",
            "pets",
            "treasures",
            "episodes",
            "ingredients",
            "jellies",
        ] {
            assert!(
                entries.iter().any(|(s, _)| s == section),
                "{section} missing"
            );
        }
        assert!(entries.len() > 1000);
    }

    /// The gacha page renders through its template, navbar and tabs script
    /// included, even with no pools behind it.
    #[test]
    fn gacha_page_renders() {
        let html = GachaPage {
            ctx: crate::testutil::test_ctx("en"),
            pools: Vec::new(),
        }
        .render()
        .expect("gacha renders");
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("/static/js/gacha_tabs.js"));
    }

    /// The DB handle behind the gated tests: unset means they skip, so
    /// `cargo test` stays green without a server. Points at a scratch
    /// namespace/database — never at data you cannot lose.
    async fn live_db() -> Option<crate::db::Db> {
        let url = std::env::var("CR_SURREAL_URL").ok()?;
        let ns = std::env::var("CR_SURREAL_NS").unwrap_or_else(|_| "cookierun".into());
        let database = std::env::var("CR_SURREAL_DB").unwrap_or_else(|_| "cookierun".into());
        let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
        let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
        crate::db::connect_url(&url, &ns, &database, &user, &pass)
            .await
            .ok()
    }
}
