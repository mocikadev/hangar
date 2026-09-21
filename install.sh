#!/usr/bin/env bash
# install.sh — hangar 安装脚本
# 用法：curl -fsSL https://raw.githubusercontent.com/mocikadev/hangar/main/install.sh | bash
# 或：   HANGAR_VERSION=v0.3.0 bash install.sh

set -euo pipefail

REPO="mocikadev/hangar"
BINARY="hangar"
INSTALL_DIR="${HANGAR_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${HANGAR_VERSION:-latest}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
RESET='\033[0m'

info()  { printf "${BOLD}info${RESET}  %s\n" "$*"; }
ok()    { printf "${GREEN}ok${RESET}    %s\n" "$*"; }
warn()  { printf "${YELLOW}warn${RESET}  %s\n" "$*" >&2; }
die()   { printf "${RED}error${RESET} %s\n" "$*" >&2; exit 1; }

case "${LANG:-}" in
  zh_*)
    MSG_TITLE="安装 hangar — Codex 多账号管理工具"
    MSG_PLATFORM="平台    ："
    MSG_VERSION="版本    ："
    MSG_INSTALL_DIR="安装目录："
    MSG_DOWNLOADING="下载中..."
    MSG_VERIFYING="校验 SHA256..."
    MSG_CHECKSUM_OK="SHA256 校验通过"
    MSG_CHECKSUM_SKIP_DL="无法下载 SHA256SUMS.txt，跳过校验。"
    MSG_CHECKSUM_SKIP_MISSING="SHA256SUMS.txt 中未找到对应条目，跳过校验。"
    MSG_CHECKSUM_SKIP_CMD="未找到 sha256sum / shasum，跳过校验。"
    MSG_CHECKSUM_FAIL="SHA256 校验失败！\n  预期: %s\n  实际: %s"
    MSG_NO_DOWNLOADER="需要 curl 或 wget 之一，但均未找到。"
    MSG_DOWNLOAD_FAIL="下载失败，请检查网络或版本号是否正确。"
    MSG_INSTALLED="已安装："
    MSG_PATH_WARN="%s 不在 \$PATH 中，请将以下内容加入 ~/.bashrc 或 ~/.zshrc："
    MSG_DONE="完成！"
    MSG_HINT="运行 %shangar%s 开始使用。"
    MSG_UNSUPPORTED_ARCH="不支持的架构：%s（%s）"
    MSG_UNSUPPORTED_OS="不支持的操作系统：%s（仅支持 Linux / macOS）"
    ;;
  *)
    MSG_TITLE="Installing hangar — Codex multi-account manager"
    MSG_PLATFORM="Platform   :"
    MSG_VERSION="Version    :"
    MSG_INSTALL_DIR="Install dir:"
    MSG_DOWNLOADING="Downloading..."
    MSG_VERIFYING="Verifying SHA256..."
    MSG_CHECKSUM_OK="SHA256 checksum verified"
    MSG_CHECKSUM_SKIP_DL="Could not download SHA256SUMS.txt, skipping verification."
    MSG_CHECKSUM_SKIP_MISSING="No matching entry in SHA256SUMS.txt, skipping verification."
    MSG_CHECKSUM_SKIP_CMD="sha256sum / shasum not found, skipping verification."
    MSG_CHECKSUM_FAIL="SHA256 mismatch!\n  expected: %s\n  actual:   %s"
    MSG_NO_DOWNLOADER="curl or wget is required but neither was found."
    MSG_DOWNLOAD_FAIL="Download failed. Check your network or the version string."
    MSG_INSTALLED="Installed:"
    MSG_PATH_WARN="%s is not in \$PATH. Add the following to ~/.bashrc or ~/.zshrc:"
    MSG_DONE="Done!"
    MSG_HINT="Run %shangar%s to get started."
    MSG_UNSUPPORTED_ARCH="Unsupported architecture: %s (%s)"
    MSG_UNSUPPORTED_OS="Unsupported OS: %s (only Linux / macOS are supported)"
    ;;
esac

