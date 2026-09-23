#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
app_bundle="$repo_root/build/xcode/Build/Products/Debug/Hangar.app"
app_binary="$app_bundle/Contents/MacOS/Hangar"
source_accounts="$HOME/.hangar/accounts.json"

if [ ! -x "$app_binary" ]; then
    echo "Hangar.app 尚未构建，请先运行文档中的 xcodebuild 命令。" >&2
    exit 1
fi
if [ ! -f "$source_accounts" ]; then
    echo "未找到 $source_accounts，无法创建真实账号隔离副本。" >&2
    exit 1
fi

qa_home=$(mktemp -d /tmp/hangar-native-qa.XXXXXX)
cleanup() {
    if [ -n "$qa_home" ] && [ -d "$qa_home" ]; then
        find "$qa_home" -depth -delete
    fi
}
trap cleanup EXIT INT TERM

mkdir -p "$qa_home/.hangar" "$qa_home/.codex"
cp "$source_accounts" "$qa_home/.hangar/accounts.json"
printf '%s\n' 'cli_auth_credentials_store = "file"' > "$qa_home/.codex/config.toml"
chmod 700 "$qa_home/.hangar" "$qa_home/.codex"
chmod 600 "$qa_home/.hangar/accounts.json" "$qa_home/.codex/config.toml"

echo "Hangar 使用隔离目录和 file 凭据存储启动；不会访问 macOS 钥匙串。请从菜单栏选择“退出 Hangar”，退出后自动删除：$qa_home"
if pgrep -x Hangar >/dev/null 2>&1; then
    echo "已有 Hangar 进程，请先从菜单栏退出后再运行 QA。" >&2
    exit 1
fi

# LaunchServices 必须从完整 .app 启动，才能加载 AppIcon 与 Asset Catalog。
# `open --env` 只把隔离路径注入本次进程，不修改当前用户的全局 launchd 环境。
open -n -F \
    --env "HANGAR_TEST_HOME=$qa_home" \
    --env "CODEX_HOME=$qa_home/.codex" \
    -o "$qa_home/app.stdout.log" \
    --stderr "$qa_home/app.stderr.log" \
    "$app_bundle" --args "$@"

app_pid=""
attempt=0
while [ "$attempt" -lt 50 ]; do
    app_pid=$(pgrep -x Hangar | head -n 1 || true)
    [ -n "$app_pid" ] && break
    attempt=$((attempt + 1))
    sleep 0.1
done
if [ -z "$app_pid" ]; then
    echo "Hangar.app 未能通过 LaunchServices 启动。" >&2
    exit 1
fi
while kill -0 "$app_pid" 2>/dev/null; do
    sleep 1
done

if [ -f "$qa_home/.codex/auth.json" ]; then
    cli_binary="$repo_root/target/debug/hangar"
    if [ ! -x "$cli_binary" ]; then
        (cd "$repo_root" && cargo build -p hangar --locked)
    fi
    doctor_output=$(env HANGAR_TEST_HOME="$qa_home" CODEX_HOME="$qa_home/.codex" "$cli_binary" doctor 2>&1 || true)
    if printf '%s\n' "$doctor_output" | grep -q "官方 auth.json 与当前账号一致"; then
        echo "隔离切换验证通过：账号库 current 与官方 auth.json 一致。"
    else
        echo "隔离切换验证失败：账号库与官方 auth.json 未确认一致。" >&2
        printf '%s\n' "$doctor_output" >&2
        exit 1
    fi
else
    echo "本次未执行账号切换，跳过 auth.json 一致性检查。"
fi
