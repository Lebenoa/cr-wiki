# Repository Guidelines

## Project Overview
Cookie Run fan wiki: cookies, pets, treasures, episodes, ingredients, jellies,
skins, relics, gacha pools and community builds, with server-side rendered
pages, a build planner and admin editing. The implementation is **Rust**
(axum + askama + rusqlite) at the repo root. The original V/veb app was removed
from the tree once the port reached parity — its history lives in git, and
`PORTING.md` records the port's intent.

## Development Commands
Run from the repo root:
- **Dev run:** `cargo run` — binds `Config.toml`'s host/port (default 127.0.0.1:6785).
  Debug builds skip rate limiting and Turnstile verification, and grant admin
  to headerless loopback peers (local development needs no login).
- **Release:** `cargo run --release` — this is the deployable profile: rate
  limiting and Turnstile verification are active, and the loopback admin
  bypass is compiled out (`cfg!(debug_assertions)`). Deployments MUST be
  release builds, or every form submission skips the bot check.
- **Typecheck:** `cargo check` — exit clean before yielding work.
- **Tests:** `cargo test` (unit tests live beside the code, e.g. in
  `src/main.rs`). Never start a dev server yourself; if one is already
  running, reuse it and leave it alone.
- The app reads `Config.toml`, `translations/` and `static/` from the repo
  root; `CR_HOST` / `CR_PORT` env vars override the bind address without
  editing the shared file.

## Key Directories
- **`src/main.rs`** — router assembly, middleware ordering, tracing setup.
- **`src/routes/*.rs`** — one module per surface (`catalog`, `detail`,
  `builds`, `planner`, `picker`, `admin`, `auth`, `uploads`, `misc` (sitemap,
  robots, changelog), `api`, `errors`). List pages share one handler keyed by
  path section.
- **`src/db.rs`** — all SQL. Values are always bound parameters;
  identifiers/table names come from match whitelists, never from request
  input.
- **`src/ctx.rs`** — per-request context extractor (locale, site URL,
  htmx flags, session user, `is_local`/`is_admin`) mirroring the old veb
  Context.
- **`src/session.rs`** — argon2id password hashing (PHC strings, cross-
  compatible with hashes the V app wrote), in-memory sessions keyed by the
  CRSESSID cookie.
- **`src/{ratelimit,middleware}.rs`** — per-client token bucket applied
  before locale resolution.
- **`templates/**`**** — askama templates (own tree; there is no shared
  template dir anymore).
- **`translations/{en,th}.tr`** — loaded once at startup by `i18n::load`;
  a missing key renders as the key itself rather than panicking.
- **`scripts/seed_data.json`** — committed data fixture. It was consumed by
  the removed V seeding layer (`seed_if_empty`); until an import tool exists,
  fresh databases must be populated from it manually. The upstream source of
  truth is `scripts/cookierundb/*.json` (uncommitted scraper output); the
  per-level values, grades and blessed states in the fixture are rebuilt from
  it by the untracked `scripts/build_seed_cookierundb.py`. Every other file
  in `scripts/` is untracked scraper tooling — never commit it.

## Security Invariants (all verified by review; do not regress)
- **Admin gating:** unauthenticated access to admin routes returns **404**,
  not 401/403. `Ctx::is_admin` ORs in the loopback bypass only in debug
  builds; `is_local` requires a loopback TCP peer AND absence of
  `CF-Connecting-IP` / `X-Forwarded-For` / `X-Real-Ip`, failing closed when
  no peer address is available.
- **Forwarded headers are guilty until proven trusted:** the rate limiter
  keys buckets on the TCP peer; forwarded headers stand in only when the peer
  is listed in `[ratelimit] trusted_proxies` (Config.toml /
  Config.example.toml). Behind a CDN, configure the proxy IPs there or every
  visitor shares one bucket.
- **Rate limiter bounds:** idle sweep past `sweep_above` entries, hard cap of
  65,536 buckets with oldest-eviction — a flood of unique addresses must not
  turn the map into an attacker-sized allocation.
- **Build video links** accept only `http://` / `https://` (≤200 chars, no
  control bytes): they render as live hrefs and HTML escaping does not
  neutralise a `javascript:` target. Description is capped at 5000 chars.
- **Redirect targets** must be site-relative paths of printable ASCII —
  absolute URLs are open redirects, CR/LF bytes smuggle response headers
  through HX-Redirect / Location.
- **Sessions** come from the OS CSPRNG (uuid v4), carry a 7-day TTL swept on
  access, and their cookie is HttpOnly + Secure + SameSite=Lax. Logout deletes
  server-side.
- **Turnstile** fails closed on missing config or any network error, and is
  skipped only in debug builds.

## Code Conventions & Common Patterns
- **Grade model**: display rank is NOT the enum's declaration order —
  E ("Extra") ranks ABOVE L ("Legend"). Any graded grid orders through
  `grade::rank_sql(col)` (a CASE built from the shared rank table), never
  `ORDER BY grade DESC`.
- **Ordering belongs in SQL**, not in memory over loaded rows.
- **Treasure effects** live in one table with a `state` column
  (`normal`/`blessed`); per-level values stay strings (+0..+9 rows); builds
  store a per-slot equipped level 0–9. Combi bonuses reuse the effect table.
- **Images**: `static/img/<entity>/<english name snake_case>.png`, `_2`/`_3`
  suffixes when entities share a display name but differ in artwork. Never
  store coded catalog icon names.
- **User-facing text** goes through translation keys present in BOTH
  `translations/en.tr` and `th.tr` — add to both files together.
- **Locale in the URL**: `?lang=xx` selects a language, validated against the
  loaded locales, falling back to the `wikilang` cookie then `en`; canonical
  URLs and hreflang alternates derive from the same helper so they cannot
  disagree. Language switching is plain links, never POST.
- **No `<style>` tags anywhere** and **no HTML comments in templates** —
  styling is UnoCSS utilities plus the preflight blocks in `uno.config.ts`
  (theme palettes, popover positioning, the treasure effect-panel animations).

## Runtime / Tooling Preferences
- **UnoCSS**: regenerate `static/styles.css` with `bunx unocss` (config's
  cli entry scans `./**/*.html`) whenever template classes change, and commit
  the regenerated CSS together with the change. A class that only appears in
  `.rs` files or JS is never generated — style JS-driven state from an
  attribute the markup carries. Use bun/bunx, never node/npm/npx.
- **Package manager**: bun (see above).
- **Static assets** are served by tower-http `ServeDir` mounts in main.rs
  (`/static`, `/js`, `/img`, `/thirdparty`, favicons) from `../static`.

## Testing & QA
- `cargo test` covers unit-level behaviour (translations loading, db helpers,
  pagination). There is no HTTP integration harness yet; when adding one,
  boot against a throwaway database, never the live `sqlite.db`.
- Coverage expectation: high on validation paths and state transitions —
  including the denial branches (wrong password, cross-user edits, bad
  upload extensions), not just happy paths.
