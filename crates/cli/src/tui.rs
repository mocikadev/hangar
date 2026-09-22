//! 全屏 TUI（ratatui）：账号列表 + 详情/配额 + 日志三栏，j/k + / 过滤。
//!
//! - 键位：j/k 移动，/ 过滤，回车切换，: 命令面板（添加/复活/配额/自检/收敛/删除/更新），q 退出
//! - 网络/长耗时走后台线程 + mpsc 事件，界面不卡；spinner 动画靠 100ms tick
//! - 需要整屏交还终端的流程（浏览器登录/手动粘贴 URL）用挂起-恢复：
//!   退出 alt-screen + 还原 cooked 模式跑原有阻塞流程，结束重进 TUI
//! - 本线程全程 quiet（ui::set_quiet），后台线程各自 quiet，harvest/refresh 的
//!   println 不会打花界面；挂起期间临时还原因登录流程本就打字输出

use hangar_core as core;
use hangar_core::account::Account;
use hangar_core::quota::Quota;
use std::collections::{HashMap, HashSet};
use std::io::Stdout;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const LOG_CAP: usize = 200;

enum Ev {
    SwitchDone {
        email: String,
        res: Result<(), String>,
    },
    QuotaOne {
        id: String,
        email: String,
        res: Box<Result<(Account, Quota), String>>,
    },
    QuotaDone,
    UpdateDone {
        res: Result<Option<String>, String>,
    },
}

struct App {
    accounts: Vec<Account>,
    current: Option<String>,
    selected: usize,
    filter: String,
    filtering: bool,
    log: Vec<String>,
    busy: Option<String>,
    confirm_delete: bool,
    doctor_view: Option<(Vec<String>, usize)>,
    quotas: HashMap<String, Quota>,
    quota_pending: HashSet<String>,
    quota_errors: HashMap<String, String>,
    quota_now: i64,
    quota_busy: bool,
    log_expanded: bool,
    should_quit: bool,
    tick: u64,
    /// 命令面板状态：打开中 / 过滤词（None=选择模式，Some=过滤模式）
    palette_open: bool,
    palette_input: Option<String>,
    palette_sel: usize,
    tx: Sender<Ev>,
    rx: Receiver<Ev>,
}

impl App {
    fn filtered(&self) -> Vec<usize> {
        let q = self.filter.trim().to_lowercase();
        self.accounts
            .iter()
            .enumerate()
            .filter(|(_, a)| q.is_empty() || a.email.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn selected_account(&self) -> Option<&Account> {
        self.filtered()
            .get(self.selected)
            .and_then(|&i| self.accounts.get(i))
    }

    fn clamp_selection(&mut self) {
        let n = self.filtered().len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }
    }

    fn reload(&mut self) {
        if let Ok(f) = core::account::load_accounts() {
            self.current = f.current_account_id;
            self.accounts = f.accounts;
        }
        self.clamp_selection();
    }

    fn push_log(&mut self, s: String) {
        self.log.push(s);
        if self.log.len() > LOG_CAP {
            self.log.drain(..self.log.len() - LOG_CAP);
        }
    }

    fn apply_quota_result(
        &mut self,
        id: String,
        email: String,
        result: Result<(Account, Quota), String>,
    ) {
        self.quota_pending.remove(&id);
        match result {
            Ok((account, quota)) => {
                self.quota_errors.remove(&id);
                if let Some(stored) = self
                    .accounts
                    .iter_mut()
                    .find(|stored| stored.id == account.id)
                {
                    *stored = account;
                }
                self.quotas.insert(id, quota);
            }
            Err(error) => {
                self.quotas.remove(&id);
                self.quota_errors.insert(id, error.clone());
                self.push_log(format!("✗ {}：{}", email, error));
            }
        }
    }

    fn now_secs() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }
}

/// 挂起 TUI 跑阻塞交互流程（登录/复活）：还回 cooked 终端，结束重进并刷新
fn suspend<T>(term: &mut Terminal<CrosstermBackend<Stdout>>, f: impl FnOnce() -> T) -> T {
    crate::ui::set_quiet(false);
    disable_raw_mode().ok();
    execute!(std::io::stdout(), LeaveAlternateScreen).ok();
    let r = f();
    enable_raw_mode().ok();
    execute!(std::io::stdout(), EnterAlternateScreen).ok();
    term.clear().ok();
    crate::ui::set_quiet(true);
    r
}

fn centered(r: Rect, w_pct: u16, h_pct: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - h_pct) / 2),
            Constraint::Percentage(h_pct),
            Constraint::Percentage((100 - h_pct) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - w_pct) / 2),
            Constraint::Percentage(w_pct),
            Constraint::Percentage((100 - w_pct) / 2),
        ])
        .split(v[1])[1]
}

fn level_color(pct: Option<i32>) -> Color {
    match pct {
        None => Color::DarkGray,
        Some(p) if p >= 50 => Color::Green,
        Some(p) if p >= 20 => Color::Yellow,
        _ => Color::Red,
    }
}

