use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub id_token: String,
    pub expires_at: u64,
    /// refresh_token 已失效，需要重新登录（刷新 401 时标记）
    #[serde(default)]
    pub stale: bool,
    /// 同邮箱多 org/账号区分（对齐 cockpit-tools 三元组）；老库缺字段时默认 None
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub organization_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsFile {
    pub accounts: Vec<Account>,
    pub current_account_id: Option<String>,
}

/// email 归一化：email 是账号唯一身份键（收编/去重/stale 复活都靠它匹配），
/// 必须消除大小写/空白差异，否则同一账号会裂成两条
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn switcher_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取 home 目录".to_string())?;
    let dir = home.join(".hangar");
    // 明文 token 目录：创建即 700，避免先 755 再 chmod 的窗口期可被同机他用户列目录
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|e| format!("创建目录失败: {}", e))?;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    Ok(dir)
}

fn accounts_path() -> Result<PathBuf, String> {
    Ok(switcher_dir()?.join("accounts.json"))
}

/// doctor 自检用的路径暴露（只读，不绕开锁做写）
pub fn accounts_file_path() -> Result<PathBuf, String> {
    accounts_path()
}

pub fn codex_home_path() -> Result<PathBuf, String> {
    codex_home()
}

pub fn accounts_lock_path() -> Result<PathBuf, String> {
    lock_path()
}

/// 与 cockpit-tools / 官方 codex 一致：CODEX_HOME 环境变量优先，否则 ~/.codex
fn codex_home() -> Result<PathBuf, String> {
    if let Ok(raw) = std::env::var("CODEX_HOME") {
        let trimmed = raw.trim().trim_matches('"').trim_matches('\'').trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            // 环境变量可被污染指向任意路径：已存在则必须是目录，否则后续
            // create_dir_all/写 auth.json 会误写敏感位置；提前拦截并给出明确报错
            if p.exists() && !p.is_dir() {
                return Err(format!(
                    "CODEX_HOME 指向非目录（{}），已拒绝写入以保护该路径",
                    p.display()
                ));
            }
            return Ok(p);
        }
    }
    let home = dirs::home_dir().ok_or_else(|| "无法获取 home 目录".to_string())?;
    Ok(home.join(".codex"))
}

/// 原子写入：唯一临时文件 + rename，避免崩溃留下半截文件。
/// 固定 tmp 名在双实例并发下互踩，改用 pid+nanos 唯一名（对齐 cockpit-tools）。
/// 附带 .bak 备份：目标已存在时先 copy 备份，rename 失败清理 tmp；磁盘满时显式提示。
fn backup_path_for(path: &std::path::Path) -> std::path::PathBuf {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    path.with_file_name(format!("{}.bak", name))
}

fn temp_path_for(path: &std::path::Path) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    path.with_file_name(format!(
        ".{}.tmp.{}.{}.atomic",
        name,
        std::process::id(),
        nanos
    ))
}

fn is_disk_full_err(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(28) | Some(112))
        || e.to_string().contains("No space left on device")
}

/// 明文凭据文件直写：创建即 600，关闭“先 644 再 chmod”的可读窗口。
/// tmp 名唯一（pid+nanos），用 create_new 防复用；非 unix 回退普通写。
pub(crate) fn write_file_private(
    path: &std::path::Path,
    content: &str,
) -> Result<(), std::io::Error> {
    write_bytes_private(path, content.as_bytes())
}

/// 二进制版直写（自升级下载产物用，str 版会破坏非 UTF-8 字节）
pub(crate) fn write_bytes_private(
    path: &std::path::Path,
    bytes: &[u8],
) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(bytes)?;
        // umask 可能进一步收紧但不会放宽；显式再收紧一次兜底已存在文件的旧权限
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
    }
}

fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            if is_disk_full_err(&e) {
                format!("创建目录失败（磁盘空间不足）: {}", e)
            } else {
                format!("创建目录失败: {}", e)
            }
        })?;
    }
    // 先备份旧文件（最佳努力，失败只告警不阻断）。
    // 备份含明文 token：经 write_file_private 直写 600，避免 copy 经 umask 的 644 窗口
    if path.exists() {
        let bak = backup_path_for(path);
        match std::fs::read(path) {
            Ok(bytes) => {
                if let Err(e) = write_file_private(&bak, &String::from_utf8_lossy(&bytes)) {
                    crate::emit::emit_err(format!("备份旧文件失败，继续写入: {}", e));
                }
            }
            Err(e) => crate::emit::emit_err(format!("备份旧文件失败，继续写入: {}", e)),
        }
    }
    let tmp = temp_path_for(path);
    // 唯一 tmp 名理论不重；若因 nanos 碰撞已存在则先清掉再以 600 建新文件
    let _ = std::fs::remove_file(&tmp);
    write_file_private(&tmp, content).map_err(|e| {
        if is_disk_full_err(&e) {
            format!("写入临时文件失败（磁盘空间不足）: {}", e)
        } else {
            format!("写入临时文件失败: {}", e)
        }
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        if is_disk_full_err(&e) {
            format!("原子替换失败（磁盘空间不足）: {}", e)
        } else {
            format!("原子替换失败: {}", e)
        }
    })?;
    // rename 保留 tmp 的 600 权限；若目标是旧文件替换，部分平台沿用旧 inode 权限，显式再收紧
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// 从 .bak 恢复：主文件解析失败时尝试回滚，避免单文件损坏即全丢
fn restore_from_backup(path: &std::path::Path) -> Result<bool, String> {
    let bak = backup_path_for(path);
    if !bak.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&bak).map_err(|e| format!("读取备份文件失败: {}", e))?;
    // 备份本身也要能解析才回写，防止用坏备份覆盖
    let parsed: AccountsFile =
        serde_json::from_str(&content).map_err(|e| format!("备份文件同样损坏: {}", e))?;
    let pretty =
        serde_json::to_string_pretty(&parsed).map_err(|e| format!("序列化备份失败: {}", e))?;
    let tmp = temp_path_for(path);
    let _ = std::fs::remove_file(&tmp);
    write_file_private(&tmp, &pretty).map_err(|e| format!("回滚写入失败: {}", e))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("回滚替换失败: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(true)
}

