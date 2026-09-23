#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
source_icon="$repo_root/resources/shared/hangar-icon.svg"
icon_set="$repo_root/apps/macos/HangarMac/Assets.xcassets/AppIcon.appiconset"
dock_icon_set="$repo_root/apps/macos/HangarMac/Assets.xcassets/HangarDockIcon.imageset"
master_icon="$icon_set/icon_512x512@2x.png"

if ! command -v sips >/dev/null 2>&1; then
    echo "需要 macOS 的 sips 才能导出 AppIcon。" >&2
    exit 1
fi

# 先从矢量源渲染 1024px 母图，再缩小；直接以 16/32/64px 渲染 SVG 会让边缘采样过硬。
sips -s format png -Z 1024 "$source_icon" --out "$master_icon" >/dev/null
cp "$master_icon" "$dock_icon_set/HangarDockIcon.png"

for entry in \
    icon_16x16.png:16 \
    icon_16x16@2x.png:32 \
    icon_32x32.png:32 \
    icon_32x32@2x.png:64 \
    icon_128x128.png:128 \
    icon_128x128@2x.png:256 \
    icon_256x256.png:256 \
    icon_256x256@2x.png:512 \
    icon_512x512.png:512
do
    filename=${entry%:*}
    pixels=${entry#*:}
    sips -s format png -Z "$pixels" "$master_icon" --out "$icon_set/$filename" >/dev/null
done
