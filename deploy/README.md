# Deployment bundle

First-time, zero-to-running setup for the Cookie Run wiki (Rust port) on a
fresh Linux box: SurrealDB v3, seed data, admin account, and the web server.
Only `root` + `curl` are required — no python, no build tools.

## Layout

- `setup.sh` — the installer. Idempotent; safe to re-run for upgrades.
- `make_bundle.sh` — builds the single release artifact
  `cookierun-bundle.tar.gz` (binary + `static/` + `translations/` +
  `seed.surql`).
- `seed.surql` — the catalog as SurrealQL, exported from a seeded database
  with `surreal export` (2.1 MB). Regenerate after reseeding:

  ```sh
  surreal export -e http://127.0.0.1:8100 -u root -p "$PASS" \
      --ns cookierun --db cookierun seed.surql
  ```

  The import recreates the namespace/database, every table, the
  `user.user_username` unique index, and all rows — byte-for-byte what the
  seeded dev DB holds.

## Quick start

```sh
sudo ./deploy/setup.sh                # repo checkout, or:
sudo ./deploy/setup.sh --bundle cookierun-bundle.tar.gz   # any machine
```

What it does, in order:

1. **SurrealDB v3** — installs via `https://install.surrealdb.com` if
   `surreal` is not on `PATH`, then pins it to `/usr/local/bin/surreal`.
2. **Runtime tree** — from `--bundle` (tarball with binary + assets +
   seed, downloaded first if the value is a URL) or from this checkout.
   Lands in `deploy/runtime/` beside the script (override
   `--installdir` / `CR_INSTALL_DIR`). A `cookierun` system user owns it
   all. The SurrealDB root password is generated and kept in
   `/etc/cookierun/surrealdb.env` (mode 640, `root:cookierun`).
   `Config.toml` is generated on the spot (app talks to the datastore over
   `http://` — over ws, intermittent server-side session failures were
   observed in testing: a request after an idle gap intermittently 500s
   with `Session not found` / `Specify a namespace to use`. http carries
   auth + namespace per request and showed zero failures; the SDK supports
   both).
3. **Seed** — `surreal import seed.surql` only when the `cookie` table is
   empty; the import creates the namespace/database itself.
4. **Admin** — first `is_admin` user created only when the `user` table is
   empty; the password is hashed server-side with
   `crypto::argon2::generate`, the same primitive the app uses. Password in
   `/etc/cookierun/admin-credentials` (mode 600) when auto-generated.
5. **Services** — systemd units `cookierun.service` +
   `cookierun-surrealdb.service` (storage `surrealkv:///var/lib/cookierun/surreal`,
   loopback-only bind), started with `enable --now`. Use `--no-systemd`
   for containers/non-systemd hosts (backgrounded, PID files under
   `$INSTALL/run/`, logs under `$INSTALL/logs/`).
6. **Health** — waits for SurrealDB `/health` and for `HTTP 200` on the
   site, then prints the summary.

## Options

| Flag | Default | Meaning |
|---|---|---|
| `--bundle PATH/URL` | repo checkout | release tarball (binary + `static/` + `translations/` + `seed.surql`) |
| `--installdir DIR` | `deploy/runtime` | install tree (beside the script) |
| `--host HOST` | `0.0.0.0` | app bind address |
| `--port PORT` | `6785` | app port |
| `--admin-user NAME` | `admin` | first admin login |
| `--admin-pass PASS` | generated | first admin password (saved to `/etc/cookierun/admin-credentials`) |
| `--db-pass PASS` | generated | SurrealDB root password (saved to `/etc/cookierun/surrealdb.env`) |
| `--no-systemd` | off | background launches + PID files |

Environment overrides: `CR_BUNDLE`, `CR_HOST`, `CR_PORT`, `CR_NS`, `CR_DB`,
`CR_DB_BIND`, `CR_ADMIN_USER`, `CR_ADMIN_PASS`, `CR_DB_PASS`,
`CR_TURNSTILE_SECRET`, `CR_TURNSTILE_HOSTNAMES`, `CR_TRUSTED_PROXIES`,
`CR_RELEASE`.

## Production checklist

- **Turnstile is mandatory.** Release builds fail closed: login, register
  and every protected POST return 403 until `CR_TURNSTILE_SECRET` is set.
  Set `CR_TURNSTILE_HOSTNAMES` to your public domain.
