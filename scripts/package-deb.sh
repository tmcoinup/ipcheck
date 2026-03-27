#!/usr/bin/env bash
# 在 Ubuntu/Debian 上构建 amd64 .deb（需 dpkg-deb、已安装 Rust）
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo build --release

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
OUT_DIR="${ROOT}/dist"
PKG_ROOT="${OUT_DIR}/ipcheck-deb-root"
DEB_NAME="ipcheck_${VERSION}_amd64.deb"

rm -rf "$PKG_ROOT"
mkdir -p "$PKG_ROOT/DEBIAN"
mkdir -p "$PKG_ROOT/usr/bin"
mkdir -p "$PKG_ROOT/usr/share/applications"

cp "${ROOT}/target/release/ipcheck" "$PKG_ROOT/usr/bin/ipcheck"
chmod 755 "$PKG_ROOT/usr/bin/ipcheck"
cp "${ROOT}/packaging/debian/ipcheck.desktop" "$PKG_ROOT/usr/share/applications/ipcheck.desktop"

cat > "$PKG_ROOT/DEBIAN/control" << EOF
Package: ipcheck
Version: ${VERSION}
Section: net
Priority: optional
Architecture: amd64
Maintainer: Unspecified Maintainer
Description: SOCKS5 IP quality and risk check (desktop)
 IP quality detection tool using Iced GUI; optional zenity for import dialog.
Depends: libc6 (>= 2.31), zenity
EOF

mkdir -p "$OUT_DIR"
rm -f "${OUT_DIR}/${DEB_NAME}"
dpkg-deb --root-owner-group --build "$PKG_ROOT" "${OUT_DIR}/${DEB_NAME}"
echo "Built: ${OUT_DIR}/${DEB_NAME}"
echo "Install: sudo apt install ./${DEB_NAME}   # or dpkg -i"
