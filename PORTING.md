# Rust port

**The port is complete: the V sources were removed from the tree after the
final review round (their history lives in git). This file remains as the
map from old to new.** The app reads `Config.toml`, `translations/` and
`static/` from the repo root, and talks to an external SurrealDB v3 server
configured in `[surreal]`; run it there with `cargo run`. `cargo test`
needs none of those: it exercises the parsing, paging, queries and
templates directly.

Post-removal hardening that originated on the V side and was carried here:
sessions expire after 7 days (swept on access), the loopback admin bypass is
debug-builds only, rate-limit buckets are keyed on the TCP peer unless it is
a configured trusted proxy, and graded simple catalogs (ingredients, skins)
order by display rank rather than raw grade.


## Stack

| concern | choice |
| --- | --- |
| HTTP | axum + tokio |
| templates | askama |
| storage | SurrealDB v3 over the wire; the app does not embed a storage engine |
| config | serde + toml |
| i18n | hand-rolled `.tr` loader |


