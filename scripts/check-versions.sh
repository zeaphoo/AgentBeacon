#!/usr/bin/env bash
# 版本一致性：tauri.conf.json / Cargo workspace / web package.json。
# Node 在 Windows 上不认识 Git Bash 的 /d/... 路径，先转成 Windows 路径。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

node_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$1"
    else
        printf '%s\n' "$1"
    fi
}

cargo_version="$(awk -F'"' '/^version = "/{print $2; exit}' "$ROOT/Cargo.toml")"
web_version="$(node -e 'console.log(require(process.argv[1]).version)' "$(node_path "$ROOT/web/package.json")")"
tauri_version="$(node -e 'console.log(require(process.argv[1]).version)' "$(node_path "$ROOT/desktop/src-tauri/tauri.conf.json")")"

if [[ "$cargo_version" != "$web_version" || "$cargo_version" != "$tauri_version" ]]; then
    echo "version mismatch: Cargo=$cargo_version web=$web_version tauri=$tauri_version" >&2
    exit 1
fi

echo "$cargo_version"
