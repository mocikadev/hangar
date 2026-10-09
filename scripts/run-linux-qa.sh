#!/usr/bin/env bash
# Empty configuration only. No real credential copies and no default browser launch.
set -euo pipefail
cd "$(dirname "$0")/.."
qa_home=$(mktemp -d -t hangar-linux-qa-XXXXXX)
cleanup() {
  rm -rf -- "$qa_home"
}
trap cleanup EXIT
build_log="$qa_home/build.json"
cargo build --locked -p hangar-linux --message-format=json > "$build_log"
binary=$(python3 - "$build_log" <<'PY'
import json,sys
for line in open(sys.argv[1]):
    item=json.loads(line)
    if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name']=='hangar-desktop':
        print(item['executable'])
PY
)
mkdir -m 700 "$qa_home/.codex" "$qa_home/config" "$qa_home/data" "$qa_home/cache"
printf 'Isolated empty HOME: %s\n' "$qa_home"
# Keep the desktop bus/runtime: its Settings portal owns live system preferences.
# A unique ID prevents forwarding activation to an ordinary, real-account instance.
GIO_USE_VFS=local HOME="$qa_home" CODEX_HOME="$qa_home/.codex" HANGAR_TEST_HOME="$qa_home" \
XDG_CONFIG_HOME="$qa_home/config" XDG_DATA_HOME="$qa_home/data" XDG_CACHE_HOME="$qa_home/cache" \
HANGAR_TEST_APP_ID="dev.mocika.hangar.Qa$$" \
HANGAR_TEST_TOKEN_ENDPOINT=http://127.0.0.1:9/token \
HANGAR_TEST_USAGE_ENDPOINT=http://127.0.0.1:9/usage \
  "$binary"
