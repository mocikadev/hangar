# hangar

> 此文件是 AI 代理的项目导航地图。详细规范查阅 `docs/` 目录。

## 项目概览

**hangar** — OpenAI Codex 多账号管理 CLI/TUI 工具：免重复登录地在多个 ChatGPT 账号间切换，官方 `auth.json` 为唯一权威，本工具负责缓存、收敛与投影。
当前状态：**v0.3.0 已发布**（TUI + 经典 + egui GUI 三前端、配额、守卫、GUI 打包发版已落地；S1-S12 场景全绿；三平台真机 GUI 冒烟待补，见 docs/design/gui.md 验收标准）

## 技术栈

- Rust 2021（Cargo workspace：`crates/hangar-core` 业务库 + `crates/cli` 前端 bin `hangar`）
- 前端：ratatui + crossterm（TUI）；HTTP：ureq；本地回调：tiny_http
- 无 chrono：时间格式化用 civil_from_days + libc `localtime_r`

## AI 执行协议摘要

- 需求不清时，先补齐 `docs/requirements/project-spec.md`，再进入设计或实现
- 普通/复杂变更遵循 `spec → 设计/计划 → 实现 → 验证 → 汇报`
- **项目特有升级条件与实现硬约束见 `docs/ai/execution-protocol.md`**（S11 写入顺序、OAuth 白名单常量、Account 兼容、双前端同步等，触碰即升级流程）
- 完成前必须给出新鲜验证证据，不得未验证声称完成

## 开发/协作约束

> ⚠️ **高优先级：只记录本项目特有、AI 无法从代码或全局规则推断的协作约束。**

- 测试一律用隔离 `HOME`/`CODEX_HOME` 沙箱；**禁止在用户真实 HOME 跑写路径验证**
- 不得用编造的数据（如假 expires_at/token）宣称"测试通过"；无法离线验证的链路明确请用户真机操作回报
- 除此之外无额外项目约束

## 提交前检查清单

```bash
cargo fmt                                  # 1. 格式化（必须先于 clippy，否则 clippy 报格式错）
cargo clippy -- -D warnings                # 2. lint 零警告
cargo test                                 # 3. 全部单测（core 15 + cli 5）
```

> ⚠️ 顺序不可颠倒：未 `cargo fmt` 先跑 `clippy -- -D warnings` 会因格式问题失败。

## 关键约束

- **S11 写入顺序**：切换/刷新后**先写账号库、后写官方 auth.json**；倒置会在崩溃时丢 RT 并误标 stale（`docs/design/scenarios.md` S11）
- **OAuth 白名单**：`redirect_uri` 仅 `http://localhost:{1455,1457}/auth/callback`，改端口/路径/域名即登录失败
- **跨文件同步**：改 `Account` 结构（`crates/core/src/account.rs`）→ 同步检查 TUI/classic 两前端引用与 `#[serde(default)]` 老库兼容；改场景状态 → 同步 `docs/design/scenarios.md`
- **双前端对等**：新增用户可见能力必须同时进 TUI（命令面板）与 classic 菜单
- **明文安全模型**：凭据明文存储，靠目录 700/文件 600；错误输出只记 `status+error_code+body_len`，禁止打印 token/body
- **非公开契约**：wham/usage 等接口响应全容错解析，字段缺失显示"未知"，不许 panic

## 常用命令

```bash
cargo build --release   # 构建产物 target/release/hangar（~2.3MB）
cargo test              # 单测（core + cli）
./target/release/hangar            # 默认 TUI
./target/release/hangar --classic  # 经典菜单（管道/非 TTY 自动回退）
```

## 文档导航

| 文档 | 路径 |
|------|------|
| README（面向用户：能力/使用/FAQ） | `README.md` |
| 项目 Spec（目标/范围/验收） | `docs/requirements/project-spec.md` |
| AI 执行协议 | `docs/ai/execution-protocol.md` |
| 场景矩阵（S1-S12 风险与方案） | `docs/design/scenarios.md`（原根目录 SCENARIOS.md） |
| 技术设计（架构/机制/外部接口） | `docs/design/architecture.md` |
| 测试策略与验收清单 | `docs/quality/test-strategy.md` |

## Skills 导航

本项目暂无 project skill。候选（触发条件满足时再建，勿提前创建）：

| Skill | 触发场景 | 放置路径 | 原因 |
|-------|----------|----------|------|
| `release-check` | 需要发布 release 包时 | `.opencode/skills/release-check/SKILL.md` | 多步固定流程：fmt→clippy→test→release 构建→tmux 冒烟→体积/二进制验证 |
| `tui-smoke-test` | 验证 TUI 变更时 | `.opencode/skills/tui-smoke-test/SKILL.md` | tmux 会话脚本化验证（send-keys/capture-pane/转义检查），顺序敏感易漏步骤 |