detect_target() {
  local os arch
  os=$(uname -s)
  arch=$(uname -m)

  case "$os" in
    Linux)
      case "$arch" in
        x86_64)          echo "linux-amd64" ;;
        aarch64 | arm64) echo "linux-arm64" ;;
        *)               die "$(printf "$MSG_UNSUPPORTED_ARCH" "$arch" "Linux")" ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64)          echo "macos-amd64" ;;
        arm64)           echo "macos-arm64" ;;
        *)               die "$(printf "$MSG_UNSUPPORTED_ARCH" "$arch" "macOS")" ;;
      esac
      ;;
    *)
      die "$(printf "$MSG_UNSUPPORTED_OS" "$os")"
      ;;
  esac
}

download() {
  local url="$1" dest="$2"
  if command -v curl > /dev/null 2>&1; then
    curl -fsSL --retry 3 --retry-delay 2 "$url" -o "$dest"
  elif command -v wget > /dev/null 2>&1; then
    wget -q --tries=3 "$url" -O "$dest"
  else
    die "$MSG_NO_DOWNLOADER"
  fi
}

resolve_url() {
  local name="$1"
  if [ "$VERSION" = "latest" ]; then
    echo "https://github.com/$REPO/releases/latest/download/$name"
  else
    echo "https://github.com/$REPO/releases/download/$VERSION/$name"
  fi
}

verify_checksum() {
  local binary_path="$1" target="$2"
  local checksum_url checksum_tmp expected actual

  checksum_url=$(resolve_url "SHA256SUMS.txt")
  checksum_tmp=$(mktemp)

  info "$MSG_VERIFYING"
  if ! download "$checksum_url" "$checksum_tmp" 2>/dev/null; then
    rm -f "$checksum_tmp"
    warn "$MSG_CHECKSUM_SKIP_DL"
    return 0
  fi

  expected=$(grep "  $BINARY-$target$" "$checksum_tmp" | awk '{print $1}')
  rm -f "$checksum_tmp"

  if [ -z "$expected" ]; then
    warn "$MSG_CHECKSUM_SKIP_MISSING"
    return 0
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$binary_path" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$binary_path" | awk '{print $1}')
  else
    warn "$MSG_CHECKSUM_SKIP_CMD"
    return 0
  fi

  if [ "$expected" != "$actual" ]; then
    die "$(printf "$MSG_CHECKSUM_FAIL" "$expected" "$actual")"
  fi

  ok "$MSG_CHECKSUM_OK"
}

main() {
  printf "\n${BOLD}%s${RESET}\n\n" "$MSG_TITLE"

  local target url tmp
  target=$(detect_target)
  url=$(resolve_url "$BINARY-$target")

  info "$MSG_PLATFORM $target"
  info "$MSG_VERSION $VERSION"
  info "$MSG_INSTALL_DIR $INSTALL_DIR"
  printf "\n"

  mkdir -p "$INSTALL_DIR"

  info "$MSG_DOWNLOADING"
  tmp=$(mktemp)
  # shellcheck disable=SC2064
  trap "rm -f '$tmp'" EXIT

  download "$url" "$tmp" || die "$MSG_DOWNLOAD_FAIL"
  verify_checksum "$tmp" "$target"
  chmod +x "$tmp"
  mv "$tmp" "$INSTALL_DIR/$BINARY"

  ok "$MSG_INSTALLED $INSTALL_DIR/$BINARY"

  if "$INSTALL_DIR/$BINARY" --version >/dev/null 2>&1; then
    ok "$MSG_VERSION $("$INSTALL_DIR/$BINARY" --version 2>&1)"
  fi

  if ! echo ":${PATH}:" | grep -qF ":${INSTALL_DIR}:"; then
    printf "\n"
    warn "$(printf "$MSG_PATH_WARN" "$INSTALL_DIR")"
    printf "\n  ${BOLD}export PATH=\"\$HOME/.local/bin:\$PATH\"${RESET}\n"
  fi

  printf "\n${GREEN}${BOLD}%s${RESET} $(printf "$MSG_HINT" "${BOLD}" "${RESET}")\n\n" "$MSG_DONE"
}

main "$@"
