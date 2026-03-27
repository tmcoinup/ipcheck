#!/usr/bin/env bash
# Windows：优先本机已构建的 target/release/ipcheck.exe；否则尝试 gnu 交叉编译。
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${ROOT}/dist"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ZIP_NAME="ipcheck-windows-x86_64-${VERSION}.zip"
EXE=""

if [[ -f "${ROOT}/target/release/ipcheck.exe" ]]; then
  EXE="${ROOT}/target/release/ipcheck.exe"
  echo "Using existing: $EXE"
elif rustup target list --installed 2>/dev/null | grep -q 'x86_64-pc-windows-gnu'; then
  echo "Cross-building for x86_64-pc-windows-gnu ..."
  cargo build --release --target x86_64-pc-windows-gnu
  EXE="${ROOT}/target/x86_64-pc-windows-gnu/release/ipcheck.exe"
else
  echo "未找到 Windows 可执行文件。"
  echo "请在 Windows 上执行: cargo build --release"
  echo "或安装交叉编译: rustup target add x86_64-pc-windows-gnu 及对应 mingw 工具链后再运行本脚本。"
  exit 1
fi

mkdir -p "$OUT_DIR"
rm -f "${OUT_DIR}/${ZIP_NAME}"
zip -j "${OUT_DIR}/${ZIP_NAME}" "$EXE" "${ROOT}/README.md"
echo "Built: ${OUT_DIR}/${ZIP_NAME}"
