# install.ps1 — hangar Windows 安装脚本
# 用法：irm https://raw.githubusercontent.com/mocikadev/hangar/main/install.ps1 | iex
# 或：   $env:HANGAR_VERSION="v0.3.0"; irm .../install.ps1 | iex

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$Repo       = "mocikadev/hangar"
$Binary     = "hangar"
$Artifact   = "hangar-windows-x86_64.exe"
$InstallDir = if ($env:HANGAR_INSTALL_DIR) { $env:HANGAR_INSTALL_DIR } else { "$HOME\.local\bin" }
$Version    = if ($env:HANGAR_VERSION)     { $env:HANGAR_VERSION }     else { "latest" }

$IsZh = (Get-UICulture).Name -like "zh-*"

function msg_info  { param($s) Write-Host "info  $s" -ForegroundColor Cyan }
function msg_ok    { param($s) Write-Host "ok    $s" -ForegroundColor Green }
function msg_warn  { param($s) Write-Host "warn  $s" -ForegroundColor Yellow }
function msg_die   { param($s) Write-Host "error $s" -ForegroundColor Red; exit 1 }

function Resolve-AssetUrl {
    param($Name)
    if ($Version -eq "latest") {
        "https://github.com/$Repo/releases/latest/download/$Name"
    } else {
        "https://github.com/$Repo/releases/download/$Version/$Name"
    }
}

function Get-Checksum {
    param($FilePath, $ArtifactName)

    $ChecksumUrl = Resolve-AssetUrl "SHA256SUMS.txt"
    $ChecksumTmp = [System.IO.Path]::GetTempFileName()

    try {
        Invoke-WebRequest -Uri $ChecksumUrl -OutFile $ChecksumTmp -UseBasicParsing -ErrorAction SilentlyContinue
    } catch {
        if ($IsZh) { msg_warn "无法下载 SHA256SUMS.txt，跳过校验。" }
        else        { msg_warn "Could not download SHA256SUMS.txt, skipping verification." }
        return
    }

    $Lines    = Get-Content $ChecksumTmp
    $Expected = ($Lines | Where-Object { $_ -match "  $ArtifactName$" }) -replace "  $ArtifactName$", "" | Select-Object -First 1

    Remove-Item $ChecksumTmp -Force -ErrorAction SilentlyContinue

    if (-not $Expected) {
        if ($IsZh) { msg_warn "SHA256SUMS.txt 中未找到对应条目，跳过校验。" }
        else        { msg_warn "No matching entry in SHA256SUMS.txt, skipping verification." }
        return
    }

    $Actual = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLower()
    $Expected = $Expected.Trim().ToLower()

    if ($Actual -ne $Expected) {
        if ($IsZh) { msg_die "SHA256 校验失败！`n  预期: $Expected`n  实际: $Actual" }
        else        { msg_die "SHA256 mismatch!`n  expected: $Expected`n  actual:   $Actual" }
    }

    if ($IsZh) { msg_ok "SHA256 校验通过" }
    else        { msg_ok "SHA256 checksum verified" }
}

function Add-ToUserPath {
    param($Dir)
    $CurrentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($CurrentPath -split ";" -notcontains $Dir) {
        [Environment]::SetEnvironmentVariable("PATH", "$Dir;$CurrentPath", "User")
        if ($IsZh) { msg_warn "$Dir 已加入用户 PATH，重新打开终端后生效。" }
        else        { msg_warn "$Dir added to user PATH. Restart your terminal for it to take effect." }
    }
}

function Main {
    Write-Host ""
    if ($IsZh) { Write-Host "安装 hangar — Codex 多账号管理工具" -ForegroundColor White }
    else        { Write-Host "Installing hangar — Codex multi-account manager" -ForegroundColor White }
    Write-Host ""

    $Url = Resolve-AssetUrl $Artifact

    if ($IsZh) {
        msg_info "平台    : windows-x86_64"
        msg_info "版本    : $Version"
        msg_info "安装目录: $InstallDir"
    } else {
        msg_info "Platform   : windows-x86_64"
        msg_info "Version    : $Version"
        msg_info "Install dir: $InstallDir"
    }
    Write-Host ""

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $Tmp = [System.IO.Path]::GetTempFileName() + ".exe"

    if ($IsZh) { msg_info "下载中..." } else { msg_info "Downloading..." }
    try {
        Invoke-WebRequest -Uri $Url -OutFile $Tmp -UseBasicParsing
    } catch {
        if ($IsZh) { msg_die "下载失败，请检查网络或版本号是否正确。" }
        else        { msg_die "Download failed. Check your network or the version string." }
    }

    if ($IsZh) { msg_info "校验 SHA256..." } else { msg_info "Verifying SHA256..." }
    Get-Checksum $Tmp $Artifact

    $Dest = Join-Path $InstallDir "$Binary.exe"
    Move-Item -Path $Tmp -Destination $Dest -Force

    if ($IsZh) { msg_ok "已安装: $Dest" } else { msg_ok "Installed: $Dest" }

    $Ver = & $Dest --version 2>&1
    if ($Ver) { msg_ok "$Ver" }

    Add-ToUserPath $InstallDir

    Write-Host ""
    if ($IsZh) { Write-Host "完成！运行 hangar 开始使用。" -ForegroundColor Green }
    else        { Write-Host "Done! Run 'hangar' to get started." -ForegroundColor Green }
    Write-Host ""
}

Main
