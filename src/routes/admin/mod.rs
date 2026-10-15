//! The admin catalog forms: create and edit a cookie, pet or treasure.
//!
//! Every handler gates on `ctx.is_admin()` and answers 404 rather than 403,
//! matching the V routes — whether an admin page exists is not worth
//! confirming to a stranger.
//!
//! The surface owns one file per concern: this table, the form's shape and
//! its gates (`form`), how the form page is composed (`render`), the two
//! editor reads (`edit`) and the three writes (`save`).

use axum::routing::{get, post};
use axum::Router;

mod edit;
mod form;
mod render;
mod save;

use edit::{edit_form, new_form};
use save::{create, delete_combi, update};

/// The submitted entity form is the write side's interface: `db::admin` takes
/// it by reference, so it is re-exported from the surface rather than reached
/// through its file.
pub use form::EntityForm;

/// The admin forms. Registered before the catalog captures, so
/// `/cookies/new` is a form rather than a detail page for id "new".
pub fn routes() -> Router {
    Router::new()
        .route("/{section}/new", get(new_form).post(create))
        .route("/{section}/{id}/combi/{row_id}/delete", post(delete_combi))
        .route("/{section}/{id}/edit", get(edit_form).post(update))
}
