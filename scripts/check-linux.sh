#!/usr/bin/env bash
# Run every write-path test with an empty isolated account/config home.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
qa_home=$(mktemp -d -t hangar-linux-check-XXXXXX)
trap 'rm -rf -- "$qa_home"' EXIT
export HOME="$qa_home" CODEX_HOME="$qa_home/.codex" HANGAR_TEST_HOME="$qa_home"
export HANGAR_TEST_TOKEN_ENDPOINT=http://127.0.0.1:9/token
export HANGAR_TEST_USAGE_ENDPOINT=http://127.0.0.1:9/usage
cargo fmt --all
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
