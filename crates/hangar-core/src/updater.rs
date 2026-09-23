//! 自升级：GitHub Release 检查与应用（对标 skm updater，经 ureq+sha2 零新依赖）。
//!
//! - 版本比较、SHA 解析、缓存 TTL 为纯函数（单测覆盖）
//! - 网络失败一律 `Ok(None)` 静默放行，不阻塞启动
//! - 调用方必须传入二进制自身的 `env!("CARGO_PKG_VERSION")`（本模块在 core 求值会是 core 版本）

use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPO: &str = "mocikadev/hangar";
const GITHUB_API: &str = "https://api.github.com";
const CHECK_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const CACHE_TTL_SECS: u64 = 86400;

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub binary_url: String,
    pub checksum_url: String,
    /// 本平台 CLI 产物文件名（如 hangar-linux-amd64）
    pub asset: String,
}

/// 当前平台对应的 Release 产物后缀，无支持平台返回 None（调用方直接跳过升级）
pub fn current_target() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-amd64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("linux-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("macos-amd64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows-x86_64")
    } else {
        None
    }
}

fn asset_name_for_target(target: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("hangar-{}.exe", target)
    } else {
        format!("hangar-{}", target)
    }
}

fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.trim().trim_start_matches('v');
    let mut it = v.split('.');
    Some((
        it.next()?.trim().parse().ok()?,
        it.next()?.trim().parse().ok()?,
        it.next()?.trim().parse().ok()?,
    ))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// SHA256SUMS 行解析：`<hex><空白><文件名>`，按空白切分并 trim，天然兼容 CRLF
fn find_checksum(body: &str, asset: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        if name.trim() == asset {
            Some(hash.trim().to_string())
        } else {
            None
        }
    })
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 缓存是否过期：`now - last >= 86400` 即过期；`last` 为 0（损坏/缺失）视为过期走联网
fn cache_expired(now: u64, last: u64) -> bool {
    now.saturating_sub(last) >= CACHE_TTL_SECS
}

fn cache_path() -> Result<std::path::PathBuf, String> {
    let acc = crate::account::accounts_file_path()?;
    acc.parent()
        .map(|p| p.join("update-check.json"))
        .ok_or_else(|| "账号库路径无父目录".to_string())
}

/// 上次检查时间戳（秒），损坏/缺失返回 None（调用方视为过期）
pub fn last_check_secs() -> Option<u64> {
    let path = cache_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("last_check")?.as_u64()
}

fn record_check(now: u64) {
    let Ok(path) = cache_path() else { return };
    let content = format!("{{\"last_check\":{}}}", now);
    // 最佳努力：失败忽略（不影响升级主流程）
    let _ = crate::account::write_file_private(&path, &content);
}

/// 检查更新：`force` 跳过 24h 缓存。网络/解析/限流等任何失败都返回 `Ok(None)`。
/// CLI 版本由调用方传入；`env!` 在 core 求值会得到 core 版本，不能在此求值。
pub fn check_update(force: bool, current_version: &str) -> Result<Option<ReleaseInfo>, String> {
    let target = match current_target() {
        Some(t) => t,
        None => return Ok(None),
    };
    let now = now_secs();
    if !force {
        if let Some(last) = last_check_secs() {
            if !cache_expired(now, last) {
                return Ok(None);
            }
        }
    }
    let info = fetch_latest(target, current_version.trim())?;
    record_check(now);
    Ok(info)
}

fn agent(connect: Duration, total: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(connect)
        .timeout(total)
        .build()
}

/// 脱敏的 ureq 错误：只记 status + body_len，不回显 body
fn redacted(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let len = r.into_string().unwrap_or_default().len();
            format!("HTTP {} body_len={}", code, len)
        }
        other => format!("{}", other),
    }
}

fn fetch_release_doc(current_version: &str) -> Option<serde_json::Value> {
    let url = format!("{GITHUB_API}/repos/{REPO}/releases/latest");
    let mut req = agent(CHECK_CONNECT_TIMEOUT, CHECK_TIMEOUT)
        .get(&url)
        .set("User-Agent", &format!("hangar/{}", current_version))
        .set("Accept", "application/vnd.github.v3+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.trim().is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", token.trim()));
        }
    }
    let body = req.call().ok()?.into_string().ok()?;
    serde_json::from_str(&body).ok()
}

