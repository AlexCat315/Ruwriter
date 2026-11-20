#!/usr/bin/env bash
# Builds a compressed macOS disk image containing the app and an
# Applications shortcut. Usage: build-dmg.sh <Ruwriter.app> <output.dmg> [volume name]
set -euo pipefail

APP="${1:?app bundle required}"
OUT="${2:?output dmg required}"
VOLUME="${3:-Ruwriter}"

test -d "$APP" || { echo "missing app bundle: $APP" >&2; exit 1; }
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

ditto "$APP" "$stage/$(basename "$APP")"
ln -s /Applications "$stage/Applications"

rm -f "$OUT"
hdiutil create \
  -volname "$VOLUME" \
  -srcfolder "$stage" \
  -ov -format UDZO \
  "$OUT" >/dev/null

test -f "$OUT" || { echo "dmg was not created" >&2; exit 1; }
