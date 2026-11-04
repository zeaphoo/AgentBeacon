#!/usr/bin/env bash
# 构建 Windows 桌面应用：NSIS / MSI + 便携 zip（两个 exe 同目录）。
# 需在 Git Bash / MSYS 中运行。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT/dist/windows}"
BUNDLES="${TAURI_BUNDLES:-nsis,msi}"

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *) echo "package-windows.sh must run on Windows (Git Bash / MSYS)" >&2; exit 1 ;;
esac

for command in cargo pnpm node; do
    command -v "$command" >/dev/null 2>&1 || { echo "missing command: $command" >&2; exit 1; }
done
cargo tauri --version >/dev/null 2>&1 || {
    echo "missing tauri-cli; run: cargo install tauri-cli --locked" >&2
    exit 1
}

version="$("$ROOT/scripts/check-versions.sh")"
if [[ ! -d "$ROOT/web/node_modules" ]]; then
    (cd "$ROOT/web" && CI=true pnpm install --frozen-lockfile)
fi
(cd "$ROOT/web" && pnpm build)

cd "$ROOT"
cargo build --release -p agent-beacon
TARGET="$(cargo metadata --no-deps --format-version 1 |
    node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).target_directory))')"
HELPER="$TARGET/release/agent-beacon.exe"
APP_EXE="$TARGET/release/AgentBeaconApp.exe"
[[ -f "$HELPER" ]] || { echo "missing $HELPER" >&2; exit 1; }

cp -f "$HELPER" "$ROOT/desktop/src-tauri/agent-beacon.exe"
INJECT="$ROOT/desktop/src-tauri/.bundle-resources.json"
node -e '
const fs = require("fs")
const [src, out] = process.argv.slice(1)
fs.writeFileSync(out, JSON.stringify({ bundle: { resources: { [src]: "agent-beacon.exe" } } }))
' "$(cygpath -w "$ROOT/desktop/src-tauri/agent-beacon.exe")" "$(cygpath -w "$INJECT")"
cleanup() { rm -f "$ROOT/desktop/src-tauri/agent-beacon.exe" "$INJECT"; }
trap cleanup EXIT

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/portable"
(cd "$ROOT/desktop/src-tauri" && cargo tauri build --config "$(cygpath -w "$INJECT")" --bundles "$BUNDLES")

# The desktop app executable is produced by `cargo tauri build`; it does not
# exist after the service-only `cargo build -p agent-beacon` above.
[[ -f "$APP_EXE" ]] || { echo "missing $APP_EXE" >&2; exit 1; }

cp -f "$APP_EXE" "$DIST_DIR/portable/AgentBeaconApp.exe"
cp -f "$HELPER" "$DIST_DIR/portable/agent-beacon.exe"
cat > "$DIST_DIR/portable/README.txt" <<TXT
AgentBeacon ${version} 便携版
双击 AgentBeaconApp.exe 启动；agent-beacon.exe 必须与它在同一目录。
托盘菜单可打开控制台、启停服务、关闭灯效与设置开机自启动。
TXT

powershell.exe -NoProfile -NonInteractive -Command \
    "Compress-Archive -Path '$(cygpath -w "$DIST_DIR/portable")\\*' -DestinationPath '$(cygpath -w "$DIST_DIR/AgentBeacon_${version}_windows_portable.zip")' -Force" >/dev/null

shopt -s nullglob
installers=("$TARGET"/release/bundle/nsis/*.exe "$TARGET"/release/bundle/msi/*.msi)
shopt -u nullglob
for file in "${installers[@]}"; do
    cp -f "$file" "$DIST_DIR/$(basename "$file")"
done

echo "Build complete:"
ls -l "$DIST_DIR"
