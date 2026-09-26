#!/usr/bin/env bash
# First-time installer for the Cookie Run wiki (Rust port).
#
# Takes a fresh Linux box to a running wiki in one shot:
#   1. installs SurrealDB v3 (official install script) if missing
#   2. installs the cookierun release binary (repo build, prebuilt copy, or
#      downloaded from the GitHub release matching CR_RELEASE / latest)
#   3. creates the runtime user, install tree, Config.toml and a generated
#      database password
#   4. (systemd mode, default) installs two units — cookierun-surrealdb and
#      cookierun — and starts them;  (--no-systemd) backgrounds both with
#      PID files instead, for containers and non-systemd hosts
#   5. imports scripts/seed_data.json when the cookie table is empty,
#      creates the app's indexes, and bootstraps the first admin user
#   6. waits for the site to answer and prints the summary
#
# Idempotent: re-running upgrades the binary/config assets, reuses the
# database password and skips seeding/admin creation once data exists.
#
# Usage:  sudo ./setup.sh [options]
# Options:
#   --installdir DIR   install tree (default deploy/runtime beside this script)
#   --host HOST        app bind host (default 0.0.0.0)
#   --port PORT        app port (default 6785)
#   --admin-user NAME  first admin login (default admin)
#   --admin-pass PASS  first admin password (random if omitted, saved to
#                      /etc/cookierun/admin-credentials)
#   --db-pass PASS     SurrealDB root password (random if omitted, saved to
#                      /etc/cookierun/surrealdb.env)
#   --no-systemd       background both processes instead of systemd units
#
# Environment overrides: CR_INSTALL_DIR, CR_HOST, CR_PORT, CR_NS, CR_DB,
#   CR_ADMIN_USER, CR_ADMIN_PASS, CR_DB_PASS, CR_TURNSTILE_SECRET,
#   CR_TURNSTILE_HOSTNAMES, CR_TRUSTED_PROXIES (space/comma separated),
#   CR_RELEASE (GitHub release tag to download the binary from), CR_DB_BIND.
set -Eeuo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Default install tree sits beside the script (deploy/runtime) so the whole
# bundle stays self-contained; override with --installdir or CR_INSTALL_DIR.
INSTALL="${CR_INSTALL_DIR:-$SCRIPT_DIR/runtime}"
HOST="${CR_HOST:-0.0.0.0}"
PORT="${CR_PORT:-6785}"
NS="${CR_NS:-cookierun}"
DB="${CR_DB:-cookierun}"
DB_BIND="${CR_DB_BIND:-127.0.0.1:8100}"
ADMIN_USER="${CR_ADMIN_USER:-admin}"
ADMIN_PASS="${CR_ADMIN_PASS:-}"
DB_PASS="${CR_DB_PASS:-}"
RELEASE="${CR_RELEASE:-latest}"
SYSTEMD=1

while [ "$#" -gt 0 ]; do
    case "$1" in
        --installdir) INSTALL="$2"; shift 2 ;;
        --host) HOST="$2"; shift 2 ;;
        --port) PORT="$2"; shift 2 ;;
        --admin-user) ADMIN_USER="$2"; shift 2 ;;
        --admin-pass) ADMIN_PASS="$2"; shift 2 ;;
        --db-pass) DB_PASS="$2"; shift 2 ;;
        --no-systemd) SYSTEMD=0; shift ;;
        -h|--help)
            sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