/// 重置卡行：数量 + 最早到期；无数据置灰横线（只读展示，不接 consume）
fn reset_credit_line(q: &Quota) -> Line<'static> {
    let (count, expiry) = match &q.reset_detail {
        Some(d) => (Some(d.available), d.next_expiry()),
        None => (q.reset_available, None),
    };
    let count_txt = count
        .map(|c| c.to_string())
        .unwrap_or_else(|| "-".to_string());
    let exp_txt = expiry
        .map(core::quota::fmt_ts_local)
        .unwrap_or_else(|| "-".to_string());
    let style = match count {
        Some(c) if c > 0 => Style::default().fg(Color::Cyan),
        _ => Style::default().fg(Color::DarkGray),
    };
    Line::styled(
        format!("重置卡 {} 张 · 最早到期 {}", count_txt, exp_txt),
        style,
    )
}

fn render(app: &mut App, term: &mut Terminal<CrosstermBackend<Stdout>>) {
    let _ = term.draw(|f| draw_ui(f, app));
}

fn draw_ui(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.area();
    let recommendation = (!app.quota_busy)
        .then(|| {
            crate::recommendation::recommend(&app.accounts, app.current.as_deref(), &app.quotas)
        })
        .flatten();
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(if app.log_expanded { 9 } else { 3 }),
            Constraint::Length(1),
        ])
        .split(area);

    // 标题（单行无边框：左品牌，右状态）
    let title_right = match &app.busy {
        Some(b) => format!("{} {}", SPINNER[(app.tick / 2) as usize % SPINNER.len()], b),
        None if app.quota_busy => format!(
            "{} 配额查询中…",
            SPINNER[(app.tick / 2) as usize % SPINNER.len()]
        ),
        None => "就绪".to_string(),
    };
    let title_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(40)])
        .split(root[0]);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "◆ Hangar · Codex 多账号管理",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])),
        title_row[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            title_right,
            Style::default().fg(Color::Yellow),
        )]))
        .alignment(ratatui::layout::Alignment::Right),
        title_row[1],
    );

    // 主区：所有账号总览 + 选中账号的精确信息。总览承担跨账号比较，
    // 详情只保留不适合塞进列中的重置时间与身份信息。
    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(root[1]);

    let rows = app.filtered();
    crate::tui_overview::render(
        f,
        main[0],
        crate::tui_overview::Overview {
            accounts: &app.accounts,
            visible: &rows,
            current: app.current.as_deref(),
            recommended: recommendation
                .as_ref()
                .map(|recommendation| recommendation.account_id.as_str()),
            quotas: &app.quotas,
            pending: &app.quota_pending,
            errors: &app.quota_errors,
            filter: &app.filter,
        },
        app.selected,
    );

    let right = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(main[1]);
    let detail_lines: Vec<Line> = match app.selected_account() {
        None => vec![Line::styled(
            "  无选中",
            Style::default().fg(Color::DarkGray),
        )],
        Some(a) => {
            let now = App::now_secs();
            let health = core::token_health::assess(
                &a.access_token,
                &a.refresh_token,
                a.expires_at,
                a.stale,
                now as u64,
            );
            let exp = match health.access {
                core::token_health::AccessTokenState::Missing => "访问令牌缺失".to_string(),
                core::token_health::AccessTokenState::Expired => {
                    "访问令牌已过期（切换前必须刷新）".to_string()
                }
                core::token_health::AccessTokenState::Expiring => {
                    "访问令牌即将过期（切换前必须刷新）".to_string()
                }
                core::token_health::AccessTokenState::Fresh => format!(
                    "访问令牌至 {}",
                    core::quota::fmt_ts_local(health.expires_at.unwrap_or_default())
                ),
                core::token_health::AccessTokenState::Unknown => "访问令牌有效期未知".to_string(),
            };
            let plan = app
                .quotas
                .get(&a.id)
                .and_then(|q| q.plan.clone())
                .unwrap_or_else(|| "-".to_string());
            vec![
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        a.email.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::raw(format!("  {}", exp)),
                Line::raw(format!(
                    "  刷新令牌: {}",
                    if a.refresh_token.trim().is_empty() {
                        "缺失（无法自动续期）"
                    } else {
                        "有（可自动续期）"
                    }
                )),
                Line::raw(format!("  计划: {}", plan)),
                Line::raw(format!(
                    "  身份: {} / {}",
                    a.account_id.as_deref().unwrap_or("-"),
                    a.organization_id.as_deref().unwrap_or("-")
                )),
                Line::from(vec![
                    Span::raw("  状态: "),
                    if a.stale {
                        Span::styled(
                            "⚠ 需重新登录（: → 复活账号）",
                            Style::default().fg(Color::Yellow),
                        )
                    } else {
                        Span::styled("正常", Style::default().fg(Color::Green))
                    },
                    if Some(a.id.as_str()) == app.current.as_deref() {
                        Span::styled(
                            "  ● 使用中",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("")
                    },
                ]),
            ]
        }
    };
    f.render_widget(
        Paragraph::new(detail_lines).block(Block::default().borders(Borders::ALL).title(" 详情 ")),
        right[0],
    );

    // 配额窗
    let quota_block = Block::default().borders(Borders::ALL).title(" 配额 ");
    match app.selected_account() {
        None => f.render_widget(Paragraph::new("  -").block(quota_block), right[1]),
        Some(a) => match app.quotas.get(&a.id) {
            None => f.render_widget(
                Paragraph::new("  按 : 打开命令面板查询配额").block(quota_block),
                right[1],
            ),
            Some(q) => {
                let inner = quota_block.inner(right[1]);
                f.render_widget(quota_block, right[1]);
                // 当前产品只展示实际生效的周窗口；core 仍容错保留其他窗口，
                // 待服务端重新启用后再单独评估是否恢复到前端。
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Length(1)])
                    .split(inner);
                if let Some(w) = q.windows.iter().find(|w| w.label.starts_with('周')) {
                    let pct = w.window.remaining.unwrap_or(0).clamp(0, 100);
                    let reset = core::quota::reset_at_ts(&w.window, app.quota_now)
                        .map(core::quota::fmt_ts_local)
                        .unwrap_or_else(|| "未知".to_string());
                    let text = match w.window.remaining {
                        Some(p) => format!("周剩余 {}% · 重置于 {}", p, reset),
                        None => "周剩余 未知".to_string(),
                    };
                    let g = ratatui::widgets::LineGauge::default()
                        .ratio(if w.window.remaining.is_some() {
                            pct as f64 / 100.0
                        } else {
                            0.0
                        })
                        .label(Span::styled(
                            text,
                            Style::default().fg(level_color(w.window.remaining)),
                        ))
                        .filled_style(Style::default().fg(level_color(w.window.remaining)));
                    f.render_widget(g, rows[0]);
                } else {
                    f.render_widget(
                        Paragraph::new(Line::styled(
                            "周剩余 未知",
                            Style::default().fg(Color::DarkGray),
                        )),
                        rows[0],
                    );
                }
                // 重置卡行：summary 数量 + 明细最早到期（只读）
                let rc_line = reset_credit_line(q);
                f.render_widget(Paragraph::new(rc_line), rows[1]);
            }
        },
    }

    // 日志
    let h = root[2].height as usize;
    let start = app.log.len().saturating_sub(h.saturating_sub(2));
    let log_lines: Vec<Line> = app.log[start..]
        .iter()
        .map(|s| {
            let style = if s.contains("✗") || s.contains("失败") {
                Style::default().fg(Color::Red)
            } else if s.contains("⚠") {
                Style::default().fg(Color::Yellow)
            } else if s.contains("✅") || s.contains("成功") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::styled(format!("  {}", s), style)
        })
        .collect();
    f.render_widget(
        Paragraph::new(log_lines)
            .block(Block::default().borders(Borders::ALL).title(" 日志 "))
            .wrap(Wrap { trim: false }),
        root[2],
    );

    // 帮助
    let help = if app.filtering {
        "  过滤输入中… 回车/ESC 确认"
    } else if app.palette_open {
        "  命令面板：输入过滤 回车执行 ESC 关闭"
    } else {
        "  j/k 移动  / 过滤  回车切换  : 命令  l 展开日志  q 退出"
    };
    f.render_widget(
        Paragraph::new(Line::styled(help, Style::default().fg(Color::DarkGray))),
        root[3],
    );

    // 删除确认
    if app.confirm_delete {
        if let Some(a) = app.selected_account() {
            let email = a.email.clone();
            let area = centered(area, 60, 20);
            let p = Paragraph::new(vec![
                Line::raw(""),
                Line::raw(format!("  确认删除 {} ?", email)),
                Line::raw(""),
                Line::styled(
                    "  y 确认 / n 或 ESC 取消",
                    Style::default().fg(Color::Yellow),
                ),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 删除 ")
                    .border_style(Style::default().fg(Color::Red)),
            );
            f.render_widget(ratatui::widgets::Clear, area);
            f.render_widget(p, area);
        }
    }

    // 自检覆盖层
    if let Some((lines, scroll)) = &app.doctor_view {
        let area = centered(area, 80, 80);
        let total = lines.len();
        let view_h = area.height.saturating_sub(2) as usize;
        let max_scroll = total.saturating_sub(view_h);
        let s = (*scroll).min(max_scroll);
        let body: Vec<Line> = lines
            .iter()
            .skip(s)
            .take(view_h)
            .map(|l| {
                let style = if l.starts_with("✗") {
                    Style::default().fg(Color::Red)
                } else if l.starts_with("⚠") {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Green)
                };
                Line::styled(format!("  {}", l), style)
            })
            .collect();
        let p = Paragraph::new(body).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    " 自检 ({}/{}) j/k滚动 ESC关闭 ",
                    s + 1.min(total),
                    total
                ))
                .border_style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(ratatui::widgets::Clear, area);
        f.render_widget(p, area);
    }

    // 命令面板覆盖层（最后渲染，置顶）
    if app.palette_open {
        render_palette(f, app);
    }
}

