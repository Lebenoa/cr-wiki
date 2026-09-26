# Deployment bundle

First-time, zero-to-running setup for the Cookie Run wiki (Rust port) on a
fresh Linux box: SurrealDB v3, seed data, admin account, and the web server.

## Files

- `setup.sh` — the installer. Idempotent; safe to re-run for upgrades.
- `import_seed.py` — devops copy of the seed importer (`scripts/import_seed.py`
  stays untracked scraper tooling; this one is committed). Adds
  `--if-empty`, `--ensure-indexes`, `--create-admin[-if-empty]`.
- `cookierun.service` / `cookierun-surrealdb.service` — templates written to
  `/etc/systemd/system/` by `setup.sh` (systemd mode).

## Quick start

```sh
sudo ./deploy/setup.sh
```

What it does, in order:

1. **SurrealDB v3** — installs via `https://install.surrealdb.com` if
   `surreal` is not on `PATH`, then pins it to `/usr/local/bin/surreal`.
2. **Binary** — uses `target/x86_64-unknown-linux-gnu/release/cookierun` from
   this checkout, an existing `deploy/runtime/cookierun`, or downloads the
   `cookierun-x86_64-unknown-linux-gnu` asset from the GitHub release
   (`CR_RELEASE` selects the tag; default latest).
3. **Install tree** — `deploy/runtime/` (override `--installdir`) gets the
   binary, `translations/`, `static/`, `scripts/seed_data.json`,
   `import_seed.py` and a generated `Config.toml`. The app talks to the
   datastore over `http://` — observed stable with SurrealDB 3.3.0-beta
   servers, where the websocket session can go stale after idle gaps (the
   SDK supports both transports). A `cookierun` system user owns it all.
   The SurrealDB root password is generated and kept in
   `/etc/cookierun/surrealdb.env` (mode 640, `root:cookierun`).
4. **Services** —
   - systemd (default): units `cookierun-surrealdb.service` (storage under
     `/var/lib/cookierun/surreal`, surrealkv engine, loopback-only bind) and
     `cookierun.service`, both `enable --now`.
   - `--no-systemd`: both run backgrounded with PID files under
     `$INSTALL/run/` and logs under `$INSTALL/logs/` — for containers and
     non-systemd hosts. Not for production.
5. **Data** — `import_seed.py --if-empty --ensure-indexes
   --create-admin-if-empty <user> <pass>`:
   - seeds `scripts/seed_data.json` only when the `cookie` table is empty;
   - creates the unique index on `user.username` the app relies on;
   - creates the first `is_admin` user (password hashed server-side by
     SurrealDB's `crypto::argon2::generate`, the same primitive the app
     uses), only when no users exist.
6. **Health** — waits for SurrealDB `/health` and for `HTTP 200` on the site,
   then prints the summary.

## Options

| Flag | Default | Meaning |
|---|---|---|
| `--installdir DIR` | `deploy/runtime` | install tree |
| `--host HOST` | `0.0.0.0` | app bind address |
| `--port PORT` | `6785` | app port |
| `--admin-user NAME` | `admin` | first admin login |
| `--admin-pass PASS` | generated | first admin password (saved to `/etc/cookierun/admin-credentials`, mode 600) |
| `--db-pass PASS` | generated | SurrealDB root password (saved to `/etc/cookierun/surrealdb.env`) |
| `--no-systemd` | off | background launches + PID files |

Environment overrides: `CR_HOST`, `CR_PORT`, `CR_NS`, `CR_DB`, `CR_DB_BIND`,
`CR_ADMIN_USER`, `CR_ADMIN_PASS`, `CR_DB_PASS`, `CR_TURNSTILE_SECRET`,
`CR_TURNSTILE_HOSTNAMES`, `CR_TRUSTED_PROXIES`, `CR_RELEASE`.

## Production checklist

- **Turnstile is mandatory.** Release builds fail closed: login, register
  and every protected POST return 403 until `CR_TURNSTILE_SECRET` is set.
  The setup script prints a reminder when it is not. Set
  `CR_TURNSTILE_HOSTNAMES` to your public domain.
- **Trusted proxies** — behind a CDN/nginx, set `CR_TRUSTED_PROXIES` to the
  proxy IPs, or every visitor shares one rate-limit bucket (the limiter keys
  on the TCP peer; forwarded headers are only honored from trusted peers).
- The app binds `$HOST` directly; front it with nginx/TLS if not exposed via
  a CDN. Cookies are `Secure`, so plain HTTP only works on loopback.
- Data lives in `/var/lib/cookierun/surreal` — back it up. Restart the
  service after restoring.

## Ops

```sh
journalctl -u cookierun -f          # app logs
journalctl -u cookierun-surrealdb   # database logs
sudo systemctl restart cookierun    # restart the site (sessions are in-memory)
```

**Reseed** (wipes every seeded table, including `user`):

```sh
sudo python3 deploy/runtime/import_seed.py
```

With `--if-empty` it is a no-op when data exists.

## Build

```sh
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.43
```

glibc ≥ 2.39 at runtime; zig 0.16.0 as the linker. The release asset name is
`cookierun-x86_64-unknown-linux-gnu`, which `setup.sh` knows how to fetch.