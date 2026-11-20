#!/usr/bin/env bash
# Builds a Debian package from Cove's Linux application folder.
# Usage: build-deb.sh <folder with ruwriter> <version> <amd64> <output.deb>
set -euo pipefail

FOLDER="${1:?application folder required}"
VERSION="${2:?version required}"
ARCH="${3:-amd64}"
OUT="${4:?output deb required}"

test -x "$FOLDER/ruwriter" || { echo "missing executable: $FOLDER/ruwriter" >&2; exit 1; }

root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/applications" "$root/usr/share/doc/ruwriter"

install -m 755 "$FOLDER/ruwriter" "$root/usr/bin/ruwriter"
install -m 644 "$FOLDER/ruwriter.desktop" "$root/usr/share/applications/ruwriter.desktop"

cat > "$root/usr/share/applications/ruwriter.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Ruwriter
Comment=小说与文本写作
Exec=ruwriter %F
Terminal=false
Categories=Office;Utility;TextEditor;
MimeType=text/plain;text/markdown;
StartupWMClass=ruwriter
EOF

cat > "$root/DEBIAN/control" <<EOF
Package: ruwriter
Version: $VERSION
Section: editors
Priority: optional
Architecture: $ARCH
Maintainer: AlexCat315 <AlexCat315@users.noreply.github.com>
Installed-Size: $(du -sk "$root/usr" | cut -f1)
Depends: libc6, libvulkan1, libx11-6, libxkbcommon0, libwayland-client0, libxrandr2, libxi6, libxcursor1, libxss1, libasound2 | libasound2t64
Recommends: fonts-noto-cjk
Description: Ruwriter
 面向小说作者与文字工作者的跨平台纯文本写作软件。
EOF

rm -f "$OUT"
dpkg-deb --build --root-owner-group "$root" "$OUT" >/dev/null
test -f "$OUT" || { echo "deb was not created" >&2; exit 1; }
