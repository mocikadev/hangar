//! Credential-free task feedback; timing and cancellation are presentation state only.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Refresh,
    Switch,
    Delete,
    Doctor,
    Add,
    Reauth,
}

pub struct Activity {
    pub operation: Operation,
    pub target_account: Option<String>,
    started: Instant,
    progress: Option<(usize, usize)>,
    stopping: bool,
}

impl Activity {
    pub fn new(operation: Operation) -> Self {
        Self {
            operation,
            target_account: None,
            started: Instant::now(),
            progress: None,
            stopping: false,
        }
    }

    pub fn visible_at(&self, now: Instant) -> bool {
        self.stopping || now.saturating_duration_since(self.started) >= Duration::from_millis(500)
    }

    pub fn set_progress(&mut self, completed: usize, total: usize) -> bool {
        if self.operation != Operation::Refresh || completed > total {
            return false;
        }
        if let Some((previous, expected)) = self.progress {
            if total != expected || completed < previous {
                return false;
            }
        }
        self.progress = Some((completed, total));
        true
    }

    pub fn stop_refresh(&mut self) -> bool {
        if !self.can_stop() {
            return false;
        }
        self.stopping = true;
        true
    }

    pub fn can_stop(&self) -> bool {
        self.operation == Operation::Refresh && !self.stopping
    }

    pub fn message(&self) -> String {
        if self.stopping {
            return "正在停止，等待当前账号处理完成…".into();
        }
        match self.operation {
            Operation::Refresh => match self.progress {
                Some((completed, total)) if total > 0 => {
                    format!("正在刷新周额度 · 已处理 {completed}/{total} 个账号")
                }
                _ => "正在检查账号与额度缓存…".into(),
            },
            Operation::Switch => "正在切换账号…".into(),
            Operation::Delete => "正在删除账号…".into(),
            Operation::Doctor => "正在运行自检…".into(),
            Operation::Add => "正在添加账号…".into(),
            Operation::Reauth => "正在重新登录…".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_checks_stay_hidden_but_stop_is_immediate_and_survives_progress() {
        let mut activity = Activity::new(Operation::Refresh);
        assert!(!activity.visible_at(activity.started + Duration::from_millis(499)));
        assert!(activity.visible_at(activity.started + Duration::from_millis(500)));
        assert!(activity.stop_refresh());
        assert!(activity.visible_at(activity.started));
        assert!(activity.set_progress(1, 3));
        assert_eq!(activity.message(), "正在停止，等待当前账号处理完成…");
        assert!(!activity.can_stop());
        assert!(!activity.stop_refresh());
    }

    #[test]
    fn progress_is_monotonic_and_empty_checks_never_display_zero_over_zero() {
        let mut activity = Activity::new(Operation::Refresh);
        activity.set_progress(0, 0);
        assert!(!activity.message().contains("0/0"));
        let mut activity = Activity::new(Operation::Refresh);
        assert!(activity.set_progress(1, 3));
        assert!(!activity.set_progress(0, 3));
        assert!(!activity.set_progress(2, 4));
        assert!(!activity.set_progress(4, 3));
        assert!(activity.message().contains("1/3"));
    }

    #[test]
    fn non_refresh_tasks_have_distinct_feedback_and_cannot_accept_refresh_events() {
        let mut messages = Vec::new();
        for operation in [
            Operation::Switch,
            Operation::Delete,
            Operation::Doctor,
            Operation::Add,
            Operation::Reauth,
        ] {
            let mut activity = Activity::new(operation);
            assert!(!activity.stop_refresh());
            assert!(!activity.set_progress(1, 2));
            messages.push(activity.message());
        }
        messages.sort();
        messages.dedup();
        assert_eq!(messages.len(), 5);
    }
}
