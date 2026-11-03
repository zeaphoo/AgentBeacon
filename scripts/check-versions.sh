#!/usr/bin/env bash
# 版本一致性：tauri.conf.json / Cargo workspace / web package.json。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo_version="$(awk -F'"' '/^version = "/{print $2; exit}' "$ROOT/Cargo.toml")"
web_version="$(node -p "require('$ROOT/web/package.json').version")"
tauri_version="$(node -p "require('$ROOT/desktop/src-tauri/tauri.conf.json').version")"

if [[ "$cargo_version" != "$web_version" || "$cargo_version" != "$tauri_version" ]]; then
    echo "版本不一致：Cargo=$cargo_version web=$web_version tauri=$tauri_version" >&2
    exit 1
fi

echo "$cargo_version"
