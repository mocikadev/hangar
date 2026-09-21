//! Codex 配额查询：GET chatgpt.com/backend-api/wham/usage（对齐 cockpit-tools codex_quota）。
//!
//! - 头：`Authorization: Bearer <access_token>`（必备）+ `ChatGPT-Account-Id`（关键，多账号必带）
//! - 响应：`{plan_type, rate_limit: {primary_window, secondary_window}}`，剩余 = 100 - used_percent
//! - 窗口种类按 `limit_window_seconds` 判定（社区血泪：primary/secondary 位置不可信，
//!   实测有账号 primary 即 8 天窗口）；缺失的窗口直接略过，不硬凑“5h/周”
//! - 非公开契约：解析全容错，失败只影响单账号展示，不阻断其他账号

use std::time::Duration;

use crate::account::Account;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Clone, Default)]
pub struct QuotaWindow {
    /// 剩余额度百分比（0-100），未知为 None
    pub remaining: Option<i32>,
    /// 窗口时长（秒）：当前仅用其判分类，保留字段备后续展示
    #[allow(dead_code)]
    pub limit_secs: Option<i64>,
    /// 距重置秒数（由 reset_after_seconds 换算），未知为 None
    pub reset_in_secs: Option<i64>,
    /// 重置时间戳（reset_at），未知为 None
    pub reset_at: Option<i64>,
}

/// 按窗口时长分类展示名（位置不可信，见模块注释）
pub fn classify_window(limit_secs: Option<i64>) -> String {
    match limit_secs {
        // 会话窗：18000（5h）±60s
        Some(s) if (s - 18_000).abs() <= 60 => "会话5h".to_string(),
        // 周窗：实测 604800 与 614801 两种，取宽区间
        Some(s) if (500_000..700_000).contains(&s) => "周".to_string(),
        // 月窗：实测 1692000 / 2582100，取宽区间
        Some(s) if s > 1_000_000 => "月".to_string(),
        // 未知时长：按时长直述，避免错标
        Some(s) if s >= 3600 => format!("{}h窗口", s / 3600),
        Some(s) => format!("{}m窗口", (s + 59) / 60),
        None => "窗口".to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct NamedWindow {
    pub label: String,
    pub window: QuotaWindow,
}

#[derive(Debug, Clone, Default)]
pub struct Quota {
    pub plan: Option<String>,
    /// 按服务端下发顺序排列（primary, secondary），缺失的直接略过
    pub windows: Vec<NamedWindow>,
    /// usage 自带重置卡数量摘要（免额外请求）
    pub reset_available: Option<i64>,
    /// 重置卡明细（best-effort，失败为 None）
    pub reset_detail: Option<ResetCredits>,
}

fn parse_window(v: Option<&serde_json::Value>) -> Option<NamedWindow> {
    let w = v?;
    if w.get("used_percent").and_then(|x| x.as_i64()).is_none()
        && w.get("limit_window_seconds")
            .and_then(|x| x.as_i64())
            .is_none()
    {
        return None;
    }
    let remaining = w
        .get("used_percent")
        .and_then(|x| x.as_i64())
        .map(|u| 100 - (u.clamp(0, 100) as i32));
    let limit_secs = w
        .get("limit_window_seconds")
        .and_then(|x| x.as_i64())
        .filter(|s| *s > 0);
    let reset_in_secs = w
        .get("reset_after_seconds")
        .and_then(|x| x.as_i64())
        .filter(|s| *s >= 0);
    let reset_at = w.get("reset_at").and_then(|x| x.as_i64());
    let label = classify_window(limit_secs);
    Some(NamedWindow {
        label,
        window: QuotaWindow {
            remaining,
            limit_secs,
            reset_in_secs,
            reset_at,
        },
    })
}

pub fn parse_quota(body: &str) -> Result<Quota, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("解析配额响应失败: {}", e))?;
    let rate = v.get("rate_limit");
    let mut windows = vec![];
    for key in ["primary_window", "secondary_window"] {
        if let Some(w) = parse_window(rate.and_then(|r| r.get(key))) {
            windows.push(w);
        }
    }
    // 重置卡数量摘要：顶层或 rate_limit 下的 rate_limit_reset_credits.available_count
    let reset_available = v
        .get("rate_limit_reset_credits")
        .or_else(|| rate.and_then(|r| r.get("rate_limit_reset_credits")))
        .and_then(|r| r.get("available_count"))
        .and_then(|x| x.as_i64());
    Ok(Quota {
        plan: v
            .get("plan_type")
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(plan_display),
        windows,
        reset_available,
        reset_detail: None,
    })
}

