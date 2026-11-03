#!/usr/bin/env bash
# 构建 Linux 桌面应用：deb + AppImage。服务二进制以 bundle.resources 内置。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/linux}"

for command in cargo pnpm node; do
    command -v "$command" >/dev/null 2>&1 || { echo "缺少命令: $command" >&2; exit 1; }
done
cargo tauri --version >/dev/null 2>&1 || {
    echo "缺少 tauri-cli；请执行: cargo install tauri-cli --locked" >&2
    exit 1
}

version="$("$ROOT/scripts/check-versions.sh")"
if [[ ! -d "$ROOT/web/node_modules" ]]; then
    CI=true pnpm --dir "$ROOT/web" install --frozen-lockfile
fi
pnpm --dir "$ROOT/web" build

cd "$ROOT"
cargo build --release -p agent-beacon
TARGET="$(cargo metadata --no-deps --format-version 1 |
    node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')"
HELPER="$TARGET/release/agent-beacon"
[[ -f "$HELPER" ]] || { echo "未找到 $HELPER" >&2; exit 1; }

cp -f "$HELPER" "$ROOT/desktop/src-tauri/agent-beacon"
INJECT="$ROOT/desktop/src-tauri/.bundle-resources.json"
node -e '
const fs = require("fs")
const [src, out] = process.argv.slice(1)
fs.writeFileSync(out, JSON.stringify({ bundle: { resources: { [src]: "agent-beacon" } } }))
' "$ROOT/desktop/src-tauri/agent-beacon" "$INJECT"
cleanup() { rm -f "$ROOT/desktop/src-tauri/agent-beacon" "$INJECT"; }
trap cleanup EXIT

rm -rf "$TARGET/release/bundle/appimage" "$TARGET/release/bundle/appimage_deb" "$DIST_DIR"
mkdir -p "$DIST_DIR"

(cd "$ROOT/desktop/src-tauri" &&
    APPIMAGE_EXTRACT_AND_RUN=1 cargo tauri build --config "$INJECT" --bundles deb,appimage)

shopt -s nullglob
artifacts=("$TARGET"/release/bundle/deb/*.deb "$TARGET"/release/bundle/appimage/*.AppImage)
shopt -u nullglob
[[ ${#artifacts[@]} -gt 0 ]] || { echo "未产出 deb/AppImage" >&2; exit 1; }

for file in "${artifacts[@]}"; do
    base="$(basename "$file")"
    case "$file" in
        *.deb) cp -f "$file" "$DIST_DIR/AgentBeacon_${version}_linux.deb" ;;
        *.AppImage) cp -f "$file" "$DIST_DIR/AgentBeacon_${version}_linux.AppImage" ;;
        *) cp -f "$file" "$DIST_DIR/$base" ;;
    esac
done

echo "生成完成："
ls -l "$DIST_DIR"
