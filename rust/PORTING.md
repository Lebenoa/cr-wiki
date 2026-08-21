# Rust port

The V app is the live one; this is the port growing alongside it. Both read
the same `Config.toml`, `translations/`, `static/` and `sqlite.db` from the
repo root, so a ported page can be diffed against the original by eye.

Run the V app as usual. Run this one with `cargo run` from `rust/` (it binds
the same host/port as `Config.toml`, so stop the V watch first, or override).
`cargo test` needs neither: it exercises the parsing, paging, queries and
templates directly.

## Stack, and why

| concern | choice | why |
| --- | --- | --- |
| HTTP | axum + tokio | closest thing to veb's routing with a live ecosystem |
| templates | askama | compile-time checked like veb's comptime templates, and escapes by default |
| SQLite | rusqlite + r2d2, behind `spawn_blocking` | the V queries are synchronous and short; an async driver would turn a translation into a rewrite. FTS5 comes from the bundled build |
| config | serde + toml | same file, same defaults, same env overrides |
| i18n | hand-rolled `.tr` loader | the format is this project's own |

Two deliberate differences from the V original:

- **askama escapes by default.** veb interpolates verbatim, which is what let
  a commit body containing `<select>` break the changelog list. The port
  cannot reproduce that class of bug in a template.
- **paging is i64 and returns `Option`.** The V version computed
  `(page - 1) * size` in 32-bit and panicked the process on `?page=100000000`;
  `pagination::slice_page` reports past-the-end instead.

## Ported

- `config/config.v` -> `src/config.rs` — defaults, env overrides, rate-limit clamping
- `translations/*.tr` + `app/api/available_langs.v` -> `src/i18n.rs` — startup scan, en fallback
- `app/changelog.v` -> `src/changelog.rs` + `src/routes/changelog.rs` — git log parse, conventional-commit split, trailer drop, `/changelog` with paging
- `select_cookies` -> `src/db.rs` — including the `release_date DESC, cookie_id DESC` tie-break
- `/cookies` list -> `src/routes/cookies.rs`
- static file serving, the shared paging window
- `before_request` -> `src/ctx.rs` + `src/middleware.rs` — `?lang=` over the `wikilang` cookie over English, the cookie refresh, the stripped path, `site_url`/`canonical_url`/`lang_url`, htmx vs hx-boosted, loopback detection
- `app/ratelimit.v` -> `src/ratelimit.rs` — per-IP token bucket, wall-clock refill, idle sweep, `Retry-After`, and the empty body for a denied fragment; bypassed in debug builds the way `$if !prod` bypasses it
- the changelog fragment path, so infinite scroll returns rows rather than a whole document
- `database/models/grade.v` -> `src/grade.rs` — the enum ordinal is not the display order, so rank and the SQL CASE follow `grade_values` where E outranks L
- the catalog lists: cookies, pets, treasures (with the all/normal/evolved tabs), episodes, ingredients, jellies, skins, relics — one handler, section as a path capture
- the detail pages: cookie, pet, treasure, episode, ingredient, jelly, with effects and combo bonuses
- `app/richtext.v` -> `src/richtext.rs` — `[[id]]` links with sprites, `{color:}` spans, everything else escaped
- `/search`, `/sitemap.xml` with locale alternates, `/robots.txt`, the landing page
- sessions, login, register, logout — argon2id in PHC form so hashes interop with V, and `is_admin` whole
- the build list and one build's detail page: the filters, the four sorts with the id tie-break, the expiry rule, and the loadout slots
- `app/options_cache.v` -> `src/options.rs` — the picker lists per language, built in three queries rather than one per treasure, dropped on a catalog write
- `/builds/options/:kind` -> `src/routes/picker.rs` — paginated grid, whitespace-split search where every term must match somewhere, the treasure tabs, the slot's own pick pinned first and the combo partners floated above the rest
- the navbar in full: the wiki dropdown, the theme popover and its custom-theme editor, the language dialog, the mobile sheet and the account menu — the same markup theme.js and theme_editor.js already drive
- `/gacha` with the disclosed pool odds, and `/builds/preview` and `/builds/:id/verify`
- the planner write side: `/builds/new`, `/builds/:id/edit` and `/builds/:id/delete`, with the author/owner/expiry left alone on an edit and a 404 rather than a 403 for someone else's build
- Turnstile on login, register and build submit, refusing rather than waving through when misconfigured
- the admin catalog forms for cookies, pets and treasures, upserting the translation per language so editing in Thai cannot wipe the English text
- `/api/available-langs`, `/api/richtext-names`, `/api/set-lang` and `/api/relics`
- relic and skin detail pages, and a treasure's unlock chain (the cookie or pet that grants it, and the base it evolved from)
- admin image upload: multipart into `static/img/<section>/`, with the extension taken from the declared content type rather than the filename
- the picker dialogs on both `/builds` and the planner, sharing one partial, driven by the existing picker.js

## Not ported yet

Nothing. Every route in the V app's table has an equivalent here, and the
polish items that were listed last round are done: the planner's live
preview, the edit-form prefill, the combo editor rows and the blessed-toggle
diffing.

Two things are deliberately different rather than missing:

- **`/combi/:id/delete` returns to `/cookies`**, not to the form it was
  submitted from. Following the referer would mean trusting a header for a
  redirect target; the editor is one click from the catalog either way.
- **The V app's `$if !prod` gates become `cfg!(debug_assertions)`**, so a
  debug build skips rate limiting and Turnstile exactly as `v run` does, and
  `cargo build --release` enables both.

## Verifying

`cargo test` covers the layers directly: 25 tests over config, i18n, paging,
the git-log parser, every catalog query, the detail rows, rich text, search,
grades, sessions, password hashing, the build queries, the picker lists, the
gacha pools, Turnstile and the upload sanitiser.

Nothing has been compared against the running V app page by page yet. That
is the one thing these tests cannot stand in for, and it is the next check
worth doing: start the V app on 6785, run this one on another port, and diff
the pages.