fn fetch_tag(v: &serde_json::Value) -> Option<String> {
    let tag = v
        .get("tag_name")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

fn fetch_latest(target: &str, current_version: &str) -> Result<Option<ReleaseInfo>, String> {
    let current_version = current_version.trim();
    let Some(v) = fetch_release_doc(current_version) else {
        return Ok(None);
    };
    let Some(tag) = fetch_tag(&v) else {
        return Ok(None);
    };
    let version = tag.trim_start_matches('v').to_string();
    if !is_newer(&version, current_version) {
        return Ok(None);
    }
    let find = |name: &str| {
        v.get("assets")?.as_array()?.iter().find_map(|a| {
            let n = a.get("name")?.as_str()?;
            if n == name {
                a.get("browser_download_url")?
                    .as_str()
                    .map(|s| s.to_string())
            } else {
                None
            }
        })
    };
    let asset = asset_name_for_target(target);
    let (Some(binary_url), Some(checksum_url)) = (find(&asset), find("SHA256SUMS.txt")) else {
        return Ok(None);
    };
    Ok(Some(ReleaseInfo {
        tag,
        version,
        binary_url,
        checksum_url,
        asset,
    }))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn tmp_path_for(exe: &std::path::Path) -> std::path::PathBuf {
    let name = exe.file_name().and_then(|s| s.to_str()).unwrap_or("hangar");
    exe.with_file_name(format!("{}.update-tmp", name))
}

fn old_path_for(exe: &std::path::Path) -> std::path::PathBuf {
    let name = exe.file_name().and_then(|s| s.to_str()).unwrap_or("hangar");
    exe.with_file_name(format!("{}.old", name))
}

/// 下载、验 SHA、全自动替换当前二进制。成功返回新版本号；
/// 任何失败返回 Err 且旧二进制不受影响（调用方继续进 TUI）。
pub fn apply_update(info: &ReleaseInfo) -> Result<String, String> {
    let _ = current_target().ok_or_else(|| "当前平台不支持自升级".to_string())?;
    let asset = info.asset.clone();
    let client = agent(CHECK_CONNECT_TIMEOUT, DOWNLOAD_TIMEOUT);

    let bin_bytes = client
        .get(&info.binary_url)
        .call()
        .map_err(redacted)
        .and_then(|r| {
            let mut buf = Vec::new();
            r.into_reader()
                .read_to_end(&mut buf)
                .map_err(|e| format!("读取新版本失败: {}", e))?;
            Ok(buf)
        })
        .map_err(|e| format!("下载新版本失败: {}", e))?;
    let sums = client
        .get(&info.checksum_url)
        .call()
        .map_err(redacted)
        .and_then(|r| {
            r.into_string()
                .map_err(|e| format!("读取校验文件失败: {}", e))
        })
        .map_err(|e| format!("下载校验文件失败: {}", e))?;
    let expected = find_checksum(&sums, &asset)
        .ok_or_else(|| "校验文件缺失本平台条目，已中止升级".to_string())?;
    if !sha256_hex(&bin_bytes).eq_ignore_ascii_case(expected.trim()) {
        return Err("SHA256 校验失败，已中止升级（旧版不受影响）".to_string());
    }

    let exe = std::env::current_exe().map_err(|e| format!("定位当前程序失败: {}", e))?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let tmp = tmp_path_for(&exe);
    let _ = std::fs::remove_file(&tmp);
    crate::account::write_bytes_private(&tmp, &bin_bytes)
        .map_err(|e| format!("写入临时文件失败: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    // Windows：运行中的 exe 不可覆盖，但允许改名移开；先腾出文件名再换入
    #[cfg(windows)]
    {
        let old = old_path_for(&exe);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("移开旧版本失败: {}", e)
        })?;
    }
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        let _ = std::fs::remove_file(&tmp);
        // Windows 下 exe 可能已被移至 .old：尝试回摆；回摆失败则告知备份位置手动恢复
        #[cfg(windows)]
        {
            let old = old_path_for(&exe);
            if old.exists() && std::fs::rename(&old, &exe).is_err() {
                return Err(format!(
                    "替换新版本失败: {}；旧版备份保留在 {}，请手动恢复",
                    e,
                    old.display()
                ));
            }
        }
        return Err(format!("替换新版本失败: {}", e));
    }
    Ok(info.version.clone())
}

/// 启动时顺手清理 Windows 残留 `hangar.old`（最佳努力，永不报错）
pub fn cleanup_pending_old() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if exe.exists() {
        let _ = std::fs::remove_file(old_path_for(&exe));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_comparison() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.3.0"));
        assert!(!is_newer("bad", "0.2.0"));
        assert!(!is_newer("0.2.0", "bad"));
    }

    #[test]
    fn checksum_line_parses_crlf() {
        let body = "abc123  hangar-linux-amd64\r\ndef456  hangar-macos-arm64\r\n";
        assert_eq!(
            find_checksum(body, "hangar-linux-amd64").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            find_checksum(body, "hangar-windows-x86_64.exe").as_deref(),
            None
        );
    }

    #[test]
    fn cache_ttl_boundary() {
        assert!(!cache_expired(1_000_000, 1_000_000 - 86_399)); // 未过期
        assert!(cache_expired(1_000_000, 1_000_000 - 86_401)); // 已过期
    }

    #[test]
    fn corrupt_cache_means_expired() {
        assert!(cache_expired(1_000_000, 0)); // last=0 视为过期走联网
    }

    #[test]
    fn asset_name_uses_cli_release_contract() {
        let cli = asset_name_for_target("linux-amd64");
        assert!(cli.starts_with("hangar-") && !cli.contains("gui"));
        if cfg!(target_os = "windows") {
            assert!(cli.ends_with(".exe"));
        } else {
            assert!(!cli.ends_with(".exe"));
        }
    }

    #[test]
    fn sibling_names_keep_dir_and_stem() {
        // 含空格/中文目录：PathBuf 操作不做字符串拼接
        let dir = std::path::PathBuf::from("tmp").join("我的 目录");
        let exe = dir.join("hangar");
        assert_eq!(tmp_path_for(&exe), dir.join("hangar.update-tmp"));
        assert_eq!(old_path_for(&exe), dir.join("hangar.old"));
    }
}
