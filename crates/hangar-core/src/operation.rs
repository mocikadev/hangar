//! 跨入口共享的操作错误码与任务状态；不包含平台线程、回调或展示文案。

use crate::account::AccountErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationErrorCode {
    State,
    Auth,
    External,
    Internal,
}

impl From<AccountErrorKind> for OperationErrorCode {
    fn from(value: AccountErrorKind) -> Self {
        match value {
            AccountErrorKind::State => Self::State,
            AccountErrorKind::Auth => Self::Auth,
            AccountErrorKind::External => Self::External,
            AccountErrorKind::Internal => Self::Internal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatus {
    pub request_id: u64,
    pub phase: TaskPhase,
    pub error_code: Option<OperationErrorCode>,
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.phase,
            TaskPhase::Succeeded | TaskPhase::Failed | TaskPhase::Cancelled
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_terminal_state_is_explicit() {
        let running = TaskStatus {
            request_id: 7,
            phase: TaskPhase::Running,
            error_code: None,
        };
        let failed = TaskStatus {
            request_id: 7,
            phase: TaskPhase::Failed,
            error_code: Some(OperationErrorCode::External),
        };
        assert!(!running.is_terminal());
        assert!(failed.is_terminal());
    }
}