fn spawn_switch(tx: Sender<Ev>, id: String) {
    std::thread::spawn(move || {
        crate::ui::set_quiet(true);
        let email = core::account::load_accounts()
            .ok()
            .and_then(|f| {
                f.accounts
                    .iter()
                    .find(|a| a.id == id)
                    .map(|a| a.email.clone())
            })
            .unwrap_or_default();
        let res = core::account::switch_account(&id);
        let _ = tx.send(Ev::SwitchDone { email, res });
    });
}

fn spawn_quota(tx: Sender<Ev>, ids: Vec<(String, String)>) {
    std::thread::spawn(move || {
        crate::ui::set_quiet(true);
        for (id, email) in ids {
            let res = core::quota::fetch_quota_for_account(&id);
            if tx
                .send(Ev::QuotaOne {
                    id,
                    email,
                    res: Box::new(res),
                })
                .is_err()
            {
                return;
            }
        }
        let _ = tx.send(Ev::QuotaDone);
    });
}

fn spawn_update(tx: Sender<Ev>) {
    std::thread::spawn(move || {
        crate::ui::set_quiet(true);
        // 网络不持账号锁；失败只进日志，旧版继续可用
        let res = match core::updater::check_update(
            true,
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_NAME"),
        ) {
            Ok(Some(info)) => core::updater::apply_update(&info).map(Some),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        };
        let _ = tx.send(Ev::UpdateDone { res });
    });
}

