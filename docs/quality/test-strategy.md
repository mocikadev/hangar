# 测试策略

## 现状

- 20 个单测（hangar-core 15 + cli 5），全部模块内 `#[cfg(test)]`，`cargo test` 单命令运行
- 无 CI、无集成测试目录；端到端依赖 tmux 真机冒烟（人工）

## 分层测试策略

| 层 | 方式 | 位置 |
|----|------|------|
| 纯函数（JWT/email/时间/配额解析/路径） | 表驱动单测，基准值注明生成方式 | core/cli 各模块 `mod tests` |
| 文件 I/O（原子写/备份/权限/锁） | 临时目录真实读写断言（如 `atomic_write_keeps_backup_private`） | core account.rs |
| TUI 渲染 | `ratatui::backend::TestBackend` 快照断言 | cli tui.rs |
| 前端分发/守卫逻辑 | 构造 `App` 假数据直接断言（如 `palette_visible` 过滤） | cli tui.rs |
| 端到端 | tmux 真机冒烟：`send-keys` + `capture-pane -p` 验证关键行 | 人工/会话内 |
| 真实凭据链路 | 用户真机操作回报输出（登录/刷新/配额成功路径不可离线模拟） | 用户协同 |

## 已知约束（写测试时必读）

1. **CJK 断言**：TestBackend buffer 中宽字符占两格、续格为空格（"详情"存为"详 情"），中文 `contains` 断言只用**单字**；ASCII 可整串
2. **时区无关**：时间格式化断言只验格式（长度/分隔符），不硬编码日期；需要具体值的用固定偏移环境
3. **隔离沙箱**：任何触盘测试用临时 `HOME`/`CODEX_HOME`；禁止在用户真实目录验证写路径
4. **无网络依赖**：单测不打真实接口；wham 解析用内置 JSON 样本（真实响应脱敏后可补充为 fixture）

## 验收清单（变更合入前）

- [ ] `cargo fmt --check && cargo clippy -- -D warnings && cargo test` 全绿
- [ ] 新行为有对应断言（不是只跑旧测试）
- [ ] TUI 与 classic 双前端行为一致（新增能力两边都暴露）
- [ ] 真机冒烟已做并声明覆盖范围（验了什么/没验什么）
