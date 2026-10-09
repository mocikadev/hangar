//! Menus are a projection of the existing dashboard; opening one never queries Core.
use super::{Action, Item};
use crate::model::{Card, QuotaState};
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct AccountItem {
    id: String,
    label: String,
    current: bool,
    enabled: bool,
}

pub(super) fn label(value: &str) -> String {
    value.replace('\0', "�").replace('_', "__")
}

impl AccountItem {
    pub(super) fn new(card: &Card, busy: bool) -> Self {
        let quota = if card.account.stale {
            "需重新登录".into()
        } else {
            card.remaining()
                .map(|n| format!("周剩余 {n}%"))
                .unwrap_or_else(|| "周额度未知".into())
        };
        let state = if card.account.stale {
            ""
        } else {
            match card.state {
                QuotaState::Old => " · 旧数据",
                QuotaState::Failed => " · 刷新失败",
                QuotaState::Loading => " · 刷新中",
                _ => "",
            }
        };
        Self {
            id: card.account.id.clone(),
            label: format!(
                "{} · {quota}{state}{}",
                label(&card.account.email),
                if card.recommended { " · 推荐" } else { "" }
            ),
            current: card.account.current,
            enabled: !busy && !card.account.current && !card.account.stale,
        }
    }
}

pub(super) fn build(item: &Item) -> Vec<MenuItem<Item>> {
    let mut menu = vec![
        StandardItem {
            label: item.view.current.clone(),
            enabled: false,
            ..Default::default()
        }
        .into(),
        MenuItem::Separator,
    ];
    menu.extend(item.view.accounts.iter().map(|a| {
        let id = a.id.clone();
        CheckmarkItem {
            label: a.label.clone(),
            checked: a.current,
            enabled: a.enabled,
            activate: Box::new(move |item: &mut Item| {
                if item.view.accounts.iter().any(|a| a.id == id && a.enabled) {
                    item.send(Action::Switch(id.clone()));
                }
            }),
            ..Default::default()
        }
        .into()
    }));
    menu.push(MenuItem::Separator);
    menu.push(
        StandardItem {
            label: if item.view.busy {
                "任务进行中…"
            } else {
                "刷新周额度"
            }
            .into(),
            enabled: !item.view.busy,
            activate: Box::new(|item: &mut Item| {
                if !item.view.busy {
                    item.send(Action::Refresh);
                }
            }),
            ..Default::default()
        }
        .into(),
    );
    menu.push(
        StandardItem {
            label: "显示 Hangar".into(),
            activate: Box::new(|item: &mut Item| item.send(Action::Show)),
            ..Default::default()
        }
        .into(),
    );
    menu.push(
        StandardItem {
            label: "退出 Hangar".into(),
            activate: Box::new(|item: &mut Item| item.send(Action::Quit)),
            ..Default::default()
        }
        .into(),
    );
    menu
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::Card,
        tray::{Event, View},
    };
    use hangar_core::summary::{AccessTokenSummaryState, AccountSummary, TokenHealthSummary};
    use std::sync::mpsc;

    fn card() -> Card {
        Card {
            account: AccountSummary {
                id: "a".into(),
                email: "under_score@example.invalid".into(),
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

    #[test]
    fn menu_projection_preserves_unknown_and_blocks_current_stale_and_busy_accounts() {
        let mut card = card();
        let row = AccountItem::new(&card, false);
        assert!(row.enabled);
        assert!(row.label.contains("under__score"));
        assert!(row.label.contains("未知"));
        assert!(!row.label.contains("0%"));
        assert!(!AccountItem::new(&card, true).enabled);
        card.account.current = true;
        assert!(!AccountItem::new(&card, false).enabled);
        card.account.current = false;
        card.account.stale = true;
        assert!(!AccountItem::new(&card, false).enabled);
    }

    #[test]
    fn opening_menu_is_read_only_and_old_menu_callbacks_recheck_latest_state() {
        let (events, receiver) = mpsc::channel();
        let mut item = Item {
            view: View {
                current: "当前".into(),
                accounts: vec![AccountItem::new(&card(), false)],
                busy: false,
            },
            events,
            icon: vec![],
        };
        let mut menu = build(&item);
        assert!(receiver.try_recv().is_err());
        let MenuItem::Checkmark(action) = &mut menu[2] else {
            panic!("account action");
        };
        item.view.accounts[0].enabled = false;
        (action.activate)(&mut item);
        assert!(receiver.try_recv().is_err());
        item.view.accounts[0].enabled = true;
        (action.activate)(&mut item);
        assert!(matches!(receiver.try_recv(), Ok(Event::Action(Action::Switch(id))) if id == "a"));
    }
}
