# Repository Guidelines

## Project Overview
The repository implements a web platform written in V (V Language) using a specialized web framework called `veb`. The purpose is to display and manage rich data about collectible assets—specifically cookies, pets, and treasures—serving as a modular content management system with server-side rendering (SSR) capabilities.

## Architecture & Data Flow
- **Overall pattern**: MVC‑like architecture adapted for V.
- **Request lifecycle**:
  1. Incoming HTTP request hits an entry point in `app/` controllers.
  2. Controllers delegate business logic to a service layer located in `database/`.
  3. Data is persisted through domain models defined in `database/models/`.
  4. A structured context (including fetched data and local state) is prepared for rendering.
  5. The view template (`templates/views/*.html`) consumes this context, producing the final HTML response.
- **Key modules**:
  - *Database layer*: entities, repositories, and domain logic.
  - *Controller layer*: request handling and orchestration.
  - *Service layer*: business rules (exposed as functions in `app/` or `database/`).
  - *Presentation layer*: view templates.

## Key Directories
- **`database/models/*.v`**: Persistent domain entities (e.g., `User`, `Pet`, `Cookie`, `Treasure`). Defined with ORM‑like annotations (`@[]`) for primary keys, foreign keys, and constraints.
- **`database/`**: Contains database schema definitions and model mappings.
- **`app/`**: Controllers and high‑level request handlers.
- **`templates/`**: `views/` full pages, `components/` shared partials (cards, effect cards, treasure variants, search results), `layout/` navbar/head, `admin/` forms. The shared-partials dir was renamed from `partials/` to `components/`.
- **`scripts/seed_data.json`**: The only tracked file in `scripts/` — a committed DB fixture. A fresh DB seeds itself from it via `seed_if_empty()` in `database/database.v`, so regenerate it whenever data or DB translations change. All other files in `scripts/` are untracked scraper/seed scripts — never commit them (user preference). Wiki data is fetched with `curl` via the MediaWiki API (`action=query&prop=revisions&rvprop=content`), not the fetch tool.
- **`translations/{en,th}.tr`**: Translation keys consumed via `ctx.tr()`.

## Development Commands
- **Run:** `v -d sqlite_fts5 -d new_veb -enable-globals run .` — all three flags are required: `-d sqlite_fts5` enables SQLITE_ENABLE_FTS5, `-d new_veb` selects the new veb backend, `-enable-globals` for app globals. Omitting `-d new_veb` falls back to the legacy veb backend.
- **Build/typecheck:** `v -d sqlite_fts5 -d new_veb -enable-globals -o /tmp/app.exe .` — exit 0 means clean. V "notice:" messages (e.g. implicit slice clone in `database/select.v`) are warnings, not errors; silence with an explicit `.clone()` or `unsafe{a[..]}`.
- **Server:** binds 0.0.0.0:6785 (`Config.toml`). Restart detached: kill the old PID, then `(v -d sqlite_fts5 -d new_veb -enable-globals run . > /tmp/cr_server.log 2>&1 &)`. Replacing a registered preview stops the old server.
- **Package manager:** use bun/bunx, never node/npm/npx (user preference).

## Code Conventions & Common Patterns
- Use explicit imports rather than implicit paths.
- Follow structured results for error handling (return controlled HTTP responses).
- Avoid unnecessary abstractions; prefer straightforward functions and modules.
- Error handling: return status codes via the result type, not generic exceptions.
- User-facing text always goes through translation keys (`translations/{en,th}.tr` + `ctx.tr()`) — no hardcoded labels. Adding a label touches the template and both .tr files together.
- **Grade model** (`database/models/grade.v`): `enum Grade as u8` ordered `e, c, b, a, s, s_plus, l` — E ("Extra") ranks ABOVE L ("Legend"). Ordering/tooltips come from `grade_values` + `grade_name()`. V enums convert via `.from('x')` / `.from(1)` and `.str()`. `treasure.grade` is `?int` — `none` means no wiki grade, no badge.
- **List pages** (cookies/pets/treasures): server-paginated infinite scroll (htmx `revealed`) + client-side filters; filters must re-apply to newly fetched content. Ordered by id, newest first; unknown release dates sort last (sentinel date + name tie-break).
- **Treasure effects**: stored in one table with a `state` column (normal/evo/blessed) — not separate tables. Base/evo variants are linked and rendered from shared `templates/components/` partials; the list page has all/normal/evolved tabs (default All) with a prominent "evolved" badge.

## Important Files
- **`database/models/*.v`**: Source of truth for schema definition and invariants.
- **`app/cookies.v`**: Example controller handling GET (view) vs POST (submission).
- **`app/app.v`**: `img_src()` returns the local image path or a `placehold.co` placeholder URL when missing — templates must use it, never build image paths directly.
- **`app/update.v` / `database/update.v`**: the detail-page edit feature. Admin routes (`/new/*`) are admin-only; test admin credentials are `test`/`test`.
- **`templates/views/cookies.html`**: Entry point for cookie management UI.
- **`README.md`** (if present): Project setup instructions.

## Runtime / Tooling Preferences
- **UnoCSS**: `bun run dev` = `unocss --watch` (package.json). It regenerates `static/styles.css` from utility classes in `./**/*.html` + `./app/*.v` (uno.config.ts scan globs). Commit the regenerated CSS together with template class changes; classes only generate for files inside the scan globs (test/probe files must live in the project root). Keep the watcher running — do not edit `static/styles.css` by hand.
- **No `<style>` tags** anywhere — all styling via UnoCSS utilities (`starting:` = `@starting-style`).
- **htmx** is served from `/thirdparty/htmx.js` (a route; not an on-disk dir) and drives navbar search, infinite scroll, and hx-boost navigation.
- **Chrome top-layer popover quirks** (hard-won):
  - `width:auto` never stretches on `popover` elements — stays shrink-to-fit even with `left-2 right-2`; an explicit width is required.
  - `100vw` includes the scrollbar column, while fixed/popover percentages resolve against the layout viewport (viewport − scrollbar). The desktop preview misaligns ~8px; on phones (overlay scrollbars) `100vw` == viewport — so use `w-screen m-0` (`margin:auto` UA-centering must be killed with `m-0`).
  - uno.config preflight sets `position-area: bottom center` on `div[popover]`; this Chrome doesn't resolve implicit anchors so explicit positioning wins, but the preflight is a latent conflict if anchors ever resolve.
  - Prefer physical `left-2 right-2` over logical `inset-x-2` — logical properties misbehaved in the top layer.
- **Testing gotchas** (preview):
  - Synthetic `.click()` in preview_evaluate does NOT trigger popover light-dismiss — use real `preview_click` clicks.
  - htmx's `changed` trigger dedupes identical input values — re-typing the same query won't re-fire; use a fresh value or reload.
  - Search debounce is `hx-trigger="input changed delay:300ms"`; the loading indicator uses `hx-indicator` + `not-[.hidden]` classes.
  - Default FTS5 doesn't tokenize Thai — Thai queries in the search box are a known gap; a Thai-aware tokenizer/search path is still pending.

## Testing & QA
- Test frameworks: None currently defined; a default test suite will be added after initial development.
- Coverage expectation: High coverage on business logic validation paths and state transitions.
