//! Cache/recommendation adapters and serial refresh. Core owns all policy and persistence.
use super::Event;
use crate::model::{Card, QuotaState};
use hangar_core::{
    account::AccountsFile,
    quota_cache::{self, Entry},
    summary::{AccountSummary, QuotaSummary},
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};

pub struct Attempt {
    at: u64,
    failed: bool,
}

fn eligible(last: Option<&Attempt>, now: u64, force: bool) -> bool {
    force || last.is_none_or(|last| now < last.at || now - last.at >= quota_cache::TTL_SECS)
}

fn cards(
    file: &AccountsFile,
    cache: &HashMap<String, Entry>,
    attempts: &HashMap<String, Attempt>,
) -> Vec<Card> {
    let now = quota_cache::now_secs();
    let failed = |id: &str, entry: &Entry| {
        attempts
            .get(id)
            .is_some_and(|a| a.failed && a.at >= entry.fetched_at)
    };
    let quotas = cache
        .iter()
        .filter(|(id, entry)| entry.is_fresh(now) && !failed(id, entry))
        .map(|(id, entry)| (id.clone(), entry.quota.clone()))
        .collect();
    let recommended = hangar_core::recommendation::recommend(
        &file.accounts,
        file.current_account_id.as_deref(),
        &quotas,
    )
    .map(|r| r.account_id);
    file.accounts
        .iter()
        .map(|account| {
            let entry = cache.get(&account.id).filter(|_| !account.stale);
            let state = match entry {
                Some(entry) if failed(&account.id, entry) => QuotaState::Failed,
                Some(entry) if entry.is_fresh(now) => QuotaState::Fresh,
                Some(_) => QuotaState::Old,
                None if attempts.get(&account.id).is_some_and(|a| a.failed) => QuotaState::Failed,
                None => QuotaState::Unknown,
            };
            Card {
                account: AccountSummary::from_account(
                    account,
                    file.current_account_id.as_deref() == Some(&account.id),
                    now,
                ),
                quota: entry.map(|e| QuotaSummary::from_quota(&e.quota, e.fetched_at as i64)),
                fetched_at: entry.map(|e| e.fetched_at),
                state,
                recommended: recommended.as_deref() == Some(&account.id),
            }
        })
        .collect()
}

pub fn snapshot(
    sender: &Sender<Event>,
    generation: u64,
    terminal: bool,
    attempts: &HashMap<String, Attempt>,
) {
    if let Ok(file) = hangar_core::load_accounts() {
        let _ = sender.send(Event::Cards {
            generation,
            cards: cards(&file, &quota_cache::load(), attempts),
            terminal,
        });
    }
}

fn pending_accounts<'a>(
    file: &'a AccountsFile,
    cache: &HashMap<String, Entry>,
    attempts: &HashMap<String, Attempt>,
    now: u64,
    force: bool,
) -> Vec<&'a hangar_core::Account> {
    file.accounts
        .iter()
        .filter(|account| {
            !account.stale
                && quota_cache::due(cache.get(&account.id), now, force)
                && eligible(attempts.get(&account.id), now, force)
        })
        .collect()
}

