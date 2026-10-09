use super::*;
use hangar_core::summary::{
    AccessTokenSummaryState, AccountSummary, QuotaSummary, TokenHealthSummary,
};

// Display facts only: no credentials or service success is fabricated.
fn card() -> Card {
    Card {
        account: AccountSummary {
            id: "display-only".into(),
            email: "display@example.invalid".into(),
            current: false,
            stale: false,
            account_id: None,
            organization_id: None,
            expires_at: None,
            token_health: TokenHealthSummary {
                status: AccessTokenSummaryState::Unknown,
                expiry_source: None,
                refresh_available: false,
            },
        },
        quota: None,
        fetched_at: None,
        state: QuotaState::Unknown,
        recommended: false,
    }
}

fn quota() -> QuotaSummary {
    QuotaSummary {
        plan: None,
        weekly_remaining: None,
        weekly_reset_at: None,
        reset_available: Some(0),
        reset_next_expiry: None,
    }
}

#[test]
fn failure_and_loading_distinguish_first_request_from_partial_cached_data() {
    let mut card = card();
    card.state = QuotaState::Failed;
    assert_eq!(cache_status(&card), "额度获取失败");
    // A timestamp or plan without usable quota must not claim old data exists.
    card.fetched_at = Some(0);
    assert_eq!(cache_status(&card), "额度获取失败");
    card.state = QuotaState::Loading;
    assert_eq!(cache_status(&card), "正在获取周额度…");
    card.quota = Some(quota());
    assert!(cache_status(&card).starts_with("正在刷新…\n更新于 "));
    card.state = QuotaState::Failed;
    assert!(cache_status(&card).starts_with("刷新失败，旧数据仅供参考\n上次成功 "));
    card.fetched_at = None;
    assert_eq!(cache_status(&card), "刷新失败，旧数据仅供参考");
    card.account.stale = true;
    assert_eq!(cache_status(&card), "需要重新登录");
}

#[test]
fn unknown_weekly_quota_keeps_reset_facts_and_zero_has_no_expiry_placeholder() {
    let mut card = card();
    assert!(quota_details(&card).contains("重置卡数量未知"));
    card.quota = Some(quota());
    assert_eq!(card.remaining(), None);
    let text = quota_details(&card);
    assert!(text.contains("重置卡 0 张\n暂无可用重置卡"));
    assert!(!text.contains("到期时间未知"));
    card.quota.as_mut().unwrap().reset_available = Some(2);
    assert!(quota_details(&card).contains("重置卡 2 张\n重置卡到期时间未知"));
    card.quota.as_mut().unwrap().reset_next_expiry = Some(0);
    assert!(quota_details(&card).contains("最早到期 "));
    card.quota.as_mut().unwrap().reset_available = None;
    assert!(quota_details(&card).contains("重置卡数量未知\n最早到期 "));
}

#[test]
fn dates_keep_core_local_time_and_use_chinese_card_format() {
    // Epoch is a formatting fixture, not an account/token expiry.
    let local = hangar_core::fmt_ts_local(0);
    let text = timestamp(0);
    assert!(text.contains('年') && text.contains('月') && text.contains('日'));
    assert!(text.ends_with(local.split_once(' ').unwrap().1));
    assert!(!text.contains("月0"));
}
