#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
target_dir=${CARGO_TARGET_DIR:-"$repo_root/target"}
case "$target_dir" in
    /*) ;;
    *) target_dir="$repo_root/$target_dir" ;;
esac
output_dir=${1:-"$repo_root/build/generated/swift"}
profile=${HANGAR_UNIFFI_PROFILE:-release}

cd "$repo_root"
case "$profile" in
    debug)
        cargo build -p hangar-uniffi --locked
        profile_dir=debug
        ;;
    release)
        cargo build --release -p hangar-uniffi --locked
        profile_dir=release
        ;;
    *)
        echo "不支持的 HANGAR_UNIFFI_PROFILE: $profile" >&2
        exit 2
        ;;
esac
cargo run -p hangar-uniffi --features bindgen --bin uniffi-bindgen-swift --locked -- \
    "$target_dir/$profile_dir/libhangar_uniffi.a" \
    "$output_dir" \
    --swift-sources \
    --headers \
    --modulemap \
    --module-name HangarCoreFFI \
    --modulemap-filename module.modulemap
