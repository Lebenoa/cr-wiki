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

## Not ported yet

Roughly in dependency order — the earlier ones unblock the rest.

1. **Sessions**: the `CRSESSID` cookie, the in-memory session map, and the user half of `is_admin` (the loopback half is ported)
2. **Builds**: the planner, the picker endpoints with search/tabs/pinning/combo ordering, preview, verify, edit, delete — the largest single area left, and the one `cards_by_ids` is waiting for
3. **The catalog option cache** (`app/options_cache.v`)
4. **The remaining `database/select.v`** (~2.6k lines): pets, treasures, effects, combi bonuses, builds, search (FTS5 + LIKE fallback), catalog queries
5. **Admin forms** (`app/forms.v`, ~650 lines) and image upload
6. **Auth**: login, register, Turnstile
7. **Gacha and relic detail pages**, which have shapes of their own
8. **The navbar** in full: the wiki dropdown, the theme popover and its editor, the language dialog, mobile — the port has a plain two-link bar

The JS in `static/js/` is unaffected: it is served as-is and already talks to
these routes.
