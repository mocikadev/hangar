//! Card-only wording. Core owns quota facts, freshness and credential rules.
use crate::model::{Card, QuotaState};

pub(super) fn quota_details(card: &Card) -> String {
    let quota = card.quota.as_ref();
    let reset = quota.and_then(|q| q.weekly_reset_at).map_or_else(
        || "周额度重置时间未知".into(),
        |ts| format!("周额度重置于 {}", timestamp(ts)),
    );
    let count = quota.and_then(|q| q.reset_available);
    let count_text = count.map_or("重置卡数量未知".into(), |n| format!("重置卡 {n} 张"));
    let expiry = match count {
        Some(n) if n <= 0 => "暂无可用重置卡".into(),
        _ => quota.and_then(|q| q.reset_next_expiry).map_or_else(
            || "重置卡到期时间未知".into(),
            |ts| format!("最早到期 {}", timestamp(ts)),
        ),
    };
    format!("{reset}\n{count_text}\n{expiry}")
}

pub(super) fn cache_status(card: &Card) -> String {
    if card.account.stale {
        return "需要重新登录".into();
    }
    // A timestamp alone isn't displayable quota data; partial quota fields are.
    let cached = card.quota.as_ref().is_some_and(|q| {
        q.weekly_remaining.is_some()
            || q.weekly_reset_at.is_some()
            || q.reset_available.is_some()
            || q.reset_next_expiry.is_some()
    });
    let state = match card.state {
        QuotaState::Failed if cached => "刷新失败，旧数据仅供参考",
        QuotaState::Failed => "额度获取失败",
        QuotaState::Loading if cached => "正在刷新…",
        QuotaState::Loading => "正在获取周额度…",
        QuotaState::Old if cached => "旧数据，仅供参考",
        _ if !cached => "尚无额度数据",
        _ => "",
    };
    let updated = card
        .fetched_at
        .and_then(|t| i64::try_from(t).ok())
        .filter(|_| cached)
        .map(|ts| {
            let prefix = if card.state == QuotaState::Failed {
                "上次成功"
            } else {
                "更新于"
            };
            format!("{prefix} {}", timestamp(ts))
        });
    [Some(state.to_owned()).filter(|s| !s.is_empty()), updated]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n")
}

fn timestamp(ts: i64) -> String {
    // Preserve Core's timezone calculation; only adapt the card typography.
    let local = hangar_core::fmt_ts_local(ts);
    let Some((date, time)) = local.split_once(' ') else {
        return local;
    };
    let mut parts = date.rsplitn(3, '-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(day), Some(month), Some(year)) => format!(
            "{year}年{}月{}日 {time}",
            month.trim_start_matches('0'),
            day.trim_start_matches('0')
        ),
        _ => local,
    }
}

#[cfg(test)]
mod tests;