/// 计划展示名：plan_type 为计费档位；pro 需按 CPA/cockpit 规则补倍率
/// （显式 prolite/pro-5x → 5x，其余 pro 一律按 20x/Pro Max 展示）
fn plan_display(raw: String) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower != "pro" {
        return raw;
    }
    // usage 响应无 auth_file_plan_type，但若 plan_type 本身带档位后缀则尊重
    if lower.contains("5x") || lower.contains("lite") {
        return "Pro 5x".to_string();
    }
    "Pro 20x".to_string()
}

fn detail_code(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("detail")
        .and_then(|d| d.get("code"))
        .and_then(|c| c.as_str())
        .or_else(|| {
            v.get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_str())
        })
        .or_else(|| v.get("code").and_then(|c| c.as_str()))
        .map(|s| s.to_string())
}

/// 单次配额请求（不刷新 token，401 由上层决定是否强制刷新重试）
fn fetch_once(access_token: &str, account_id: Option<&str>) -> Result<Quota, String> {
    let mut req = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .build()
        .get(USAGE_URL)
        .set("Authorization", &format!("Bearer {}", access_token))
        .set("Accept", "application/json");
    if let Some(id) = account_id.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.set("ChatGPT-Account-Id", id);
    }
    let resp = req.call().map_err(|e| match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            let mut msg = format!("配额请求失败: HTTP {}", code);
            if let Some(c) = detail_code(&body) {
                msg.push_str(&format!(", error_code={}", c));
            }
            msg.push_str(&format!(", body_len={}", body.len()));
            msg
        }
        other => format!("配额请求失败: {}", other),
    })?;
    let body = resp
        .into_string()
        .map_err(|e| format!("读取配额响应失败: {}", e))?;
    parse_quota(&body)
}

/// 查指定账号配额：先保证 AT 新鲜 → 请求 → 401 则强制刷新后重试一次。
/// 重置卡详情 best-effort 附带（失败不影响配额行，只读，不做 consume）
pub fn fetch_quota_for_account(account_id: &str) -> Result<(Account, Quota), String> {
    let acc = crate::account::fresh_account(account_id)?;
    let mut q = match fetch_once(&acc.access_token, acc.account_id.as_deref()) {
        Ok(q) => q,
        Err(e) if e.contains("401") || e.to_ascii_lowercase().contains("unauthorized") => {
            let acc2 = crate::account::force_refresh_account(account_id)?;
            let q = fetch_once(&acc2.access_token, acc2.account_id.as_deref())?;
            let q = with_reset_detail(q, &acc2);
            return Ok((acc2, q));
        }
        Err(e) => return Err(e),
    };
    q = with_reset_detail(q, &acc);
    Ok((acc, q))
}

fn with_reset_detail(mut q: Quota, acc: &Account) -> Quota {
    q.reset_detail = fetch_reset_credits(&acc.access_token, acc.account_id.as_deref()).ok();
    q
}

// ---------------------------------------------------------------------------
// 重置卡（只读）：
// - 数量摘要：wham/usage 自带 `rate_limit_reset_credits.available_count`（免额外请求）
// - 明细：GET wham/rate-limit-reset-credits → credits[{id,status,title,
//   granted_at,expires_at}]，时间戳可能是 unix 秒或 RFC3339 字符串
// - 注意：consume（核销）接口存在但属破坏性操作，本工具不接
// ---------------------------------------------------------------------------

const RESET_CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";

