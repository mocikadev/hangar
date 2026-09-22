# 测试策略

## 现状

- 当前 workspace 为 64 项自动化测试（hangar-core 28 + CLI 单元 14 + CLI 二进制集成 9 + GUI 13），`cargo test` 单命令运行
- GitHub Actions 在 Linux/macOS/Windows 跑 clippy/test/GUI build，Linux 额外执行 fmt 门禁
- CLI 已有隔离沙箱集成测试；端到端依赖 TUI 真实 PTY/tmux 冒烟与 GUI 三平台真机验收

## 分层测试策略

| 层 | 方式 | 位置 |
|----|------|------|
| 纯函数（JWT/email/时间/配额解析/路径） | 表驱动单测，基准值注明生成方式 | core/cli 各模块 `mod tests` |
| 文件 I/O（原子写/备份/权限/锁） | 临时目录真实读写断言（如 `atomic_write_keeps_backup_private`） | core account.rs |
| TUI 渲染 | `ratatui::backend::TestBackend` 快照断言 | cli tui.rs |
| 前端分发/守卫逻辑 | 构造 `App` 假数据直接断言（如 `palette_visible` 过滤） | cli tui.rs |
| 一次性 CLI 协议 | 参数解析、选择器、退出码、human/JSON 脱敏断言 | cli args/selector/output/commands |
| CLI 文件集成 | 隔离 HOME/CODEX_HOME 后调用二进制，断言 stdout/stderr/退出码及文件副作用 | cli integration tests |
| GUI reducer/托盘守卫 | 构造 `App`/事件直接断言，不启动窗口 | gui app.rs / tray.rs |
| 端到端 | 真实 PTY 或 tmux 冒烟：验证启动、按键、命令面板与退出 | 人工/会话内 |
| 真实凭据链路 | 用户真机操作回报输出（登录/刷新/配额成功路径不可离线模拟） | 用户协同 |

## 已知约束（写测试时必读）

1. **CJK 断言**：TestBackend buffer 中宽字符占两格、续格为空格（"详情"存为"详 情"），中文 `contains` 断言只用**单字**；ASCII 可整串
2. **时区无关**：时间格式化断言只验格式（长度/分隔符），不硬编码日期；需要具体值的用固定偏移环境
3. **隔离沙箱**：任何触盘测试用临时 `HOME`/`CODEX_HOME`；禁止在用户真实目录验证写路径
4. **无网络依赖**：单测不打真实接口；wham 解析用内置 JSON 样本（真实响应脱敏后可补充为 fixture）

## 验收清单（变更合入前）

- [x] `cargo fmt` → `cargo clippy -- -D warnings` → `cargo test` 全绿
- [x] 新行为有对应断言（不是只跑旧测试）
- [x] v0.5 新能力在一次性 CLI 与 TUI 可达；classic/GUI 若不接入须符合冻结边界，shared core 回归通过
- [x] JSON 输出不含 token，未知参数/选择歧义/守卫失败具有非零退出码
- [x] 真机冒烟已做并声明覆盖范围（见 v0.5.0 CLI/TUI 计划验收记录）
- [ ] Linux/macOS/Windows 的 `cfg` 分支至少在对应 runner 完成 clippy/build
