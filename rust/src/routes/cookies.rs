use askama::Template;
use axum::extract::{Query, State};
use axum::response::Html;

use crate::db::{self, CookieCard, Db};
use crate::i18n::Loc;

use super::{lang_of, CommonQuery};

pub const PAGE_SIZE: i64 = 30;

#[derive(Template)]
#[template(path = "cookies.html")]
struct CookiesPage {
    l: Loc,
    lang: String,
    cookies: Vec<CookieCard>,
    next_page: i64,
}

pub async fn list(State(pool): State<Db>, q: Query<CommonQuery>) -> Html<String> {
    let lang = lang_of(&q);
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1).saturating_mul(PAGE_SIZE);

    let cookies = tokio::task::spawn_blocking({
        let pool = pool.clone();
        let lang = lang.clone();
        move || db::select_cookies(&pool, &lang, PAGE_SIZE, offset)
    })
    .await
    .unwrap_or_else(|_| Ok(Vec::new()))
    .unwrap_or_default();

    let next_page = if cookies.len() as i64 == PAGE_SIZE { page + 1 } else { 0 };
    let tpl = CookiesPage { l: Loc::new(&lang), lang, cookies, next_page };
    Html(tpl.render().unwrap_or_else(|e| format!("template error: {e}")))
}