pub fn load_accounts() -> Result<AccountsFile, String> {
    let path = accounts_path()?;
    if !path.exists() {
        return Ok(AccountsFile {
            accounts: vec![],
            current_account_id: None,
        });
    }
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取账号文件失败: {}", e))?;
    match serde_json::from_str::<AccountsFile>(&content) {
        Ok(v) => Ok(v),
        Err(first) => {
            // 主文件损坏时尝试从 .bak 自动恢复（对齐 cockpit-tools）
            match restore_from_backup(&path) {
                Ok(true) => {
                    let fixed = std::fs::read_to_string(&path)
                        .map_err(|e| format!("回滚后读取失败: {}", e))?;
                    serde_json::from_str(&fixed)
                        .map_err(|e| format!("主文件损坏（{}），已回滚但仍解析失败: {}", first, e))
                }
                Ok(false) => Err(format!("解析账号文件失败: {}", first)),
                Err(e) => Err(format!("解析账号文件失败: {}；回滚失败: {}", first, e)),
            }
        }
    }
}

pub fn save_accounts(accounts: &AccountsFile) -> Result<(), String> {
    // 短持锁防双实例同时 rename 交错；需要横跨 load→save 持锁时用 with_accounts_lock
    let _lock = acquire_accounts_lock()?;
    save_accounts_unlocked(accounts)
}

pub fn save_accounts_unlocked(accounts: &AccountsFile) -> Result<(), String> {
    let path = accounts_path()?;
    let content =
        serde_json::to_string_pretty(accounts).map_err(|e| format!("序列化账号文件失败: {}", e))?;
    atomic_write(&path, &content)
}

/// 横跨 load→改→save 的事务：持锁执行闭包，闭包内用 *_unlocked / 直接改内存后统一落盘
/// 同线程可重入（save_accounts 内再次加锁不会自死锁）
pub fn with_accounts_lock<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let _lock = acquire_accounts_lock()?;
    f()
}

/// 定向复活（reauth）：用新登录凭据原位覆盖目标 id 的账号（stale 复活专用）
/// 邮箱不一致时拒绝，避免把 A 的登录态写到 B 身上（对齐 cockpit resolve_reauth_target）
pub fn reauth_account(target_id: &str, fresh: &Account) -> Result<(), String> {
    with_accounts_lock(|| {
        let mut file = load_accounts()?;
        let pos = file
            .accounts
            .iter()
            .position(|a| a.id == target_id)
            .ok_or_else(|| format!("账号未找到: {}", target_id))?;
        let old_email = normalize_email(&file.accounts[pos].email);
        if !old_email.is_empty() && old_email != fresh.email {
            return Err(format!(
                "重新登录邮箱不匹配：目标为 {}，本次为 {}，已拒绝覆盖",
                file.accounts[pos].email, fresh.email
            ));
        }
        let t = &mut file.accounts[pos];
        t.email = fresh.email.clone();
        t.access_token = fresh.access_token.clone();
        t.refresh_token = fresh.refresh_token.clone();
        t.id_token = fresh.id_token.clone();
        t.expires_at = fresh.expires_at;
        t.stale = false;
        if fresh.account_id.is_some() {
            t.account_id = fresh.account_id.clone();
        }
        if fresh.organization_id.is_some() {
            t.organization_id = fresh.organization_id.clone();
        }
        file.current_account_id = Some(t.id.clone());
        save_accounts_unlocked(&file)
    })?;
    switch_account(target_id)
}

/// 删除账号（经典循环与 TUI 共用）。
/// 禁止删除"使用中"账号：官方 auth.json 仍持有其明文凭据，删了会留下
/// 无主登录态且无任何入口清理，属不安全残留（S7 保守策略升级为硬约束）。
/// 返回恒为 Ok(false)（签名保持 Result 以便与调用方错误处理统一）
pub fn delete_account(account_id: &str) -> Result<bool, String> {
    with_accounts_lock(|| {
        let mut file = load_accounts()?;
        let idx = file
            .accounts
            .iter()
            .position(|a| a.id == account_id)
            .ok_or_else(|| format!("账号未找到: {}", account_id))?;
        if Some(account_id) == file.current_account_id.as_deref() {
            return Err(format!(
                "账号 {} 正在使用中，无法删除；请先切换到其他账号（登录新号或切到已有账号）",
                file.accounts[idx].email
            ));
        }
        file.accounts.remove(idx);
        save_accounts_unlocked(&file)?;
        Ok(false)
    })
}

