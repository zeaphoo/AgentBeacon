#!/usr/bin/env bash
# 构建 macOS 桌面应用：AgentBeacon.app + dmg。
# 服务二进制放进 Contents/MacOS，并按「内层 helper 先签、.app 后签」顺序处理。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/mac}"
IDENTITY="${MAC_SIGN_IDENTITY:-}"

[[ "$(uname -s)" == "Darwin" ]] || { echo "package-mac.sh only supports macOS" >&2; exit 1; }
for command in cargo pnpm node codesign hdiutil; do
    command -v "$command" >/dev/null 2>&1 || { echo "missing command: $command" >&2; exit 1; }
done
cargo tauri --version >/dev/null 2>&1 || {
    echo "missing tauri-cli; run: cargo install tauri-cli --locked" >&2
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
[[ -f "$HELPER" ]] || { echo "missing $HELPER" >&2; exit 1; }

rm -rf "$TARGET/release/bundle/macos" "$DIST_DIR"
mkdir -p "$DIST_DIR"
(cd "$ROOT/desktop/src-tauri" && cargo tauri build --bundles app)

APP="$TARGET/release/bundle/macos/AgentBeacon.app"
[[ -d "$APP" ]] || { echo "missing $APP" >&2; exit 1; }

cp -f "$HELPER" "$APP/Contents/MacOS/agent-beacon"
chmod +x "$APP/Contents/MacOS/agent-beacon"

if [[ -n "$IDENTITY" ]]; then
    codesign --force --options runtime -s "$IDENTITY" "$APP/Contents/MacOS/agent-beacon"
    codesign --force --options runtime -s "$IDENTITY" "$APP"
    codesign --verify --deep --strict "$APP"
else
    echo "MAC_SIGN_IDENTITY is not set; producing an unsigned .app (local validation only)" >&2
fi

ARCH="$(uname -m)"
hdiutil create -volname AgentBeacon -srcfolder "$APP" -ov -format UDZO \
    "$DIST_DIR/AgentBeacon_${version}_macos_${ARCH}.dmg"
echo "Build complete: $DIST_DIR"
