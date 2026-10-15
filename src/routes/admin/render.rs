//! How the admin form page is composed: the two entry points the editor and
//! the writes render through, plus the option lists the treasure form needs.

use askama::Template;
use axum::response::{Html, IntoResponse, Response};

use crate::ctx::Ctx;
use crate::db::{self, CombiEditRow, Detail};
use crate::section::Section;

use super::form::{AdminForm, IdName};

pub(super) async fn page(
    ctx: Ctx,
    section: Section,
    item: Option<Detail>,
    error: &str,
) -> Response {
    page_with(
        ctx.clone(),
        section,
        item,
        Vec::new(),
        error,
        ctx.lang.clone(),
        String::new(),
    )
    .await
}

/// The link-select option lists the treasure form needs. Cached like the
/// picker lists, so an edit page costs no extra queries.
async fn link_opts(db: &crate::db::Db, lang: &str) -> (Vec<IdName>, Vec<IdName>, Vec<IdName>) {
    async fn simple(db: &crate::db::Db, lang: &str, kind: &str) -> Vec<IdName> {
        let list = crate::options::options(db, lang, kind)
            .await
            .unwrap_or_default();
        list.iter()
            .map(|o| IdName {
                id: o.id,
                name: if o.name.is_empty() {
                    o.en_name.clone()
                } else {
                    o.name.clone()
                },
            })
            .collect()
    }
    let c = simple(db, lang, "cookie").await;
    let p = simple(db, lang, "pet").await;
    let t = simple(db, lang, "treasure").await;
    (c, p, t)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn page_with(
    ctx: Ctx,
    section: Section,
    item: Option<Detail>,
    combi: Vec<CombiEditRow>,
    error: &str,
    edit_lang: String,
    preview_html: String,
) -> Response {
    let state = crate::state::state();
    let (cookie_opts, pet_opts, treasure_opts, links) = if section == Section::Treasures {
        let (c, p, t) = link_opts(&state.db, &edit_lang).await;
        let links = match item.as_ref() {
            Some(i) => db::treasure_edit_links(&state.db, i.id)
                .await
                .unwrap_or_default(),
            None => db::TreasureEditLinks::default(),
        };
        (c, p, t, links)
    } else {
        (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            db::TreasureEditLinks::default(),
        )
    };
    Html(
        AdminForm {
            ctx,
            section: section.as_str().to_string(),
            item,
            combi,
            edit_lang,
            preview_html,
            error: error.to_string(),
            cookie_opts,
            pet_opts,
            treasure_opts,
            links,
        }
        .render()
        .unwrap_or_else(|e| format!("template error: {e}")),
    )
    .into_response()
}
