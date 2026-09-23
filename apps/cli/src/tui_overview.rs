//! TUI 账号总览：只把账号、令牌健康和会话内配额快照映射为表格。

use hangar_core::account::Account;
use hangar_core::quota::Quota;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Account,
    Status,
    Plan,
    Week,
    Access,
    Refresh,
}

fn columns(width: u16) -> Vec<Column> {
    if width >= 78 {
        vec![
            Column::Account,
            Column::Status,
            Column::Plan,
            Column::Week,
            Column::Access,
            Column::Refresh,
        ]
    } else if width >= 60 {
        vec![
            Column::Account,
            Column::Status,
            Column::Week,
            Column::Access,
        ]
    } else {
        vec![Column::Account, Column::Status, Column::Week]
    }
}

fn heading(column: Column) -> &'static str {
    match column {
        Column::Account => "账号",
        Column::Status => "状态",
        Column::Plan => "计划",
        Column::Week => "周剩余",
        Column::Access => "AT",
        Column::Refresh => "RT",
    }
}

fn constraint(column: Column) -> Constraint {
    match column {
        Column::Account => Constraint::Min(18),
        Column::Status => Constraint::Length(13),
        Column::Plan => Constraint::Length(12),
        Column::Week => Constraint::Length(10),
        Column::Access | Column::Refresh => Constraint::Length(9),
    }
}

fn weekly_percent(quota: Option<&Quota>) -> String {
    quota
        .and_then(hangar_core::recommendation::weekly_remaining)
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "未知".to_string())
}

fn quota_value(
    id: &str,
    quota: Option<&Quota>,
    pending: &HashSet<String>,
    errors: &HashMap<String, String>,
    quota_times: &HashMap<String, u64>,
    column: Column,
) -> String {
    if pending.contains(id) && quota.is_none() {
        return "查询中".to_string();
    }
    if errors.contains_key(id) && quota.is_none() {
        return "失败".to_string();
    }
    let Some(quota) = quota else {
        return "待查询".to_string();
    };
    match column {
        Column::Week => {
            let value = weekly_percent(Some(quota));
            let fresh = quota_times.get(id).is_some_and(|fetched_at| {
                let now = hangar_core::quota_cache::now_secs();
                now >= *fetched_at && now - *fetched_at < hangar_core::quota_cache::TTL_SECS
            });
            if !fresh || errors.contains_key(id) {
                format!("{value} 旧")
            } else {
                value
            }
        }
        _ => String::new(),
    }
}

pub(super) struct Overview<'a> {
    pub accounts: &'a [Account],
    pub visible: &'a [usize],
    pub current: Option<&'a str>,
    pub recommended: Option<&'a str>,
    pub quotas: &'a HashMap<String, Quota>,
    pub quota_times: &'a HashMap<String, u64>,
    pub pending: &'a HashSet<String>,
    pub errors: &'a HashMap<String, String>,
    pub filter: &'a str,
}

pub(super) fn render(frame: &mut ratatui::Frame, area: Rect, data: Overview<'_>, selected: usize) {
    let columns = columns(area.width);
    let rows = data.visible.iter().map(|index| {
        let account = &data.accounts[*index];
        let quota = data.quotas.get(&account.id);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let health = hangar_core::token_health::assess(
            &account.access_token,
            &account.refresh_token,
            account.expires_at,
            account.stale,
            now,
        );
        let cells = columns.iter().map(|column| {
            let value = match column {
                Column::Account => account.email.clone(),
                Column::Status if account.stale => "⚠ 需登录".to_string(),
                Column::Status
                    if data.current == Some(account.id.as_str())
                        && data.recommended == Some(account.id.as_str()) =>
                {
                    "● 使用中 ★".to_string()
                }
                Column::Status if data.current == Some(account.id.as_str()) => {
                    "● 使用中".to_string()
                }
                Column::Status if data.recommended == Some(account.id.as_str()) => {
                    "★ 建议".to_string()
                }
                Column::Status => "未激活".to_string(),
                Column::Plan => quota
                    .and_then(|quota| quota.plan.clone())
                    .unwrap_or_else(|| "待查询".to_string()),
                Column::Week => quota_value(
                    &account.id,
                    quota,
                    data.pending,
                    data.errors,
                    data.quota_times,
                    *column,
                ),
                Column::Access => health.label().to_string(),
                Column::Refresh => if health.has_refresh_token {
                    "可续期"
                } else {
                    "缺失"
                }
                .to_string(),
            };
            let color = match column {
                Column::Status if account.stale => Color::Yellow,
                Column::Status if data.current == Some(account.id.as_str()) => Color::Green,
                Column::Status if data.recommended == Some(account.id.as_str()) => Color::Cyan,
                Column::Access if !health.can_project_without_refresh() => Color::Yellow,
                _ => Color::Reset,
            };
            Cell::from(value).style(Style::default().fg(color))
        });
        Row::new(cells)
    });
    let title = if data.filter.is_empty() {
        format!(" 账号总览 ({}) ", data.accounts.len())
    } else {
        format!(" 账号总览 [{}] ", data.filter)
    };
    let header = Row::new(columns.iter().map(|column| heading(*column))).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    let widths: Vec<Constraint> = columns.iter().map(|column| constraint(*column)).collect();
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    let mut state = TableState::default();
    if !data.visible.is_empty() {
        state.select(Some(selected));
    }
    frame.render_stateful_widget(table, area, &mut state);

    if data.visible.is_empty() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Line::raw("  （空）按 : 打开命令面板添加"))
                .style(Style::default().fg(Color::DarkGray)),
            area.inner(ratatui::layout::Margin {
                horizontal: 2,
                vertical: 2,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_degrade_for_narrow_terminals() {
        assert_eq!(columns(120).len(), 6);
        assert_eq!(columns(80).len(), 6);
        assert_eq!(columns(70).len(), 4);
        assert_eq!(columns(50).len(), 3);
    }
}
