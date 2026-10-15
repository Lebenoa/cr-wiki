//! The build planner's write side: submit, edit, delete and verify.
//!
//! The surface owns one file per concern: this table, the submitted shape and
//! its validation (`form`), the page it renders and the two handlers that only
//! read it (`page`), the writes (`write`), and the verdict a signed-in visitor
//! leaves on someone's build (`review`).

use axum::routing::{get, post};
use axum::Router;

mod form;
mod page;
mod review;
mod write;

use page::{edit_form, new_form};
use review::verify;
use write::{create, delete, update};

/// The planner's write side. `/builds/new` is a static segment under the
/// `/builds/{id}` capture `builds` registers, so this table is merged first.
pub fn routes() -> Router {
    Router::new()
        .route("/builds/new", get(new_form).post(create))
        .route("/builds/{id}/delete", post(delete))
        .route("/builds/{id}/verify", post(verify))
        .route("/builds/{id}/edit", get(edit_form).post(update))
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests;