log() { printf '\033[1;32m[setup]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[setup] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" = 0 ] || die "run as root (sudo ./setup.sh)"

# --- 1. SurrealDB -----------------------------------------------------------
if ! command -v surreal >/dev/null 2>&1; then
    log "SurrealDB not found; installing via install.surrealdb.com ..."
    curl -sSf https://install.surrealdb.com | sh || die "surrealdb install failed"
    SRC="$(command -v surreal 2>/dev/null || echo "$HOME/.surrealdb/surreal")"
    [ -x "$SRC" ] || die "surrealdb installed somewhere unexpected: $SRC"
    cp "$SRC" /usr/local/bin/surreal
    chmod 755 /usr/local/bin/surreal
fi
log "SurrealDB: $(surreal version | head -1)"

# --- 2. binary ---------------------------------------------------------------
BIN_CANDIDATES=(
    "$REPO/target/x86_64-unknown-linux-gnu/release/cookierun"
    "$INSTALL/cookierun"
)
BIN=""
for c in "${BIN_CANDIDATES[@]}"; do
    if [ -x "$c" ]; then BIN="$c"; break; fi
done
if [ -z "$BIN" ]; then
    log "no local binary; downloading cookierun $RELEASE from GitHub ..."
    API="https://api.github.com/repos/Lebenoa/cr-wiki/releases/$RELEASE"
    ASSET="$(curl -sSf "$API" | grep -oE 'https://[^"]*cookierun-x86_64-unknown-linux-gnu[^"]*' | head -1)"
    [ -n "$ASSET" ] || die "no release asset found for $RELEASE (build first: cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.43)"
    curl -sSfL -o /tmp/cookierun-bundle "$ASSET" || die "binary download failed"
    mkdir -p "$INSTALL"
    install -m 755 /tmp/cookierun-bundle "$INSTALL/cookierun"
    rm -f /tmp/cookierun-bundle
    BIN="$INSTALL/cookierun"
fi
log "binary: $BIN"

# --- 3. install tree, credential files --------------------------------------
mkdir -p "$INSTALL" /etc/cookierun /var/lib/cookierun/surreal
if ! id -u cookierun >/dev/null 2>&1; then
    useradd --system --home-dir "$INSTALL" --shell /usr/sbin/nologin cookierun
fi

[ -s /etc/cookierun/surrealdb.env ] || [ -n "$DB_PASS" ] || \
    DB_PASS="$(openssl rand -hex 24 2>/dev/null || od -An -N24 -tx1 /dev/urandom | tr -d ' \n')"
umask 077
if [ ! -s /etc/cookierun/surrealdb.env ]; then
    printf 'SURREAL_USER=root\nSURREAL_PASS=%s\nSURREAL_NO_BANNER=true\n' "$DB_PASS" \
        > /etc/cookierun/surrealdb.env
fi
chown root:cookierun /etc/cookierun/surrealdb.env
chmod 640 /etc/cookierun/surrealdb.env

# --- 4. install tree contents ------------------------------------------------
if [ "$BIN" != "$INSTALL/cookierun" ]; then
    install -m 755 "$BIN" "$INSTALL/cookierun"
fi
mkdir -p "$INSTALL/scripts"
cp -r "$REPO/translations" "$INSTALL/translations"
rm -rf "$INSTALL/static"
cp -r "$REPO/static" "$INSTALL/static"
cp "$REPO/scripts/seed_data.json" "$INSTALL/scripts/seed_data.json"
cp "$REPO/deploy/import_seed.py" "$INSTALL/import_seed.py"
chmod 644 "$INSTALL/import_seed.py"

# Config.toml: host/port, the datastore, turnstile, trusted proxies.
CONF="$INSTALL/Config.toml"
CONF_PASS="$(sed -n 's/^SURREAL_PASS=//p' /etc/cookierun/surrealdb.env)"
cat > "$CONF" <<EOF
host = "$HOST"
port = $PORT

[surreal]
url = "http://$DB_BIND"
namespace = "$NS"
database = "$DB"
username = "root"
password = "$CONF_PASS"

[turnstile]
secret = "${CR_TURNSTILE_SECRET:-}"
hostnames = "${CR_TURNSTILE_HOSTNAMES:-}"
EOF
if [ -n "${CR_TRUSTED_PROXIES:-}" ]; then
    PROXIES="$(printf '%s\n' "${CR_TRUSTED_PROXIES//,/ }" | tr ' ' '\n' | sed '/^$/d' | sed 's/.*/"&"/' | paste -sd, -)"
    printf '\n[ratelimit]\ntrusted_proxies = [%s]\n' "$PROXIES" >> "$CONF"
fi
chown -R cookierun:cookierun "$INSTALL"
chown -R cookierun:cookierun /var/lib/cookierun

# --- 5. services --------------------------------------------------------------
export IMPORT_URL="http://$DB_BIND" IMPORT_NS="$NS" IMPORT_DB="$DB" \
       IMPORT_USER=root IMPORT_PASS="$(sed -n 's/^SURREAL_PASS=//p' /etc/cookierun/surrealdb.env)"

surreal_start() {
    env SURREAL_USER=root \
        SURREAL_PASS="$(sed -n 's/^SURREAL_PASS=//p' /etc/cookierun/surrealdb.env)" \
        SURREAL_NO_BANNER=true \
        /usr/local/bin/surreal start --bind "$DB_BIND" \
        "surrealkv:///var/lib/cookierun/surreal" "$@"
}
wait_db() {
    for _ in $(seq 1 30); do
        curl -sf -m 2 -o /dev/null "http://$DB_BIND/health" && return 0
        sleep 1
    done
    return 1
}
wait_site() {
    for _ in $(seq 1 30); do
        curl -sf -m 2 -o /dev/null "http://127.0.0.1:$PORT/" && return 0
        sleep 1
    done
    return 1
}

if [ "$SYSTEMD" = 1 ]; then
    cat > /etc/systemd/system/cookierun-surrealdb.service <<EOF
[Unit]
Description=CookieRun wiki SurrealDB v3 datastore
After=network.target

[Service]
Type=simple
User=cookierun
Group=cookierun
EnvironmentFile=/etc/cookierun/surrealdb.env
ExecStart=/usr/local/bin/surreal start --bind $DB_BIND --log info surrealkv:///var/lib/cookierun/surreal
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ReadWritePaths=/var/lib/cookierun/surreal

[Install]
WantedBy=multi-user.target
EOF
    cat > /etc/systemd/system/cookierun.service <<EOF
[Unit]
Description=CookieRun wiki web server
After=network.target cookierun-surrealdb.service
Requires=cookierun-surrealdb.service

[Service]
Type=simple
User=cookierun
Group=cookierun
WorkingDirectory=$INSTALL
ExecStart=$INSTALL/cookierun
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
    log "starting cookierun-surrealdb.service ..."
    systemctl enable --now cookierun-surrealdb.service >/dev/null 2>&1
    wait_db || { journalctl -u cookierun-surrealdb.service -n 20 --no-pager >&2; die "SurrealDB did not become healthy"; }

    log "seeding and bootstrapping admin ..."
    python3 "$INSTALL/import_seed.py" --fixture "$INSTALL/scripts/seed_data.json" --if-empty --ensure-indexes \
        --create-admin-if-empty "$ADMIN_USER" "$ADMIN_PASS"

    log "starting cookierun.service ..."
    systemctl enable --now cookierun.service >/dev/null 2>&1
    wait_site || { journalctl -u cookierun.service -n 20 --no-pager >&2; die "site did not answer on :$PORT"; }
else
    mkdir -p "$INSTALL/run" "$INSTALL/logs"
    log "starting SurrealDB (no-systemd) ..."
    surreal_start >> "$INSTALL/logs/surrealdb.log" 2>&1 &
    echo $! > "$INSTALL/run/surrealdb.pid"
    wait_db || die "SurrealDB did not become healthy (see $INSTALL/logs/surrealdb.log)"

    log "seeding and bootstrapping admin ..."
    python3 "$INSTALL/import_seed.py" --fixture "$INSTALL/scripts/seed_data.json" --if-empty --ensure-indexes \
        --create-admin-if-empty "$ADMIN_USER" "$ADMIN_PASS"

    log "starting cookierun (no-systemd) ..."
    cd "$INSTALL"
    "$INSTALL/cookierun" >> "$INSTALL/logs/cookierun.log" 2>&1 &
    echo $! > "$INSTALL/run/cookierun.pid"
    cd "$REPO"
    wait_site || die "site did not answer on :$PORT (see $INSTALL/logs/cookierun.log)"
fi

# --- 6. admin credentials + summary -------------------------------------------
if [ -z "$ADMIN_PASS" ] && [ ! -s /etc/cookierun/admin-credentials ]; then
    ADMIN_PASS="$(openssl rand -hex 12 2>/dev/null || od -An -N12 -tx1 /dev/urandom | tr -d ' \n')"
    {
        printf 'username: %s\n' "$ADMIN_USER"
        printf 'password: %s\n' "$ADMIN_PASS"
    } > /etc/cookierun/admin-credentials
    chmod 600 /etc/cookierun/admin-credentials
fi

log "done"
echo
echo "------------------------------------------------------------"
echo "  Cookie Run wiki is live at  http://$HOST:$PORT"
echo "  Admin login: $ADMIN_USER  (password in /etc/cookierun/admin-credentials)"
echo "  SurrealDB root password: /etc/cookierun/surrealdb.env"
if [ -z "${CR_TURNSTILE_SECRET:-}" ]; then
    echo "  WARNING: TURNSTILE_SECRET not set — release builds fail closed:"
    echo "           login/register/admin forms return 403 until you set"
    echo "           CR_TURNSTILE_SECRET (Cloudflare dashboard) and rerun."
fi
echo "  Logs:  journalctl -u cookierun -f"
echo "  Reseed (destructive):  sudo python3 $INSTALL/import_seed.py"
echo "------------------------------------------------------------"