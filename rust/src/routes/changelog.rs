use askama::Template;
use axum::extract::Query;
use axum::response::Html;

use crate::changelog::{self, ChangeEntry};
use crate::i18n::Loc;
use crate::pagination::slice_page;

use super::{lang_of, CommonQuery};

pub const PAGE_SIZE: i64 = 30;

#[derive(Template)]
#[template(path = "changelog.html")]
struct ChangelogPage {
    l: Loc,
    lang: String,
    entries: Vec<ChangeEntry>,
    next_url: String,
    page: i64,
}

pub async fn page(q: Query<CommonQuery>) -> Html<String> {
    let lang = lang_of(&q);
    let page = q.page.unwrap_or(1).max(1);
    let all = changelog::entries();

    let (entries, next_url) = match slice_page(page, PAGE_SIZE, all.len()) {
        Some((start, end)) => {
            let next = if end < all.len() {
                format!("/changelog?page={}", page + 1)
            } else {
                String::new()
            };
            (all[start..end].to_vec(), next)
        }
        None => (Vec::new(), String::new()),
    };

    let tpl = ChangelogPage { l: Loc::new(&lang), lang, entries, next_url, page };
    Html(tpl.render().unwrap_or_else(|e| format!("template error: {e}")))
}