fn quota_targets(app: &App) -> Vec<(String, String)> {
    app.accounts
        .iter()
        .filter(|account| !account.stale)
        .map(|a| (a.id.clone(), a.email.clone()))
        .collect()
}

pub fn run() -> Result<(), String> {
    crate::ui::set_quiet(true);
    enable_raw_mode().map_err(|e| format!("进入 raw 模式失败: {}", e))?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| format!("进入 alt-screen 失败: {}", e))?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend).map_err(|e| format!("创建终端失败: {}", e))?;
    let res = event_loop(&mut term);
    disable_raw_mode().ok();
    execute!(term.backend_mut(), LeaveAlternateScreen).ok();
    res
}

fn event_loop(term: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<Ev>();
    let mut app = App {
        accounts: vec![],
        current: None,
        selected: 0,
        filter: String::new(),
        filtering: false,
        log: vec!["就绪：: 打开命令面板（登录/复活/删除/配额/自检），60s 自动收敛".to_string()],
        busy: None,
        confirm_delete: false,
        doctor_view: None,
        quotas: HashMap::new(),
        quota_pending: HashSet::new(),
        quota_errors: HashMap::new(),
        quota_now: App::now_secs(),
        quota_busy: false,
        log_expanded: false,
        should_quit: false,
        tick: 0,
        palette_open: false,
        palette_input: None,
        palette_sel: 0,
        tx,
        rx,
    };
    core::account::harvest();
    app.reload();
    // 账号规模以个位数为主：启动后单线程顺序查询全部正常账号，结果逐个回填；
    // 不引入并发刷新与额外锁竞争，网络任务也不阻塞浏览和退出。
    let startup = quota_targets(&app);
    if !startup.is_empty() {
        app.quota_busy = true;
        for (id, _) in &startup {
            app.quota_pending.insert(id.clone());
            app.quota_errors.remove(id);
        }
        app.push_log(format!("开始查询 {} 个账号的周剩余…", startup.len()));
        spawn_quota(app.tx.clone(), startup);
    }

    while !app.should_quit {
        // 后台事件
        while let Ok(ev) = app.rx.try_recv() {
            match ev {
                Ev::SwitchDone { email, res } => {
                    app.busy = None;
                    match res {
                        Ok(()) => {
                            app.push_log(format!("✅ 已切换到: {}", email));
                            if core::codex_process_running() {
                                app.push_log(
                                    "ℹ 检测到 Codex 正在运行，请重启 Codex 生效".to_string(),
                                );
                            }
                        }
                        Err(e) => app.push_log(format!("✗ {}", e)),
                    }
                    app.reload();
                }
                Ev::QuotaOne { id, email, res } => {
                    app.apply_quota_result(id, email, *res);
                }
                Ev::QuotaDone => {
                    app.quota_busy = false;
                    app.quota_now = App::now_secs();
                    match crate::recommendation::recommend(
                        &app.accounts,
                        app.current.as_deref(),
                        &app.quotas,
                    ) {
                        Some(recommendation)
                            if recommendation.reason
                                == crate::recommendation::Reason::KeepCurrent =>
                        {
                            app.push_log(format!(
                                "✅ 配额查询完成；建议继续使用 {}（周剩余 {}%）",
                                recommendation.email, recommendation.weekly_remaining
                            ));
                        }
                        Some(recommendation) => app.push_log(format!(
                            "✅ 配额查询完成；建议使用 {}（周剩余 {}%）",
                            recommendation.email, recommendation.weekly_remaining
                        )),
                        None => app.push_log("✅ 配额查询完成；暂无可用建议".to_string()),
                    }
                }
                Ev::UpdateDone { res } => {
                    app.busy = None;
                    match res {
                        Ok(Some(v)) => {
                            app.push_log(format!("✅ 已升级到 {}，请重启 hangar 生效", v))
                        }
                        Ok(None) => app.push_log("已是最新版本".to_string()),
                        Err(e) => app.push_log(format!("✗ 更新失败：{}（旧版继续可用）", e)),
                    }
                }
            }
        }

        app.tick += 1;
        if app.tick.is_multiple_of(600) {
            // 60s 被动收敛一次（静默）
            core::account::harvest();
            app.reload();
        }

        render(&mut app, term);

        if !event::poll(Duration::from_millis(100)).map_err(|e| format!("事件轮询失败: {}", e))?
        {
            continue;
        }
        let Event::Key(k) = event::read().map_err(|e| format!("读取按键失败: {}", e))? else {
            continue;
        };
        if k.kind != event::KeyEventKind::Press {
            continue;
        }

        // 自检覆盖层独占按键
        if let Some((_, scroll)) = app.doctor_view.as_mut() {
            match k.code {
                KeyCode::Esc | KeyCode::Enter => app.doctor_view = None,
                KeyCode::Down | KeyCode::Char('j') => *scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                _ => {}
            }
            continue;
        }

        // 命令面板独占按键（两段式：默认上下选，/ 进过滤模式）
        if app.palette_open {
            let vis = palette_visible(&app);
            // 过滤模式：输入即过滤，回车执行，ESC 回选择模式
            if app.palette_input.is_some() {
                match k.code {
                    KeyCode::Esc => {
                        app.palette_input = None;
                        app.palette_sel = 0;
                    }
                    KeyCode::Enter => {
                        if let Some(&i) = vis.get(app.palette_sel) {
                            let action = PALETTE[i].action;
                            app.palette_open = false;
                            app.palette_input = None;
                            app.palette_sel = 0;
                            exec_action(&mut app, term, action);
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(s) = app.palette_input.as_mut() {
                            s.pop();
                        }
                        app.palette_sel = 0;
                    }
                    KeyCode::Char(c) => {
                        if let Some(s) = app.palette_input.as_mut() {
                            s.push(c);
                        }
                        app.palette_sel = 0;
                    }
                    _ => {}
                }
                continue;
            }
            // 选择模式（默认）：上下选，回车执行，/ 进过滤，ESC 关闭
            match k.code {
                KeyCode::Esc => app.palette_open = false,
                KeyCode::Char('/') => {
                    app.palette_input = Some(String::new());
                    app.palette_sel = 0;
                }
                KeyCode::Down => {
                    if !vis.is_empty() {
                        app.palette_sel = (app.palette_sel + 1) % vis.len();
                    }
                }
                KeyCode::Up => {
                    if !vis.is_empty() {
                        app.palette_sel = (app.palette_sel + vis.len() - 1) % vis.len();
                    }
                }
                KeyCode::Enter => {
                    if let Some(&i) = vis.get(app.palette_sel) {
                        let action = PALETTE[i].action;
                        app.palette_open = false;
                        app.palette_input = None;
                        app.palette_sel = 0;
                        exec_action(&mut app, term, action);
                    }
                }
                _ => {}
            }
            continue;
        }

        // 删除确认独占
        if app.confirm_delete {
            match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(a) = app.selected_account() {
                        let (email, id) = (a.email.clone(), a.id.clone());
                        match core::account::delete_account(&id) {
                            Ok(_) => app.push_log(format!("✅ 已删除: {}", email)),
                            Err(e) => app.push_log(format!("✗ {}", e)),
                        }
                        app.reload();
                    }
                    app.confirm_delete = false;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    app.confirm_delete = false;
                }
                _ => {}
            }
            continue;
        }

        // 过滤输入
        if app.filtering {
            match k.code {
                KeyCode::Esc | KeyCode::Enter => app.filtering = false,
                KeyCode::Backspace => {
                    app.filter.pop();
                    app.selected = 0;
                }
                KeyCode::Char(c) => {
                    app.filter.push(c);
                    app.selected = 0;
                }
                _ => {}
            }
            continue;
        }

        // 忙时只响应退出
        if app.busy.is_some() {
            if matches!(k.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
                app.should_quit = true;
            }
            continue;
        }

        match k.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => app.should_quit = true,
            KeyCode::Down | KeyCode::Char('j') => {
                let n = app.filtered().len();
                if n > 0 {
                    app.selected = (app.selected + 1) % n;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let n = app.filtered().len();
                if n > 0 {
                    app.selected = (app.selected + n - 1) % n;
                }
            }
            KeyCode::Char('/') => {
                app.filtering = true;
                app.filter.clear();
                app.selected = 0;
            }
            KeyCode::Char(':') => {
                app.palette_open = true;
                app.palette_input = None;
                app.palette_sel = 0;
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                app.log_expanded = !app.log_expanded;
            }
            KeyCode::Enter => {
                let sel = app.selected_account().map(|a| {
                    (
                        a.id.clone(),
                        a.email.clone(),
                        a.stale,
                        Some(a.id.as_str()) == app.current.as_deref(),
                    )
                });
                if let Some((id, email, stale, is_current)) = sel {
                    if stale {
                        app.push_log(format!("⚠ {} 已失效，用命令面板（:）选「复活账号」", email));
                    } else if is_current {
                        // 已是使用中账号：完整切换会重写 auth.json/keychain，
                        // 还可能在 Codex 刚轮换 token 的瞬间覆盖它，纯多余动作
                        app.push_log(format!("ℹ {} 已是使用中的账号，无需切换", email));
                        let _ = id;
                    } else if app.quota_busy {
                        app.push_log(
                            "ℹ 配额查询期间可继续浏览；请等待查询结束后再切换账号".to_string(),
                        );
                    } else {
                        app.busy = Some(format!("切换到 {}…", email));
                        spawn_switch(app.tx.clone(), id);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 命令面板（'/' 风格菜单，参照 CLI agent）：所有动作收进来，
// 主界面只留安全键，杜绝误触 d/D/a/r/u
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    Add,
    Reauth,
    QuotaCurrent,
    QuotaAll,
    Doctor,
    Harvest,
    Delete,
    Update,
}

struct PaletteItem {
    label: &'static str,
    hint: &'static str,
    action: Action,
}

const PALETTE: &[PaletteItem] = &[
    PaletteItem {
        label: "添加账号",
        hint: "打开浏览器 OAuth 登录，仅添加不切换",
        action: Action::Add,
    },
    PaletteItem {
        label: "复活账号",
        hint: "为失效账号重新登录（原位覆盖）",
        action: Action::Reauth,
    },
    PaletteItem {
        label: "删除账号",
        hint: "删除选中的账号（使用中账号不可删）",
        action: Action::Delete,
    },
    PaletteItem {
        label: "刷新当前配额",
        hint: "只查询选中账号的用量与重置时间",
        action: Action::QuotaCurrent,
    },
    PaletteItem {
        label: "刷新全部配额",
        hint: "逐个查询全部正常账号，不锁住浏览",
        action: Action::QuotaAll,
    },
    PaletteItem {
        label: "自检",
        hint: "离线检查凭据/权限/一致性",
        action: Action::Doctor,
    },
    PaletteItem {
        label: "收敛凭据",
        hint: "立即从官方 auth.json 回收最新 token",
        action: Action::Harvest,
    },
    PaletteItem {
        label: "检查更新",
        hint: "检查并升级到最新版",
        action: Action::Update,
    },
];

fn palette_visible(app: &App) -> Vec<usize> {
    let q = app
        .palette_input
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    PALETTE
        .iter()
        .enumerate()
        .filter(|(_, it)| {
            q.is_empty()
                || it.label.to_lowercase().contains(&q)
                || it.hint.to_lowercase().contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}

fn render_palette(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.area();
    let vis = palette_visible(app);
    let sel = app.palette_sel.min(vis.len().saturating_sub(1));
    let h = (vis.len() as u16 + 2).clamp(5, 12);
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Length(h),
            Constraint::Percentage(100 - 35 - ((h * 100) / area.height.max(1)).min(55)),
        ])
        .split(area);
    let rect = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(v[1])[1];

    let lines: Vec<Line> = vis
        .iter()
        .enumerate()
        .map(|(row, &i)| {
            let it = &PALETTE[i];
            let style = if row == sel {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::styled(format!(" {}  {}", it.label, it.hint), style)
        })
        .collect();
    let title = match &app.palette_input {
        Some(q) => format!(" 命令 · 过滤: {}（回车执行 ESC返回） ", q),
        None => " 命令（j/k选择 回车执行 /过滤 ESC关闭） ".to_string(),
    };
    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(p, rect);
}

fn exec_action(app: &mut App, term: &mut Terminal<CrosstermBackend<Stdout>>, action: Action) {
    if app.quota_busy
        && !matches!(
            action,
            Action::QuotaCurrent | Action::QuotaAll | Action::Doctor
        )
    {
        app.push_log("ℹ 配额查询期间可继续浏览；写操作请等待查询结束".to_string());
        return;
    }
    match action {
        Action::Add => {
            let r = suspend(term, core::do_login);
            match r {
                Ok(email) => {
                    app.push_log(format!("✅ 已添加账号: {}（未激活，选中回车切换）", email));
                    if core::codex_process_running() {
                        app.push_log("ℹ 检测到 Codex 正在运行，请重启 Codex 生效".to_string());
                    }
                }
                Err(e) => app.push_log(format!("✗ {}", e)),
            }
            core::account::harvest();
            app.reload();
        }
        Action::Reauth => {
            let target: Option<(String, String)> = (|| {
                if let Some(a) = app.selected_account() {
                    if a.stale {
                        return Some((a.id.clone(), a.email.clone()));
                    }
                }
                let stales: Vec<(String, String)> = app
                    .accounts
                    .iter()
                    .filter(|a| a.stale)
                    .map(|a| (a.id.clone(), a.email.clone()))
                    .collect();
                if stales.len() == 1 {
                    return stales.into_iter().next();
                }
                None
            })();
            match target {
                None => app.push_log(
                    "ℹ 没有可复活账号（选中带 ⚠ 的账号，或确保只有一个失效账号）".to_string(),
                ),
                Some((id, email)) => {
                    let r = suspend(term, || {
                        let fresh = crate::core::oauth::login_codex()?;
                        core::account::reauth_account(&id, &fresh)?;
                        Ok::<String, String>(fresh.email.clone())
                    });
                    match r {
                        Ok(_) => {
                            app.push_log(format!("✅ 已复活并切换到: {}", email));
                            if core::codex_process_running() {
                                app.push_log(
                                    "ℹ 检测到 Codex 正在运行，请重启 Codex 生效".to_string(),
                                );
                            }
                        }
                        Err(e) => app.push_log(format!("✗ {}", e)),
                    }
                    core::account::harvest();
                    app.reload();
                }
            }
        }
        Action::Delete => {
            let target = app.selected_account().map(|a| {
                (
                    a.email.clone(),
                    Some(a.id.as_str()) == app.current.as_deref(),
                )
            });
            match target {
                Some((email, true)) => app.push_log(format!(
                    "⚠ {} 正在使用中，无法删除；请先切换到其他账号",
                    email
                )),
                Some((_email, false)) => app.confirm_delete = true,
                None => app.push_log("ℹ 没有选中账号".to_string()),
            }
        }
        Action::QuotaCurrent => {
            if app.quota_busy {
                app.push_log("ℹ 配额查询正在进行中".to_string());
                return;
            }
            let target = app
                .selected_account()
                .filter(|a| !a.stale)
                .map(|a| (a.id.clone(), a.email.clone()));
            if let Some(target) = target {
                app.quota_busy = true;
                app.quota_pending.insert(target.0.clone());
                app.quota_errors.remove(&target.0);
                app.push_log(format!("开始查询 {} 的配额…", target.1));
                spawn_quota(app.tx.clone(), vec![target]);
            } else {
                app.push_log("ℹ 选中账号已失效或不存在，无法查询配额".to_string());
            }
        }
        Action::QuotaAll => {
            if app.quota_busy {
                app.push_log("ℹ 配额查询正在进行中".to_string());
                return;
            }
            let ids = quota_targets(app);
            if ids.is_empty() {
                app.push_log("ℹ 没有可查的账号".to_string());
            } else {
                app.quota_busy = true;
                for (id, _) in &ids {
                    app.quota_pending.insert(id.clone());
                    app.quota_errors.remove(id);
                }
                app.push_log(format!("开始查询 {} 个账号配额…", ids.len()));
                spawn_quota(app.tx.clone(), ids);
            }
        }
        Action::Doctor => match core::doctor::doctor_lines(env!("CARGO_PKG_VERSION")) {
            Ok((lines, _)) => app.doctor_view = Some((lines, 0)),
            Err(e) => app.push_log(format!("✗ {}", e)),
        },
        Action::Harvest => match core::account::harvest_checked() {
            Ok(report) => {
                app.reload();
                app.push_log(format!(
                    "✅ 已从官方 auth.json 收敛（{} 个账号{}）",
                    report.account_count,
                    if report.changed { "，有更新" } else { "" }
                ));
            }
            Err(error) => app.push_log(format!("✗ 收敛失败：{error}")),
        },
        Action::Update => {
            app.busy = Some("检查更新中…".to_string());
            spawn_update(app.tx.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_core::quota::{NamedWindow, QuotaWindow};
    use ratatui::backend::TestBackend;

    fn fake_account(id: &str, email: &str, stale: bool) -> Account {
        Account {
            id: id.to_string(),
            email: email.to_string(),
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            id_token: String::new(),
            expires_at: 0,
            stale,
            account_id: None,
            organization_id: None,
        }
    }

    fn fake_app() -> App {
        let (tx, rx) = mpsc::channel();
        App {
            accounts: vec![
                fake_account("id-1", "alice@example.com", false),
                fake_account("id-2", "bob@example.com", true),
            ],
            current: Some("id-1".to_string()),
            selected: 0,
            filter: String::new(),
            filtering: false,
            log: vec!["hello log".to_string()],
            busy: None,
            confirm_delete: false,
            doctor_view: None,
            quotas: HashMap::new(),
            quota_pending: HashSet::new(),
            quota_errors: HashMap::new(),
            quota_now: 0,
            quota_busy: false,
            log_expanded: false,
            should_quit: false,
            tick: 0,
            palette_open: false,
            palette_input: None,
            palette_sel: 0,
            tx,
            rx,
        }
    }

    fn weekly_quota(remaining: i32) -> Quota {
        Quota {
            plan: Some("Pro".to_string()),
            windows: vec![NamedWindow {
                label: "周".to_string(),
                window: QuotaWindow {
                    remaining: Some(remaining),
                    limit_secs: Some(604_800),
                    reset_in_secs: None,
                    reset_at: None,
                },
            }],
            reset_available: None,
            reset_detail: None,
        }
    }

    fn screen_text(app: &mut App) -> String {
        screen_text_size(app, 120, 40)
    }

    fn screen_text_size(app: &mut App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_ui(f, app)).unwrap();
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    // 注意：TestBackend 中 CJK 宽字符占两格、续格为空格（如"详情"存为"详 情"），
    // 因此中文断言只用单字，ASCII 字符串可直接包含匹配
    #[test]
    fn renders_list_detail_log_panes() {
        let mut app = fake_app();
        let t = screen_text(&mut app);
        assert!(t.contains("alice@example.com"), "missing alice");
        assert!(t.contains("bob@example.com"), "missing bob");
        assert!(t.contains('详'), "missing detail pane");
        assert!(t.contains('配'), "missing quota pane");
        assert!(t.contains("hello log"), "missing log");
        assert!(t.contains("j/k"), "missing help");
        assert!(t.contains("AT"), "missing token health column");
        assert!(t.contains("RT"), "missing refresh availability column");
        assert!(t.contains("unknown"), "missing local token health");
        assert!(!t.contains("0%"), "unloaded quota must not look exhausted");
        assert!(!t.contains("5h"), "inactive 5h quota must not be shown");
    }

    #[test]
    fn stale_badge_and_filter() {
        let mut app = fake_app();
        assert!(screen_text(&mut app).contains("⚠"));
        app.filter = "bob".to_string();
        app.clamp_selection();
        assert_eq!(app.filtered(), vec![1]);
        let t = screen_text(&mut app);
        assert!(t.contains("bob@example.com"));
    }

    #[test]
    fn overview_distinguishes_pending_failure_and_unloaded_quota() {
        let mut app = fake_app();
        app.quota_pending.insert("id-1".to_string());
        app.quota_errors
            .insert("id-2".to_string(), "network".to_string());
        let text = screen_text(&mut app);
        assert!(text.contains('查'), "pending quota should be visible");
        assert!(text.contains('失'), "failed quota should be visible");
        assert!(
            !text.contains("0%"),
            "unknown quota must never render as 0%"
        );
    }

    #[test]
    fn overview_handles_narrow_and_ten_account_screens() {
        let mut app = fake_app();
        app.accounts = (0..10)
            .map(|index| {
                fake_account(
                    &format!("id-{index}"),
                    &format!("user{index}@example.com"),
                    false,
                )
            })
            .collect();
        let narrow = screen_text_size(&mut app, 70, 30);
        assert!(narrow.contains("user0@example.com"));
        let normal = screen_text_size(&mut app, 120, 40);
        assert!(normal.contains("user9@example.com"));
    }

    #[test]
    fn doctor_overlay_renders() {
        let mut app = fake_app();
        app.doctor_view = Some((vec!["✅ ok".to_string(), "⚠ w".to_string()], 0));
        let t = screen_text(&mut app);
        assert!(t.contains('检'));
    }

    #[test]
    fn delete_modal_renders() {
        let mut app = fake_app();
        app.confirm_delete = true;
        assert!(screen_text(&mut app).contains('删'));
    }

    #[test]
    fn palette_lists_commands_and_filters() {
        let mut app = fake_app();
        app.palette_open = true;
        let t = screen_text(&mut app);
        // CJK 宽字符在 TestBackend 中被空格隔开，中文断言只用单字；ASCII 可整串
        assert!(t.contains('添'), "palette missing add");
        assert!(t.contains('删'), "palette missing delete");
        assert!(t.contains('命'), "palette missing title");
        // 过滤（对逻辑的断言不经渲染，无 CJK 间距问题）；Some=过滤模式
        app.palette_input = Some("删除".to_string());
        let vis = palette_visible(&app);
        assert_eq!(vis.len(), 1);
        assert_eq!(PALETTE[vis[0]].action, Action::Delete);
        // None=选择模式：不过滤，全部可见
        app.palette_input = None;
        assert_eq!(palette_visible(&app).len(), PALETTE.len());
    }

    #[test]
    fn palette_lists_update_command() {
        let mut app = fake_app();
        app.palette_open = true;
        let t = screen_text(&mut app);
        assert!(t.contains('升'), "palette missing update");
        app.palette_input = Some("升级".to_string());
        let vis = palette_visible(&app);
        assert_eq!(vis.len(), 1);
        assert_eq!(PALETTE[vis[0]].action, Action::Update);
    }

    #[test]
    fn startup_quota_targets_every_healthy_account_in_library_order() {
        let mut app = fake_app();
        assert_eq!(
            quota_targets(&app),
            vec![("id-1".into(), "alice@example.com".into())]
        );
        app.accounts[1].stale = false;
        assert_eq!(
            quota_targets(&app),
            vec![
                ("id-1".into(), "alice@example.com".into()),
                ("id-2".into(), "bob@example.com".into())
            ]
        );
        app.accounts[0].stale = true;
        app.accounts[1].stale = true;
        assert!(quota_targets(&app).is_empty());
    }

    #[test]
    fn overview_marks_weekly_recommendation_after_queries_finish() {
        let mut app = fake_app();
        app.accounts[1].stale = false;
        app.quotas.insert("id-1".into(), weekly_quota(40));
        app.quotas.insert("id-2".into(), weekly_quota(70));
        let text = screen_text(&mut app);
        assert!(text.contains('★'), "recommended account badge missing");
        assert!(text.contains("70%"), "weekly remaining missing");
        assert!(!text.contains("5h"), "inactive 5h quota must not be shown");
    }

    #[test]
    fn quota_result_updates_refreshed_account_snapshot() {
        let mut app = fake_app();
        app.quota_pending.insert("id-1".into());
        let mut refreshed = app.accounts[0].clone();
        refreshed.access_token = "refreshed-at".into();
        refreshed.refresh_token = "rotated-rt".into();

        app.apply_quota_result(
            "id-1".into(),
            "alice@example.com".into(),
            Ok((refreshed, weekly_quota(66))),
        );

        assert_eq!(app.accounts[0].access_token, "refreshed-at");
        assert_eq!(app.accounts[0].refresh_token, "rotated-rt");
        assert_eq!(
            app.quotas
                .get("id-1")
                .and_then(crate::recommendation::weekly_remaining),
            Some(66)
        );
        assert!(!app.quota_pending.contains("id-1"));
    }

    #[test]
    fn palette_has_separate_current_and_all_quota_actions() {
        let mut app = fake_app();
        app.palette_input = Some("当前配额".to_string());
        let current = palette_visible(&app);
        assert_eq!(current.len(), 1);
        assert_eq!(PALETTE[current[0]].action, Action::QuotaCurrent);

        app.palette_input = Some("全部配额".to_string());
        let all = palette_visible(&app);
        assert_eq!(all.len(), 1);
        assert_eq!(PALETTE[all[0]].action, Action::QuotaAll);
    }
}