pub fn run(
    sender: &Sender<Event>,
    generation: u64,
    force: bool,
    cancel: &AtomicBool,
    attempts: &mut HashMap<String, Attempt>,
) -> Result<String, String> {
    snapshot(sender, generation, false, attempts);
    hangar_core::account::harvest_checked()
        .map_err(|_| "无法收敛官方登录状态，请运行自检".to_string())?;
    let file =
        hangar_core::load_accounts().map_err(|_| "无法读取账号库，请运行自检".to_string())?;
    let cache = quota_cache::load();
    let mut display = cards(&file, &cache, attempts);
    let _ = sender.send(Event::Cards {
        generation,
        cards: display.clone(),
        terminal: false,
    });
    // Freeze this round's eligible work once, so progress counts actual requests,
    // not all cards (stale, fresh cache and retry cooldown are excluded).
    let now = quota_cache::now_secs();
    let pending = pending_accounts(&file, &cache, attempts, now, force);
    let total = pending.len();
    let _ = sender.send(Event::RefreshProgress {
        generation,
        completed: 0,
        total,
    });
    let mut failures = 0;
    for (index, account) in pending.into_iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            break;
        }
        if let Some(card) = display.iter_mut().find(|c| c.account.id == account.id) {
            card.state = QuotaState::Loading;
        }
        let _ = sender.send(Event::Cards {
            generation,
            cards: display.clone(),
            terminal: false,
        });
        // Let Core finish S11 persistence even if cancellation arrives in flight.
        let result = hangar_core::fetch_quota_for_account(&account.id);
        let failed = result.is_err();
        failures += usize::from(failed);
        attempts.insert(
            account.id.clone(),
            Attempt {
                at: quota_cache::now_secs(),
                failed,
            },
        );
        let _ = sender.send(Event::RefreshProgress {
            generation,
            completed: index + 1,
            total,
        });
        let latest = hangar_core::load_accounts()
            .map_err(|_| "刷新后无法读取账号库，请运行自检".to_string())?;
        display = cards(&latest, &quota_cache::load(), attempts);
        if !cancel.load(Ordering::Acquire) {
            let _ = sender.send(Event::Cards {
                generation,
                cards: display.clone(),
                terminal: false,
            });
        }
    }
    snapshot(sender, generation, true, attempts);
    if cancel.load(Ordering::Acquire) {
        Ok("刷新已停止；已完成的结果已保留".into())
    } else if failures > 0 {
        Err(format!("{failures} 个账号刷新失败，已保留可用的旧数据"))
    } else if force {
        Ok(format!("刷新完成：已处理 {total} 个账号"))
    } else {
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooldown_does_not_loop_on_failure_but_force_and_clock_rollback_retry() {
        let attempt = Attempt {
            at: 100,
            failed: true,
        };
        assert!(!eligible(Some(&attempt), 101, false));
        assert!(eligible(Some(&attempt), 101, true));
        assert!(eligible(Some(&attempt), 1900, false));
        assert!(eligible(Some(&attempt), 99, false));
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use hangar_core::{
        quota::{NamedWindow, Quota, QuotaWindow},
        Account,
    };

    #[test]
    fn refresh_plan_counts_only_actual_work_and_manual_refresh_bypasses_cache_and_cooldown() {
        // Scheduling-only fixtures with no credentials; no file or network operations.
        let file = AccountsFile {
            accounts: ["stale", "fresh", "cooldown", "due"]
                .into_iter()
                .map(|id| Account {
                    id: id.into(),
                    email: String::new(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    id_token: String::new(),
                    expires_at: 0,
                    stale: id == "stale",
                    account_id: None,
                    organization_id: None,
                })
                .collect(),
            current_account_id: None,
        };
        let cache = HashMap::from([(
            "fresh".into(),
            Entry {
                fetched_at: 100,
                quota: Quota::default(),
            },
        )]);
        let attempts = HashMap::from([(
            "cooldown".into(),
            Attempt {
                at: 100,
                failed: true,
            },
        )]);
        let ids = |force| {
            pending_accounts(&file, &cache, &attempts, 101, force)
                .into_iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(false), ["due"]);
        assert_eq!(ids(true), ["fresh", "cooldown", "due"]);
    }

    #[test]
    fn failed_cache_retains_visible_value_but_cannot_recommend_and_stale_hides_it() {
        // Data-only projection fixture; empty credentials never leave memory or hit an endpoint.
        let account = Account {
            id: "projection".into(),
            email: "layout@example.invalid".into(),
            access_token: String::new(),
            refresh_token: String::new(),
            id_token: String::new(),
            expires_at: 0,
            stale: false,
            account_id: None,
            organization_id: None,
        };
        let mut file = AccountsFile {
            accounts: vec![account],
            current_account_id: None,
        };
        let now = quota_cache::now_secs();
        let cache = HashMap::from([(
            "projection".into(),
            Entry {
                fetched_at: now,
                quota: Quota {
                    plan: None,
                    windows: vec![NamedWindow {
                        label: "周".into(),
                        window: QuotaWindow {
                            remaining: Some(70),
                            limit_secs: None,
                            reset_in_secs: None,
                            reset_at: None,
                        },
                    }],
                    reset_available: None,
                    reset_detail: None,
                },
            },
        )]);
        let attempts = HashMap::from([(
            "projection".into(),
            Attempt {
                at: now,
                failed: true,
            },
        )]);
        let failed = cards(&file, &cache, &attempts);
        assert_eq!(failed[0].remaining(), Some(70));
        assert_eq!(failed[0].state, QuotaState::Failed);
        assert!(!failed[0].recommended);
        file.accounts[0].stale = true;
        assert!(cards(&file, &cache, &attempts)[0].quota.is_none());
    }
}
