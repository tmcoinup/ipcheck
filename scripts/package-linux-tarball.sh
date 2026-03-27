#!/usr/bin/env bash
# 生成 Linux x86_64 二进制 tar.gz（不依赖 dpkg）
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo build --release

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
OUT_DIR="${ROOT}/dist"
STAGE="${OUT_DIR}/ipcheck-linux-x86_64-${VERSION}"
ARCHIVE="ipcheck-linux-x86_64-${VERSION}.tar.gz"

rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "${ROOT}/target/release/ipcheck" "$STAGE/"
cp "${ROOT}/README.md" "$STAGE/"
chmod +x "$STAGE/ipcheck"

mkdir -p "$OUT_DIR"
tar -C "${OUT_DIR}" -czf "${OUT_DIR}/${ARCHIVE}" "$(basename "$STAGE")"
echo "Built: ${OUT_DIR}/${ARCHIVE}"