/// 账号库文件锁：~/.hangar/.accounts.lock（create_new 独占 + pid/time + 过期自愈）
/// 无需第三方依赖；持锁期间 crash 留下的陈旧锁按 mtime 过期拆除；同线程可重入。
/// 过期阈值 120s：switch 持锁横跨 refresh 网络请求（默认超时可达几十秒），
/// 15s 会导致第二实例中途夺锁、双刷同一 RT 造成 401；等待超时仍为 10s（快速失败重试）
pub struct AccountsLock {
    path: std::path::PathBuf,
    owned: bool,
}

impl Drop for AccountsLock {
    fn drop(&mut self) {
        LOCK_DEPTH.with(|c| {
            let d = c.get().saturating_sub(1);
            c.set(d);
            if d == 0 && self.owned {
                let _ = std::fs::remove_file(&self.path);
            }
        });
    }
}

use std::cell::Cell;
thread_local! {
    static LOCK_DEPTH: Cell<u32> = const { Cell::new(0) };
}

fn lock_path() -> Result<std::path::PathBuf, String> {
    Ok(switcher_dir()?.join(".accounts.lock"))
}

pub fn acquire_accounts_lock() -> Result<AccountsLock, String> {
    // 同线程重入：直接增加深度，不重复建锁文件
    if LOCK_DEPTH.with(|c| c.get()) > 0 {
        LOCK_DEPTH.with(|c| c.set(c.get() + 1));
        return Ok(AccountsLock {
            path: lock_path()?,
            owned: false,
        });
    }
    use std::io::Write;
    let path = lock_path()?;
    let start = SystemTime::now();
    loop {
        // 锁文件经 600 建：虽只含 pid/time 无凭据，仍避免同机他用户探测实例活动
        #[cfg(unix)]
        let open_res = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
        };
        #[cfg(not(unix))]
        let open_res = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path);
        match open_res {
            Ok(mut f) => {
                let _ = writeln!(f, "{} {}", std::process::id(), now_secs());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                }
                LOCK_DEPTH.with(|c| c.set(1));
                return Ok(AccountsLock { path, owned: true });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // 陈旧锁（>120s，见上）拆除自愈，防止 crash 后永久阻塞
                let stale = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| {
                        SystemTime::now()
                            .duration_since(t)
                            .unwrap_or_default()
                            .as_secs()
                            > 120
                    })
                    .unwrap_or(true);
                if stale {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                if start.elapsed().unwrap_or_default().as_secs() > 10 {
                    return Err("账号库被另一实例锁定（10s），请稍后重试".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(format!("获取文件锁失败: {}", e)),
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 提前刷新阈值（秒），与 cockpit-tools 的 TOKEN_REFRESH_SKEW_SECONDS 一致
const REFRESH_SKEW_SECS: u64 = 300;

/// harvest 收编/更新凭据后的视为有效时长（秒）。
/// 官方 auth.json 无过期时间；凭据既来自官方刚刷新的文件，视为 1 小时有效，
/// 避免每次启动都无谓刷新消耗 RT 轮换寿命
const HARVEST_VALIDITY_SECS: u64 = 3600;

/// 收敛入口：每次命令启动时统一执行（官方 → 库 反向同步 + 新账号收编）
pub fn harvest() {
    harvest_from_official_auth();
}

/// 从官方 auth.json 回收最新凭据（harvest）。
///
/// 官方 Codex 客户端运行时会自行刷新并轮换 refresh_token 写回 auth.json。
/// 若不回收，账号库中的旧 RT 会失效。识别归属：解码 id_token（JWT）中的
/// email 声明匹配账号（官方轮换后 token 全变，唯 email 不变）。
/// 发现任何 token 字段与库中不同即采纳官方值并落盘。
fn harvest_from_official_auth() {
    // 全程持锁：load→改→save 原子化，防双实例交错覆盖
    let _ = with_accounts_lock(|| {
        harvest_locked()?;
        Ok(())
    });
}

fn harvest_locked() -> Result<(), String> {
    let file = load_accounts()?;
    // 注意：空账号库不做早退——空库正是收编官方新账号最需要的场景（S5）
    let Ok(home) = codex_home() else {
        return Ok(());
    };
    let Ok(content) = std::fs::read_to_string(home.join("auth.json")) else {
        return Ok(());
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Ok(());
    };
    let Some(tokens) = v.get("tokens") else {
        return Ok(());
    };

    let get = |k: &str| {
        tokens
            .get(k)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    let (at, rt, idt) = (get("access_token"), get("refresh_token"), get("id_token"));
    if at.is_empty() && rt.is_empty() {
        return Ok(());
    }

    // id_token 是 JWT：payload(base64) 中的 email 标识归属
    let email_claim = idt
        .split('.')
        .nth(1)
        .and_then(decode_jwt_email)
        .map(|e| crate::account::normalize_email(&e));
    let Some(email) = email_claim else {
        return Ok(());
    };

    let mut changed = false;
    let mut file = file;
    // 匹配按归一化 email + account/org 三元组：同邮箱多 org 不合并（对齐 cockpit-tools）。
    // 官方 account_id 可能在 tokens.account_id 或 access_token JWT 中，任一能解析就参与比对；
    // 老库无 account/org 时退化为纯 email 匹配，保证兼容。
    let official_account_id = tokens
        .get("account_id")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "null")
        .or_else(|| extract_chatgpt_account_id(&at));
    let official_org_id = extract_chatgpt_org_id(&at).or_else(|| extract_org_from_id_token(&idt));
    // 用下标匹配而非 &mut 迭代器：后续分支内还要改 file.current_account_id
    let matched = file.accounts.iter().position(|acc| {
        if normalize_email(&acc.email) != email {
            return false;
        }
        if let (Some(a), Some(b)) = (acc.account_id.as_deref(), official_account_id.as_deref()) {
            let a = a.trim();
            let b = b.trim();
            if !a.is_empty() && !b.is_empty() && a != b {
                return false;
            }
        }
        if let (Some(a), Some(b)) = (acc.organization_id.as_deref(), official_org_id.as_deref()) {
            let a = a.trim();
            let b = b.trim();
            if !a.is_empty() && !b.is_empty() && a != b {
                return false;
            }
        }
        true
    });

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match matched {
        Some(i) => {
            let acc = &mut file.accounts[i];
            if acc.access_token != at && !at.is_empty() {
                acc.access_token = at.clone();
                changed = true;
            }
            if !rt.is_empty() && acc.refresh_token != rt {
                // 官方已轮换 refresh_token，必须采纳新的
                acc.refresh_token = rt.clone();
                changed = true;
            }
            if !idt.is_empty() && acc.id_token != idt {
                // 过期 id_token 不采纳：防止把坏凭据写回库
                if !is_jwt_expired_soon(&idt) {
                    acc.id_token = idt.clone();
                    changed = true;
                }
            }
            // 同步三元组身份（仅当官方能解析出且与库不同时更新，不覆盖为空）
            if let Some(v) = official_account_id.clone() {
                if acc.account_id.as_deref() != Some(v.as_str()) {
                    acc.account_id = Some(v);
                    changed = true;
                }
            }
            if let Some(v) = official_org_id.clone() {
                if acc.organization_id.as_deref() != Some(v.as_str()) {
                    acc.organization_id = Some(v);
                    changed = true;
                }
            }
            // official expires 信息在 auth.json 中无 expires_at；
            // token 变了说明官方刚刷新过（access_token 生命周期通常 ~小时级），
            // 给 1 小时余量：既避免每次启动都无谓刷新，又保证过期前会真正续期
            if changed {
                acc.expires_at = now + HARVEST_VALIDITY_SECS;
                // 官方凭据是新鲜的（重新登录/刚刷新），stale 状态随之解除
                acc.stale = false;
            }
        }
        None => {
            // S5 反向收编：用户在官方 CLI 手动登录了库中不存在的新账号，
            // 采纳为库中新账号，避免官方凭据丢失
            let id = uuid::Uuid::new_v4().to_string();
            file.accounts.push(Account {
                id: id.clone(),
                email: email.clone(),
                access_token: at,
                refresh_token: rt,
                id_token: idt,
                expires_at: now + HARVEST_VALIDITY_SECS,
                stale: false,
                account_id: official_account_id.clone(),
                organization_id: official_org_id.clone(),
            });
            // 新收编的账号就是官方文件的主人 → 使用中
            file.current_account_id = Some(id);
            changed = true;
            crate::emit::emit(format!(
                "📥 发现官方新登录账号（{}），已自动收编入库",
                email
            ));
        }
    }
    // 官方 auth.json 即当前使用凭据：归属账号必须标为使用中
    // （修复收编/更新后 current 缺失 → 徽标、切换守卫、删除守卫全部失效）
    if let Some(i) = matched {
        let acc_id = file.accounts[i].id.clone();
        if file.current_account_id.as_deref() != Some(acc_id.as_str()) {
            file.current_account_id = Some(acc_id);
            changed = true;
        }
    }

    if changed {
        match save_accounts_unlocked(&file) {
            Ok(_) => crate::emit::emit(format!("⟳ 已从官方 auth.json 回收最新凭据（{}）", email)),
            // 保存失败不能静默：凭据已更新但没落盘，下次启动仍会用旧 RT 刷新失败
            Err(e) => {
                crate::emit::emit_err(format!("已回收官方新凭据（{}）但落盘失败：{}", email, e))
            }
        }
    }
    Ok(())
}

/// 解码 JWT payload（base64url JSON）提取 email 字段
pub fn decode_jwt_email(payload_b64: &str) -> Option<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let bytes = URL_SAFE_NO_PAD
        .decode(payload_b64.trim_end_matches('='))
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("email")
        .and_then(|e| e.as_str())
        .map(|s| s.to_string())
}

/// 从 id_token（OIDC JWT，openid scope）提取归一化 email——零网络调用，
/// 登录与 harvest 的统一身份来源
pub fn email_from_id_token(id_token: &str) -> Option<String> {
    id_token
        .split('.')
        .nth(1)
        .and_then(decode_jwt_email)
        .filter(|e| !e.trim().is_empty())
        .map(|e| normalize_email(&e))
}

fn decode_jwt_payload_value(token: &str) -> Option<serde_json::Value> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let payload_b64 = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// JWT exp（秒级）解析，失败返回 None（视为未知，不过度刷新）
pub fn jwt_exp(token: &str) -> Option<i64> {
    decode_jwt_payload_value(token.trim())?.get("exp")?.as_i64()
}

/// JWT 是否已过期或进入 300s 偏斜窗口（对齐 cockpit-tools TOKEN_REFRESH_SKEW_SECONDS）
pub fn is_jwt_expired_soon(token: &str) -> bool {
    let Some(exp) = jwt_exp(token) else {
        return true;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    exp < now + REFRESH_SKEW_SECS as i64
}

fn first_auth_string(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let auth = v.get("https://api.openai.com/auth")?;
    for k in keys {
        if let Some(s) = auth.get(*k).and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

/// 从 access_token JWT 提取 chatgpt_account_id（对齐 cockpit-tools）
pub fn extract_chatgpt_account_id(access_token: &str) -> Option<String> {
    let v = decode_jwt_payload_value(access_token)?;
    first_auth_string(&v, &["chatgpt_account_id", "account_id"])
}

/// 从 access_token JWT 提取 organization_id（多键兼容）
pub fn extract_chatgpt_org_id(access_token: &str) -> Option<String> {
    let v = decode_jwt_payload_value(access_token)?;
    if let Some(id) = first_auth_string(
        &v,
        &[
            "organization_id",
            "chatgpt_organization_id",
            "chatgpt_org_id",
            "org_id",
            "poid",
            "POID",
        ],
    ) {
        return Some(id);
    }
    let auth = v.get("https://api.openai.com/auth")?;
    let orgs = auth.get("organizations")?.as_array()?;
    let def = orgs
        .iter()
        .find(|o| {
            o.get("is_default")
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        })
        .or_else(|| orgs.first())?;
    def.get("id")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn extract_org_from_id_token(id_token: &str) -> Option<String> {
    let v = decode_jwt_payload_value(id_token)?;
    v.get("https://api.openai.com/auth")
        .and_then(|a| {
            a.get("organization_id")
                .or_else(|| a.get("chatgpt_organization_id"))
                .or_else(|| a.get("org_id"))
        })
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 校验刷新回来的 id_token：过期值直接丢弃，沿用旧值（对齐 cockpit resolve_refreshed_id_token）
fn pick_fresh_id_token(new_id: Option<String>, current_id: &str) -> Option<String> {
    let new = new_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    if is_jwt_expired_soon(&new) {
        crate::emit::emit_err("刷新返回的 id_token 已过期，已沿用旧 id_token".to_string());
        return None;
    }
    if new == current_id.trim() {
        return None;
    }
    Some(new)
}

/// access_token 过期或 5 分钟内将过期时，用 refresh_token 静默换新并回存。
/// 刷新失败（如 refresh_token 已被轮换失效）时返回原账号，由官方客户端兜底。
/// force=true 跳过新鲜度检查（配额 401 后的重试链路用）。
fn refresh_if_needed(account: &mut Account, force: bool) -> Result<Account, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if !force && account.expires_at > now + REFRESH_SKEW_SECS {
        return Ok(account.clone()); // 还有效，无需刷新
    }
    if account.refresh_token.trim().is_empty() {
        return Ok(account.clone()); // 无 RT 可刷，交给官方客户端
    }

    crate::emit::emit("⟳ access_token 已过期/将过期，正在静默刷新...".to_string());
    match crate::oauth::refresh_access_token(&account.refresh_token) {
        Ok(tok) => {
            account.access_token = tok.access_token;
            if let Some(rt) = tok.refresh_token.filter(|s| !s.is_empty()) {
                // 官方会轮换 refresh_token，必须存新的
                account.refresh_token = rt;
            }
            if let Some(idt) = pick_fresh_id_token(tok.id_token, &account.id_token) {
                account.id_token = idt;
            }
            // 刷新后同步三元组身份（access_token 轮换后 account/org 可能变化）
            if let Some(v) = extract_chatgpt_account_id(&account.access_token) {
                account.account_id = Some(v);
            }
            if let Some(v) = extract_chatgpt_org_id(&account.access_token) {
                account.organization_id = Some(v);
            }
            account.expires_at = tok
                .expires_in
                .map(|exp| now.saturating_add(exp))
                .unwrap_or_else(|| {
                    // 响应缺 expires_in 且旧值已过期/为0时回退 1 小时，避免下次切换再烧一次 RT
                    if account.expires_at > now {
                        account.expires_at
                    } else {
                        now + HARVEST_VALIDITY_SECS
                    }
                });
            account.stale = false; // 刷新成功即恢复

            // 回存到账号库（调用方已持有 &mut，但账号库文件需同步落盘）
            Ok(account.clone())
        }
        Err(e) => {
            // 区分 401（RT 彻底失效）与其他错误（网络等暂时性问题）：
            // 401 标记 stale 并中止切换——绝不能把死凭据写进官方 auth.json
            // 污染 Codex 当前正常工作的登录态；其他错误保留原状继续
            let lower = e.to_ascii_lowercase();
            let is_auth_failure = lower.contains("401")
                || lower.contains("unauthorized")
                || lower.contains("invalid_grant")
                || lower.contains("refresh_token_reused")
                || lower.contains("token_invalidated")
                || lower.contains("authentication token has been invalidated");
            if is_auth_failure {
                account.stale = true;
                // 立即把 stale 标记落盘（此处返回 Err 后 switch_account 不会走到保存）
                if let Ok(mut f) = load_accounts() {
                    if let Some(a) = f.accounts.iter_mut().find(|a| a.id == account.id) {
                        a.stale = true;
                    }
                    let _ = save_accounts(&f);
                }
                return Err(format!(
                    "账号 {} 的凭据已过期（可能在 Codex 侧轮换时未收编），已拒绝继续以保护当前登录态；请对该账号重新登录复活一次",
                    account.email
                ));
            }
            crate::emit::emit_err(format!("刷新失败（{}），使用现有凭据继续", e));
            Ok(account.clone())
        }
    }
}

/// 取账号并保证 AT 新鲜（配额查询前用）：过期则静默刷新并落盘。
/// 锁只在 load→save 短区间持有，网络配额请求本身在锁外，防长持锁阻塞他实例
pub fn fresh_account(account_id: &str) -> Result<Account, String> {
    with_accounts_lock(|| {
        let mut file = load_accounts()?;
        let idx = file
            .accounts
            .iter()
            .position(|a| a.id == account_id)
            .ok_or_else(|| format!("账号未找到: {}", account_id))?;
        if file.accounts[idx].stale {
            return Err(format!(
                "账号 {} 已失效（用「复活」重新登录一次即可），跳过",
                file.accounts[idx].email
            ));
        }
        let before = file.accounts[idx].clone();
        let account = refresh_if_needed(&mut file.accounts[idx], false)?;
        save_accounts_unlocked(&file)?;
        // 激活账号凭据被轮换后必须同步投影官方 auth.json（S11 先库后官方）：
        // 只落库会让官方侧持有已作废旧 RT（Codex 下次刷新 401 掉登录），
        // 且 TUI 周期 harvest 会把官方旧值回灌库形成双向分叉；非激活账号无此耦合
        let rotated = before.access_token != account.access_token
            || before.refresh_token != account.refresh_token
            || before.id_token != account.id_token;
        if rotated && file.current_account_id.as_deref() == Some(account_id) {
            project_official_auth(&account)?;
        }
        Ok(account)
    })
}

/// 强制刷新账号 token 并落盘（配额 401 后的重试链路用）
pub fn force_refresh_account(account_id: &str) -> Result<Account, String> {
    with_accounts_lock(|| {
        let mut file = load_accounts()?;
        let idx = file
            .accounts
            .iter()
            .position(|a| a.id == account_id)
            .ok_or_else(|| format!("账号未找到: {}", account_id))?;
        let before = file.accounts[idx].clone();
        let account = refresh_if_needed(&mut file.accounts[idx], true)?;
        save_accounts_unlocked(&file)?;
        // 同 fresh_account：激活账号轮换后投影官方文件，防止分叉（S11 先库后官方）
        let rotated = before.access_token != account.access_token
            || before.refresh_token != account.refresh_token
            || before.id_token != account.id_token;
        if rotated && file.current_account_id.as_deref() == Some(account_id) {
            project_official_auth(&account)?;
        }
        Ok(account)
    })
}

/// 切换账号：用目标账号的 token 覆盖 Codex 目录下的 auth.json
///
/// auth.json 字段与官方 codex `AuthDotJson` 对齐（参考 cockpit-tools
/// codex_account_projection.rs 的 build_auth_file_value）：
/// - auth_mode: "chatgpt"
/// - OPENAI_API_KEY: null（OAuth 账号必须显式为 null）
/// - tokens.{id_token, access_token, refresh_token, account_id}
///   （refresh_token 键必须存在，无值时为空串，官方解析器要求）
/// - last_refresh: RFC3339 时间戳
pub fn switch_account(account_id: &str) -> Result<(), String> {
    // harvest 已在命令入口统一执行，这里不再重复
    // 全程持锁：refresh 落盘 + 官方写 + current 更新原子化
    with_accounts_lock(|| switch_locked(account_id))
}

fn switch_locked(account_id: &str) -> Result<(), String> {
    let mut file = load_accounts()?;
    let idx = file
        .accounts
        .iter()
        .position(|a| a.id == account_id)
        .ok_or_else(|| format!("账号未找到: {}", account_id))?;

    // stale 账号拒绝切换：其凭据已失效，覆盖官方 auth.json 会把 Codex
    // 当前正常工作的登录态污染成废凭据（用 r 命令定向复活）
    if file.accounts[idx].stale {
        return Err(format!(
            "账号 {} 的凭据已失效（需重新登录），已拒绝切换以保护当前登录态；可用「复活」定向重新登录该账号",
            file.accounts[idx].email
        ));
    }

    // 切换前静默刷新：access_token 过期或 5 分钟内将过期时，用 refresh_token 换新
    // S11 顺序：先写库再写官方——refresh 已更新内存，先落盘库，再覆盖官方文件，
    // 中间崩溃时库中新 RT 有效，下次切换可自愈；反之库中旧 RT 已作废会误标 stale
    let account = refresh_if_needed(&mut file.accounts[idx], false)?;
    // 坏数据兜底：库中条目缺 access_token（历史损坏/外部篡改）时绝不能投影空凭据
    // 覆盖官方文件（会把 Codex 当前可用登录态洗成空）。对齐 cockpit
    // build_auth_file_value 的“缺 access_token 即错”语义，直接标 stale 指引 r 复活
    if account.access_token.trim().is_empty() {
        file.accounts[idx].stale = true;
        let _ = save_accounts_unlocked(&file);
        return Err(format!(
            "账号 {} 缺少 access_token 且无法刷新，已标为需重新登录（用「复活」处理），未触碰官方登录态",
            account.email
        ));
    }
    // 刷新可能更新了 token，先落盘一次，保证官方写失败/崩溃时库中仍是新 RT
    save_accounts_unlocked(&file)?;

    project_official_auth(&account)?;
    let codex_home = codex_home()?;

    // OAuth 切号后：重置 config.toml 中的自定义 provider 路由，保证走官方内置链路；
    // 最佳努力，失败只告警（keychain 同步已并入 project_official_auth，切换/刷新两路共用）
    reset_oauth_provider_in_config(&codex_home);

    file.current_account_id = Some(account_id.to_string());
    save_accounts_unlocked(&file)?;

    Ok(())
}

/// 把账号当前凭据投影到官方 auth.json（merge 写，保留未知顶层字段，对齐 cockpit-tools）。
/// 切换与刷新两路共用：激活账号凭据轮换后必须同步官方文件（S11：先写库、后写官方）。
/// 不含 config.toml 的 provider 重置（那是切号语义，同账号刷新无身份变化）。
fn project_official_auth(account: &Account) -> Result<(), String> {
    let codex_home = codex_home()?;
    std::fs::create_dir_all(&codex_home).map_err(|e| format!("创建 Codex 目录失败: {}", e))?;

    let last_refresh = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let last_refresh_rfc3339 = chrono_rfc3339(last_refresh);

    let next_tokens = serde_json::json!({
        "id_token": account.id_token.clone(),
        "access_token": account.access_token.clone(),
        // 官方解析器要求 refresh_token 键必须存在
        "refresh_token": account.refresh_token.clone(),
        // 未知 account_id 写 null（非 ""），与官方/上游形态一致，避免下游当有效 ID 发出
        "account_id": account.account_id.clone().filter(|s| !s.trim().is_empty()).map(serde_json::Value::String).unwrap_or(serde_json::Value::Null)
    });

    let auth_path = codex_home.join("auth.json");
    let mut merged = std::fs::read_to_string(&auth_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    // 清掉旧 token/身份残留，写入新值（"type" 保留原值，不强制改写）
    for k in [
        "auth_mode",
        "OPENAI_API_KEY",
        "tokens",
        "last_refresh",
        "expired",
        "expires_in",
        "timestamp",
    ] {
        merged.remove(k);
    }
    merged.insert(
        "auth_mode".to_string(),
        serde_json::Value::String("chatgpt".to_string()),
    );
    merged.insert("OPENAI_API_KEY".to_string(), serde_json::Value::Null);
    merged.insert("tokens".to_string(), next_tokens);
    merged.insert(
        "last_refresh".to_string(),
        serde_json::Value::String(last_refresh_rfc3339),
    );

    let content = serde_json::to_string_pretty(&serde_json::Value::Object(merged))
        .map_err(|e| format!("序列化 auth.json 失败: {}", e))?;
    atomic_write(&auth_path, &content)?;
    // macOS 同步 keychain（官方客户端可能从 keychain 读），最佳努力，失败只告警
    sync_keychain_best_effort(&codex_home, &auth_path);
    Ok(())
}

/// OAuth 切号后重置 config.toml：移除自定义 model_provider/openai_base_url 路由
/// 无 toml 解析依赖，按行过滤（保留注释与未知配置），缺文件直接返回
fn reset_oauth_provider_in_config(codex_home: &std::path::Path) {
    use std::io::BufRead;
    let path = codex_home.join("config.toml");
    if !path.exists() {
        return;
    }
    let Ok(f) = std::fs::File::open(&path) else {
        return;
    };
    let mut kept = Vec::new();
    let mut removed = 0;
    for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        let t = line.trim_start();
        // 跳过注释行误删；仅处理顶层键
        if !t.starts_with('#')
            && (t.starts_with("model_provider") && t.contains('=')
                || t.starts_with("openai_base_url") && t.contains('='))
        {
            removed += 1;
            continue;
        }
        kept.push(line);
    }
    if removed == 0 {
        return;
    }
    kept.push(String::new());
    let content = kept.join("\n");
    // 先备份原文件（回滚用的是改之前的版本），再写新内容；经 600 直写避免 umask 窗口
    if let Ok(orig) = std::fs::read_to_string(&path) {
        let _ = write_file_private(&backup_path_for(&path), &orig);
    }
    let tmp = temp_path_for(&path);
    let _ = std::fs::remove_file(&tmp);
    if write_file_private(&tmp, &content).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, &path);
}

/// macOS keychain 同步（最佳努力）：service="Codex Auth"，account=cli|<home hash 前16>
/// 非 macOS 为空操作；失败只告警不阻断切换（对齐 cockpit-tools）。
///
/// 已知风险：secret 经 `-w` 命令行参数传递，同机其他用户 `ps` 可见。
/// macOS `security` 未提供 stdin 传参，暂接受该风险（多用户共享机慎用 keychain 同步）；
/// 纯文件模式（Linux/Windows/无钥匙串）不受影响。
fn sync_keychain_best_effort(codex_home: &std::path::Path, auth_path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        use sha2::{Digest, Sha256};
        let resolved =
            std::fs::canonicalize(codex_home).unwrap_or_else(|_| codex_home.to_path_buf());
        let mut h = Sha256::new();
        h.update(resolved.to_string_lossy().as_bytes());
        let hex = format!("{:x}", h.finalize());
        let account = format!("cli|{}", &hex[..16.min(hex.len())]);
        let Ok(secret) = std::fs::read_to_string(auth_path) else {
            return;
        };
        let out = std::process::Command::new("security")
            .arg("add-generic-password")
            .arg("-U")
            .arg("-s")
            .arg("Codex Auth")
            .arg("-a")
            .arg(&account)
            .arg("-w")
            .arg(&secret)
            .output();
        if let Ok(o) = out {
            if !o.status.success() {
                crate::emit::emit_err("keychain 同步失败，官方客户端可能仍读旧凭据".to_string());
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (codex_home, auth_path);
    }
}

/// 秒级 Unix 时间戳 → RFC3339（UTC），避免为这一处引入 chrono
fn chrono_rfc3339(unix_secs: u64) -> String {
    let days = (unix_secs / 86400) as i64;
    let secs_of_day = unix_secs % 86400;
    let (h, m, s) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );

    // civil_from_days（Howard Hinnant 算法）
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, d, h, m, s
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    // 基准值由 Python datetime(UTC) 生成，覆盖：普通日期、纪元起点、
    // 2100 年（非闰年世纪年）、2000 年（闰年）
    #[test]
    fn chrono_rfc3339_matches_python_baseline() {
        let vectors = [
            (1_758_337_200u64, "2025-09-20T03:00:00Z"),
            (0, "1970-01-01T00:00:00Z"),
            (4_102_444_800, "2100-01-01T00:00:00Z"),
            (951_782_401, "2000-02-29T00:00:01Z"),
        ];
        for (ts, expected) in vectors {
            assert_eq!(chrono_rfc3339(ts), expected, "ts={}", ts);
        }
    }

    fn make_jwt(exp: i64) -> String {
        let h = URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#);
        let p = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{}}}"#, exp));
        format!("{}.{}.sig", h, p)
    }

    #[test]
    fn jwt_exp_and_skew_window() {
        let now = now_secs() as i64;
        assert!(!is_jwt_expired_soon(&make_jwt(now + 3600)));
        assert!(is_jwt_expired_soon(&make_jwt(now + 60)));
        assert!(is_jwt_expired_soon("not-a-jwt"));
    }

    #[test]
    fn fresh_id_token_picker_rejects_expired() {
        let now = now_secs() as i64;
        let fresh = make_jwt(now + 3600);
        let expired = make_jwt(now - 3600);
        assert_eq!(pick_fresh_id_token(Some(fresh.clone()), "old"), Some(fresh));
        assert_eq!(pick_fresh_id_token(Some(expired), "old"), None);
        assert_eq!(pick_fresh_id_token(Some("old".to_string()), "old"), None);
    }

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(normalize_email("  Foo@Bar.COM "), "foo@bar.com");
    }

    #[test]
    fn temp_and_backup_paths_are_unique() {
        let p = std::path::Path::new("/tmp/auth.json");
        let t = temp_path_for(p).to_string_lossy().to_string();
        assert!(t.contains(".tmp.") && t.contains(&std::process::id().to_string()));
        assert_eq!(
            backup_path_for(p),
            std::path::Path::new("/tmp/auth.json.bak")
        );
    }

    #[test]
    #[cfg(unix)]
    fn atomic_write_keeps_backup_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("hangar-test-{}-{}", std::process::id(), now_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("accounts.json");
        std::fs::write(&target, r#"{"accounts":[],"current_account_id":null}"#).unwrap();
        atomic_write(&target, r#"{"accounts":[],"current_account_id":"x"}"#).unwrap();
        let bak = backup_path_for(&target);
        assert!(bak.exists());
        let mode = std::fs::metadata(&bak).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "备份含明文 token，必须仅属主可读写");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
