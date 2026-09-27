#!/usr/bin/env bash
# Build the single-file release bundle: binary + runtime assets + seed.
#
# Output: cookierun-bundle.tar.gz with
#   cookierun/static/translations/seed.surql
# — everything setup.sh needs, no checkout required on the target.
#
# Options: --bin PATH, --seed PATH, -o OUT (defaults: repo release build,
# repo seed.surql, cookierun-bundle.tar.gz).
set -Eeuo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$REPO/target/x86_64-unknown-linux-gnu/release/cookierun"
SEED="$REPO/seed.surql"
OUT="cookierun-bundle.tar.gz"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --bin) BIN="$2"; shift 2 ;;
        --seed) SEED="$2"; shift 2 ;;
        -o|--output) OUT="$2"; shift 2 ;;
        -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

[ -x "$BIN" ] || { echo "binary missing: $BIN (build it first)" >&2; exit 1; }
[ -f "$SEED" ] || { echo "seed missing: $SEED (surreal export from a seeded DB)" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp "$BIN" "$STAGE/cookierun"
cp -r "$REPO/static" "$STAGE/static"
cp -r "$REPO/translations" "$STAGE/translations"
cp "$SEED" "$STAGE/seed.surql"
chmod 755 "$STAGE/cookierun"
tar czf "$OUT" -C "$STAGE" cookierun static translations seed.surql
echo "bundle: $OUT ($(du -h "$OUT" | cut -f1))"
tar tzf "$OUT" | head -4 || true