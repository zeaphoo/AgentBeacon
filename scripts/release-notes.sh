#!/usr/bin/env bash
# Build a bilingual GitHub Release body from CHANGELOG.md and CHANGELOG.zh-CN.md.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:?usage: release-notes.sh <version>}"
HEADING="## [${VERSION}]"

extract_section() {
    local file="$1"
    awk -v heading="$HEADING" '
        index($0, heading) == 1 { found = 1; next }
        found && /^## \[/ { exit }
        found && /^\[[^]]+\]: / { exit }
        found {
            if (!started && $0 ~ /^[[:space:]]*$/) next
            started = 1
            print
        }
    ' "$file"
}

ENGLISH="$(extract_section "$ROOT/CHANGELOG.md")"
CHINESE="$(extract_section "$ROOT/CHANGELOG.zh-CN.md")"

if [[ -z "$ENGLISH" || -z "$CHINESE" ]]; then
    echo "no changelog entry found for ${VERSION}" >&2
    exit 1
fi

cat <<EOF
## English

${ENGLISH}

## 中文

${CHINESE}

**Full Changelog**: https://github.com/zeaphoo/AgentBeacon/commits/v${VERSION}
EOF