- **Trusted proxies** — behind a CDN/nginx, set `CR_TRUSTED_PROXIES` to the
  proxy IPs, or every visitor shares one rate-limit bucket (the limiter keys
  on the TCP peer; forwarded headers are only honored from trusted peers).
- The app binds `$HOST` directly; front it with nginx/TLS if not exposed via
  a CDN. Cookies are `Secure`, so plain HTTP only works on loopback.
- Data lives in `/var/lib/cookierun/surreal` — back it up. Restart the
  service after restoring.

## Manual install (no script)

Same end state as `setup.sh`, by hand — equivalent commands (root shell):

```sh
curl -sSf https://install.surrealdb.com | sh          # once, if surreal absent
mkdir -p /opt/cookierun
curl -sSfL https://github.com/Lebenoa/cr-wiki/releases/download/v0.1.2/cookierun-bundle.tar.gz \
  | tar xz -C /opt/cookierun
useradd --system --home-dir /opt/cookierun --shell /usr/sbin/nologin cookierun
mkdir -p /var/lib/cookierun/surreal /etc/cookierun
DB_PASS="$(openssl rand -hex 24)"
printf 'SURREAL_USER=root\nSURREAL_PASS=%s\nSURREAL_NO_BANNER=true\n' "$DB_PASS" \
  > /etc/cookierun/surrealdb.env && chmod 640 /etc/cookierun/surrealdb.env
chown -R cookierun:cookierun /opt/cookierun /var/lib/cookierun
# write /opt/cookierun/Config.toml (host/port/[surreal] incl. password/[turnstile])
sudo -u cookierun env SURREAL_USER=root SURREAL_PASS="$DB_PASS" \
  surreal start --bind 127.0.0.1:8100 surrealkv:///var/lib/cookierun/surreal
surreal import -e http://127.0.0.1:8100 -u root -p "$DB_PASS" \
  --ns cookierun --db cookierun /opt/cookierun/seed.surql
ADMIN_PASS="$(openssl rand -hex 12)"
printf "CREATE type::record('user', 1) SET username = 'admin', \
password = crypto::argon2::generate('%s'), is_admin = true, \
created_at = time::unix();" "$ADMIN_PASS" \
  | surreal sql -e http://127.0.0.1:8100 -u root -p "$DB_PASS" \
      --ns cookierun --db cookierun --hide-welcome
cd /opt/cookierun && sudo -u cookierun ./cookierun
```

`setup.sh` generates the `Config.toml`; manually, write it yourself (see the
root README for the full annotated version; `[turnstile]` empty = fail
closed). For production, replace the last line with the two units below.

## systemd units (manual install)

`/etc/systemd/system/cookierun-surrealdb.service`:

```ini
[Unit]
Description=CookieRun wiki SurrealDB v3 datastore
After=network.target

[Service]
Type=simple
User=cookierun
Group=cookierun
EnvironmentFile=/etc/cookierun/surrealdb.env
ExecStart=/usr/local/bin/surreal start --bind 127.0.0.1:8100 --log info surrealkv:///var/lib/cookierun/surreal
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ReadWritePaths=/var/lib/cookierun/surreal

[Install]
WantedBy=multi-user.target
```

`/etc/systemd/system/cookierun.service`:

```ini
[Unit]
Description=CookieRun wiki web server
After=network.target cookierun-surrealdb.service
Requires=cookierun-surrealdb.service

[Service]
Type=simple
User=cookierun
Group=cookierun
WorkingDirectory=/opt/cookierun
ExecStart=/opt/cookierun/cookierun
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true

[Install]
WantedBy=multi-user.target
```

```sh
sudo systemctl daemon-reload && sudo systemctl enable --now cookierun-surrealdb.service cookierun.service
```

## Ops

```sh
journalctl -u cookierun -f          # app logs
journalctl -u cookierun-surrealdb   # database logs
sudo systemctl restart cookierun    # restart the site (sessions are in-memory)
```

**Reseed** (wipes every seeded table, including `user`):

```sh
sudo surreal import -e http://127.0.0.1:8100 -u root -p "$(grep SURREAL_PASS /etc/cookierun/surrealdb.env | cut -d= -f2)" \
    --ns cookierun --db cookierun deploy/runtime/seed.surql
```

## Build

```sh
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.43
./deploy/make_bundle.sh            # -> cookierun-bundle.tar.gz
```

glibc ≥ 2.39 at runtime; zig 0.16.0 as the linker. The bundle is the
install artifact — `setup.sh --bundle` needs nothing else.