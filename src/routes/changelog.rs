use askama::Template;
use axum::extract::Query;
use axum::response::Html;

use crate::changelog::{self, ChangeEntry};
use crate::ctx::Ctx;
use crate::pagination::slice_page;

use super::CommonQuery;

pub const PAGE_SIZE: i64 = 30;

#[derive(Template)]
#[template(path = "changelog_entries.html")]
struct ChangelogEntries {
    ctx: Ctx,
    entries: Vec<ChangeEntry>,
    next_url: String,
    page: i64,
}

#[derive(Template)]
#[template(path = "changelog.html")]
struct ChangelogPage {
    ctx: Ctx,
    entries: Vec<ChangeEntry>,
    next_url: String,
    page: i64,
}

pub async fn page(ctx: Ctx, q: Query<CommonQuery>) -> Html<String> {
    let page = q.page.unwrap_or(1).max(1);
    let all = changelog::entries();

    let (entries, next_url) = match slice_page(page, PAGE_SIZE, all.len()) {
        Some((start, end)) => {
            let next = if end < all.len() {
                format!("/changelog?page={}", page.saturating_add(1))
            } else {
                String::new()
            };
            let window = all
                .get(start..end)
                .map(<[changelog::ChangeEntry]>::to_vec)
                .unwrap_or_default();
            (window, next)
        }
        None => (Vec::new(), String::new()),
    };

    // an infinite-scroll swap wants the rows alone; an hx-boosted navigation
    // is still a page load and wants the whole document
    let html = if ctx.is_fragment() {
        ChangelogEntries {
            ctx,
            entries,
            next_url,
            page,
        }
        .render()
    } else {
        ChangelogPage {
            ctx,
            entries,
            next_url,
            page,
        }
        .render()
    };
    Html(html.unwrap_or_else(|e| format!("template error: {e}")))
}
