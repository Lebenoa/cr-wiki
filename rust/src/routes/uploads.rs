//! `POST /{section}/upload` — the admin image upload behind the catalog
//! forms. Answers JSON, so the form can drop the returned filename into its
//! image field without a page reload.

use axum::extract::{Multipart, Path};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::ctx::Ctx;
use crate::upload;

pub async fn image(
    ctx: Ctx,
    Path(section): Path<String>,
    mut form: Multipart,
) -> Response {
    // 404 rather than 403 for a non-admin, matching the other admin routes
    if !ctx.is_admin() {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let Some(dir) = upload::section_dir(&section) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    while let Ok(Some(field)) = form.next_field().await {
        if field.name() != Some("image") {
            continue;
        }
        // the extension comes from the declared type, not the submitted
        // filename, so `sprite.png.html` cannot be written as markup
        let content_type = field.content_type().unwrap_or_default().to_string();
        let Some(ext) = upload::extension_for(&content_type) else {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({ "error": "unsupported image type" })),
            )
                .into_response();
        };
        let stem = upload::safe_stem(field.file_name().unwrap_or("image"));

        let Ok(bytes) = field.bytes().await else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "could not read upload" })),
            )
                .into_response();
        };
        if bytes.is_empty() || bytes.len() > upload::MAX_BYTES {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({ "error": "image too large" })),
            )
                .into_response();
        }

        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("upload: cannot create {}: {e}", dir.display());
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "could not store image" })),
            )
                .into_response();
        }
        let name = upload::unique_name(&dir, &stem, ext);
        if let Err(e) = std::fs::write(dir.join(&name), &bytes) {
            tracing::warn!("upload: cannot write {name}: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "could not store image" })),
            )
                .into_response();
        }
        // the static handler already serves this directory, so the sprite is
        // reachable immediately
        return Json(json!({ "image": name, "url": format!("/img/{section}/{name}") }))
            .into_response();
    }

    (StatusCode::BAD_REQUEST, Json(json!({ "error": "no image field" }))).into_response()
}
