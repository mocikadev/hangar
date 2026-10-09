#!/usr/bin/env bash
# Real GTK rendering and empty-home dialog lifecycle. No real credentials.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
qa_home=$(mktemp -d -t hangar-linux-layout-XXXXXX)
cleanup() {
  fusermount3 -u -z "$qa_home/runtime/doc" 2>/dev/null || true
  fusermount3 -u -z "$qa_home/runtime/gvfs" 2>/dev/null || true
  rm -rf -- "$qa_home"
}
trap cleanup EXIT
mkdir -m 700 "$qa_home/runtime" "$qa_home/.codex"
if [[ -n "${WAYLAND_DISPLAY:-}" && "$WAYLAND_DISPLAY" != /* ]]; then
  export WAYLAND_DISPLAY="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$WAYLAND_DISPLAY"
fi
export HANGAR_LAYOUT_OUTPUT="${HANGAR_LAYOUT_OUTPUT:-$PWD/build/linux-qa}"
export HOME="$qa_home" CODEX_HOME="$qa_home/.codex" HANGAR_TEST_HOME="$qa_home"
export XDG_RUNTIME_DIR="$qa_home/runtime" GIO_USE_VFS=local
export HANGAR_TEST_TOKEN_ENDPOINT=http://127.0.0.1:9/token
export HANGAR_TEST_USAGE_ENDPOINT=http://127.0.0.1:9/usage
dbus-run-session -- cargo test --locked -p hangar-linux eight_card_layout \
  -- --ignored --test-threads=1
