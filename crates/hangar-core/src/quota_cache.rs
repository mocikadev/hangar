//! 可重建的脱敏配额快照。CLI/TUI 与原生 GUI 共用文件和新鲜度规则。

use crate::quota::{reset_at_ts, Quota};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TTL_SECS: u64 = 30 * 60;
const SCHEMA_VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub fetched_at: u64,
    pub quota: Quota,
}

impl Entry {
    pub fn is_fresh(&self, now: u64) -> bool {
        self.fetched_at > 0 && self.fetched_at <= now && now - self.fetched_at < TTL_SECS
    }
}

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
    schema_version: u32,
    accounts: HashMap<String, Entry>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn path() -> Result<PathBuf, String> {
    let accounts = crate::account::accounts_file_path()?;
    Ok(accounts.with_file_name("quota-cache.json"))
}

fn load_at(path: &Path) -> HashMap<String, Entry> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
        return HashMap::new();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return HashMap::new();
    };
    let Ok(cache) = serde_json::from_slice::<CacheFile>(&bytes) else {
        return HashMap::new();
    };
    if cache.schema_version != SCHEMA_VERSION {
        return HashMap::new();
    }
    cache.accounts
}

pub fn load() -> HashMap<String, Entry> {
    path().map_or_else(|_| HashMap::new(), |path| load_at(&path))
}

pub fn due(entry: Option<&Entry>, now: u64, force: bool) -> bool {
    force || entry.is_none_or(|entry| !entry.is_fresh(now))
}

/// 请求完成时固定相对重置时间，避免重启后把 `reset_after_seconds` 加到新的当前时间。
fn normalize_reset_times(quota: &mut Quota, now: i64) {
    for named in &mut quota.windows {
        named.window.reset_at = reset_at_ts(&named.window, now);
        named.window.reset_in_secs = None;
    }
}

fn write_at(
    path: &Path,
    account_id: &str,
    mut quota: Quota,
    fetched_at: u64,
) -> Result<(), String> {
    let mut accounts = load_at(path);
    if accounts
        .get(account_id)
        .is_some_and(|entry| entry.fetched_at > fetched_at)
    {
        return Ok(());
    }
    normalize_reset_times(&mut quota, fetched_at as i64);
    accounts.insert(account_id.to_string(), Entry { fetched_at, quota });
    let content = serde_json::to_string(&CacheFile {
        schema_version: SCHEMA_VERSION,
        accounts,
    })
    .map_err(|error| format!("序列化配额缓存失败: {error}"))?;
    let temp = path.with_extension(format!(
        "tmp.{}.{}.{}",
        std::process::id(),
        now_secs(),
        NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    crate::account::write_file_private(&temp, &content)
        .map_err(|error| format!("写入配额缓存失败: {error}"))?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("替换配额缓存失败: {error}"));
    }
    Ok(())
}

/// 每次成功只合并该账号；账号锁覆盖读-改-写，防止 CLI 与 GUI 互相覆盖。
pub fn store_success(account_id: &str, quota: &Quota, fetched_at: u64) -> Result<(), String> {
    crate::account::with_accounts_lock(|| {
        let path = path()?;
        write_at(&path, account_id, quota.clone(), fetched_at)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::{NamedWindow, QuotaWindow};

    fn quota(remaining: i32) -> Quota {
        Quota {
            windows: vec![NamedWindow {
                label: "周".into(),
                window: QuotaWindow {
                    remaining: Some(remaining),
                    reset_in_secs: Some(60),
                    ..QuotaWindow::default()
                },
            }],
            ..Quota::default()
        }
    }

    #[test]
    fn ttl_boundary_and_future_timestamp() {
        let entry = Entry {
            fetched_at: 100,
            quota: quota(50),
        };
        assert!(!due(Some(&entry), 100 + TTL_SECS - 1, false));
        assert!(due(Some(&entry), 100 + TTL_SECS, false));
        assert!(due(Some(&entry), 99, false));
        assert!(due(Some(&entry), 101, true));
    }

    #[test]
    fn merging_keeps_newer_account_and_absolute_reset_time() {
        let dir = std::env::temp_dir().join(format!(
            "hangar-quota-cache-test-{}-{}",
            std::process::id(),
            now_secs()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("quota-cache.json");
        write_at(&path, "a", quota(70), 1000).unwrap();
        write_at(&path, "b", quota(40), 1100).unwrap();
        write_at(&path, "a", quota(10), 900).unwrap();
        let entries = load_at(&path);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries["a"].quota.windows[0].window.remaining, Some(70));
        assert_eq!(entries["a"].quota.windows[0].window.reset_at, Some(1060));
        assert_eq!(entries["a"].quota.windows[0].window.reset_in_secs, None);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("token"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_schema_and_corrupt_cache_are_ignored() {
        let dir = std::env::temp_dir().join(format!(
            "hangar-quota-cache-invalid-{}-{}",
            std::process::id(),
            now_secs()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("quota-cache.json");
        std::fs::write(&path, "broken").unwrap();
        assert!(load_at(&path).is_empty());
        std::fs::write(&path, r#"{"schema_version":99,"accounts":{}}"#).unwrap();
        assert!(load_at(&path).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