#[derive(Debug, Clone)]
pub struct ResetCredit {
    /// 展示名（如 "Full reset (Weekly + 5 hr)"），当前未展示，保留备扩展
    #[allow(dead_code)]
    pub title: Option<String>,
    pub available: bool,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ResetCredits {
    pub available: i64,
    /// 累计获得数（含已用），当前未展示，保留备扩展
    #[allow(dead_code)]
    pub total_earned: Option<i64>,
    pub credits: Vec<ResetCredit>,
}

impl ResetCredits {
    /// 可用卡中最早到期（UTC 秒），无到期信息返回 None
    pub fn next_expiry(&self) -> Option<i64> {
        self.credits
            .iter()
            .filter(|c| c.available)
            .filter_map(|c| c.expires_at)
            .min()
    }
}

/// 时间戳兼容解析：unix 秒（int/float）或 RFC3339（"2026-07-17T00:00:00Z"）
fn parse_ts(v: &serde_json::Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(f) = v.as_f64() {
        return Some(f as i64);
    }
    let s = v.as_str()?.trim();
    let (date, time) = s.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let (y, m, d): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let time = time.trim_end_matches('Z').trim_end_matches('z');
    let time = time.split(['+', '-']).next().unwrap_or(time);
    let mut t = time.split(':');
    let (hh, mm): (i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 {
        return None;
    }
    // 天数累加（1970 起，够用到 2100，无闰秒需求）
    let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mut days: i64 = 0;
    for yr in 1970..y {
        days += if leap(yr) { 366 } else { 365 };
    }
    let lens = [
        31,
        if leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for mo in 1..m {
        days += lens[(mo - 1) as usize];
    }
    days += d - 1;
    Some(days * 86400 + hh * 3600 + mm * 60)
}

fn first_str(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        v.get(*k)
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

fn first_ts(v: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|k| v.get(*k).and_then(parse_ts))
}

fn parse_reset_credit(record: &serde_json::Value) -> Option<ResetCredit> {
    let status_raw = first_str(record, &["status", "state"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    let expires_at = first_ts(record, &["expires_at", "expire_at", "expiresAt"]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // available 判定：明确已用/已过期状态排除 + 过期时间已过排除
    let available = !matches!(
        status_raw.as_str(),
        "redeemed" | "consumed" | "used" | "expired" | "invalid" | "revoked"
    ) && expires_at.map(|t| t > now).unwrap_or(true);
    Some(ResetCredit {
        title: first_str(record, &["title", "name"]),
        available,
        expires_at,
    })
}

pub fn parse_reset_credits(body: &str) -> Result<ResetCredits, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("解析重置卡响应失败: {}", e))?;
    let arr = v
        .get("credits")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let credits: Vec<ResetCredit> = arr.iter().filter_map(parse_reset_credit).collect();
    let available = v
        .get("available_count")
        .and_then(|x| x.as_i64())
        .unwrap_or_else(|| credits.iter().filter(|c| c.available).count() as i64);
    Ok(ResetCredits {
        available: available.max(0),
        total_earned: v.get("total_earned_count").and_then(|x| x.as_i64()),
        credits,
    })
}

fn fetch_reset_credits(
    access_token: &str,
    account_id: Option<&str>,
) -> Result<ResetCredits, String> {
    let mut req = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .build()
        .get(RESET_CREDITS_URL)
        .set("Authorization", &format!("Bearer {}", access_token))
        .set("Accept", "application/json");
    if let Some(id) = account_id.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.set("ChatGPT-Account-Id", id);
    }
    let resp = req.call().map_err(|e| match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            format!("重置卡请求失败: HTTP {} body_len={}", code, body.len())
        }
        other => format!("重置卡请求失败: {}", other),
    })?;
    let body = resp
        .into_string()
        .map_err(|e| format!("读取重置卡响应失败: {}", e))?;
    parse_reset_credits(&body)
}

/// 窗口重置的绝对时间戳（UTC 秒）：优先 reset_at，缺失时 now+reset_after 换算
pub fn reset_at_ts(w: &QuotaWindow, now: i64) -> Option<i64> {
    w.reset_at.or_else(|| w.reset_in_secs.map(|s| now + s))
}

/// UTC 秒时间戳 → 本地时区 "YYYY-MM-DD HH:mm"（无第三方库：libc timezone 偏移）。
/// TUI 与 classic 共用的统一时间展示
pub fn fmt_ts_local(ts: i64) -> String {
    let offset = local_utc_offset_secs(ts);
    let local = ts + offset;
    let days = local.div_euclid(86400);
    let secs = local.rem_euclid(86400);
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    // civil_from_days（与 codex_account 同源算法）
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
    format!("{}-{:02}-{:02} {:02}:{:02}", year, month, d, h, m)
}

/// 本地时区相对 UTC 的偏移秒（东正西负）。取该时间戳当天 UTC 正午采样，
/// 规避 DST 边界处 `localtime` 回推的歧义；无 libc 平台回退 0（UTC）
fn local_utc_offset_secs(ts: i64) -> i64 {
    #[cfg(unix)]
    {
        let sample = ts - ts.rem_euclid(86400) + 43200;
        // tm_zone 在 macOS 是 *mut、Linux 是 *const，字面量初始化无法兼顾；
        // localtime_r 会重写全部字段，零初始化最可移植
        // SAFETY: libc::tm 为纯数据结构，全零是合法初值且随后被 localtime_r 覆盖
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let t: libc::time_t = sample as libc::time_t;
        unsafe {
            if libc::localtime_r(&t, &mut tm).is_null() {
                return 0;
            }
        }
        tm.tm_gmtoff as i64
    }
    #[cfg(not(unix))]
    {
        let _ = ts;
        0
    }
}

/// 重置倒计时秒数：优先相对值，缺失时用绝对 reset_at 换算
pub fn reset_in_secs(w: &QuotaWindow, now: i64) -> Option<i64> {
    if let Some(s) = w.reset_in_secs {
        return Some(s);
    }
    w.reset_at.map(|t| (t - now).max(0))
}

/// 秒数 → "3h12m" / "45m" / "20s"（未知显示 -）
pub fn fmt_countdown(secs: Option<i64>) -> String {
    match secs {
        None => "-".to_string(),
        Some(s) if s <= 0 => "即将重置".to_string(),
        Some(s) => {
            let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
            if h > 0 {
                format!("{}h{:02}m", h, m)
            } else if m > 0 {
                format!("{}m{:02}s", m, sec)
            } else {
                format!("{}s", sec)
            }
        }
    }
}

/// 剩余百分比 → 10 格 ASCII 条（未知显示 ----------）
pub fn quota_bar(remaining: Option<i32>) -> String {
    match remaining {
        None => "----------".to_string(),
        Some(p) => {
            let fill = (p.clamp(0, 100) as usize * 10) / 100;
            format!("{}{}", "█".repeat(fill), "░".repeat(10 - fill))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "plan_type": "plus",
        "rate_limit": {
            "primary_window": {"used_percent": 30, "limit_window_seconds": 18000, "reset_after_seconds": 7543},
            "secondary_window": {"used_percent": 80, "limit_window_seconds": 604800, "reset_at": 1790000000}
        }
    }"#;

    #[test]
    fn parse_sample_windows() {
        let q = parse_quota(SAMPLE).expect("parse");
        assert_eq!(q.plan.as_deref(), Some("plus"));
        assert_eq!(q.windows.len(), 2);
        assert_eq!(q.windows[0].label, "会话5h");
        assert_eq!(q.windows[0].window.remaining, Some(70));
        assert_eq!(q.windows[0].window.reset_in_secs, Some(7543));
        assert_eq!(q.windows[1].label, "周");
        assert_eq!(q.windows[1].window.remaining, Some(20));
        assert_eq!(q.windows[1].window.reset_at, Some(1790000000));
    }

    #[test]
    fn weekly_primary_is_not_mislabeled_session() {
        // 用户实测：primary 即周窗口（limit ~604800），必须标“周”而非“5h”
        let q = parse_quota(
            r#"{"rate_limit":{"primary_window":{"used_percent":8,"limit_window_seconds":604800,"reset_after_seconds":512000}}}"#,
        )
        .expect("parse");
        assert_eq!(q.windows.len(), 1);
        assert_eq!(q.windows[0].label, "周");
        assert_eq!(q.windows[0].window.remaining, Some(92));
    }

    #[test]
    fn classify_known_variants() {
        assert_eq!(classify_window(Some(18000)), "会话5h");
        assert_eq!(classify_window(Some(614801)), "周");
        assert_eq!(classify_window(Some(1692000)), "月");
        assert_eq!(classify_window(Some(2582100)), "月");
        assert_eq!(classify_window(Some(72000)), "20h窗口");
        assert_eq!(classify_window(None), "窗口");
    }

    #[test]
    fn parse_missing_windows_is_empty() {
        let q = parse_quota(r#"{"plan_type":"team"}"#).expect("parse");
        assert!(q.windows.is_empty());
    }

    #[test]
    fn used_percent_clamped() {
        let q = parse_quota(r#"{"rate_limit":{"primary_window":{"used_percent":150}}}"#)
            .expect("parse");
        assert_eq!(q.windows.len(), 1);
        assert_eq!(q.windows[0].window.remaining, Some(0));
    }

    #[test]
    fn countdown_and_bar() {
        assert_eq!(fmt_countdown(Some(7543)), "2h05m");
        assert_eq!(fmt_countdown(None), "-");
        assert_eq!(quota_bar(Some(70)), "███████░░░");
        assert_eq!(quota_bar(None), "----------");
    }

    #[test]
    fn fmt_ts_local_utc_baseline() {
        // 时区无关断言：偏移 0 的环境下应返回 1970-01-01 00:00
        // （非 UTC 机器上仅验证格式，具体数值由 localtime 决定）
        let s = fmt_ts_local(0);
        assert_eq!(s.len(), 16);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], " ");
        assert_eq!(&s[13..14], ":");
    }
}
