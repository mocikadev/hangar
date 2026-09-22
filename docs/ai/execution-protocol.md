# AI 执行协议

## 适用范围

本协议规定 AI 代理在 hangar 项目内处理需求、实现变更、修复缺陷和发布交付时的最低执行标准。全局代理规则继续生效；本文只写项目额外要求。

## 任务分级

| 等级 | 判断标准 | 最低流程 |
|------|----------|----------|
| 小改动 | 文案、注释、日志文案、无行为变化 | 直接修改 + `cargo test` |
| 普通变更 | 单模块行为变化（如某个命令的守卫、一条展示行） | 澄清 + 简短方案 + 实现 + 测试 + 真机冒烟 |
| 复杂变更 | 跨模块、OAuth 流程、存储格式、TUI/GUI 结构、新增前端 | spec 更新 + 计划 + 分批实现 + 回归 + 三前端验证 |
| 高风险变更 | 凭据存储格式、auth.json 投影逻辑、锁语义、删除流程 | 影响说明 + 用户确认 + 执行契约 + 验证证据 |

**本项目特有升级条件**（满足任一即从"小改动"升级）：

- 触碰 `codex_account.rs` 的写入顺序（先库后官方是 S11 硬约束）
- 触碰 OAuth 常量（redirect_uri/端口/client_id——白名单匹配是登录成败关键）
- 改 `Account` 结构体字段（涉及 accounts.json 兼容性，需 serde default 老库兼容验证）
- 改锁语义（过期阈值/重入）——涉及双实例竞态

## 开始或恢复任务

1. 读 `AGENTS.md` → 对应 docs 文档
2. 检查 `SCENARIOS.md`（现 `docs/design/scenarios.md`）场景矩阵中相关场景的状态标记
3. 存在未验证改动时，先跑 `cargo test` 确认基线，再继续

## 新需求流程

1. 需求不清时先补 `docs/requirements/project-spec.md`（目标/范围/非目标/验收）
2. 实现方案先说明再动码；用户中途改目标先更新 spec

## 实现约束（项目特有）

- **S11 顺序不可倒置**：刷新后先 `save_accounts_unlocked` 落库，再写官方 auth.json
- **守卫完备性**：任何会写官方 auth.json 的操作必须先检查 `stale` + 空 AT + "使用中"拦截；新增写路径时逐条核对
- **容错解析**：wham/usage 等非公开契约接口，字段缺失显示"未知"，不许 `unwrap` 崩溃
- **UI 无关**：业务函数不得 `println!`，一律 `crate::emit::emit/emit_err`；v0.5 新能力同步评估一次性 CLI 与 TUI。classic 保留基础兼容，GUI 冻结为维护模式；共享 core 变化仍须做 GUI 回归
- **兼容老库**：`Account` 新字段必须 `#[serde(default)]`；不能假定 account_id/organization_id 存在
- **时间展示**：统一走 `quota::fmt_ts_local`（本地时区具体日期时间），不引 chrono
- **无头/管道安全**：`stdin` EOF 优雅退出；`stdout flush` 不 `unwrap`（EPIPE）

## 测试与验证

- 行为变化必须带单测（模块内 `#[cfg(test)]`，基准值注明生成方式）
- TUI 渲染变化用 `TestBackend` 快照断言；GUI 状态变化优先测 reducer/守卫纯逻辑；注意 CJK 宽字符在 buffer 中被空格隔开，中文断言只用单字
- 完成前必须给出新鲜证据：

```bash
cargo fmt --check      # 格式
cargo clippy -- -D warnings
cargo test             # 全部单测（core + cli）
```

- 涉及 TUI 的变更额外做终端冒烟；涉及 GUI/托盘的变更在对应平台做真机冒烟，并声明"验了什么/没验什么"
- 一次性 CLI 的文件写入测试必须使用隔离 `HOME`/`CODEX_HOME`；JSON 输出测试必须断言不含 access/refresh/id token
- 涉及真实凭据链路（登录/刷新/配额成功路径）无法离线验证时，明确请用户在真机操作并回报输出；不得用编造的 expires_at/token 宣称"测试通过"

## 快速路径

- Hotfix（≤2 文件，修复明确缺陷）：直接修 + 上述验证命令
- Tweak（≤4 文件，文档/文案/样式）：`cargo fmt --check` 可省略 clippy，但 test 必须
- 超出边界或命中上文升级条件时，走普通/复杂流程

## 收口与同步

- 行为变化 → 同步 `docs/requirements/project-spec.md` 验收标准（如有影响）
- 场景状态变化（❌→✅ 等）→ 同步 `docs/design/scenarios.md` 矩阵
- 修改 `harvest_locked` 的收编/更新语义 → 必须保持"归属账号 = 使用中"不变量（`current_account_id` 与官方 auth.json 一致）
- 架构/模块变化 → 同步 `docs/design/architecture.md`
- 新增交互键位/守卫 → 同步 scenarios 的「交互（TUI）」小节

## 风险操作（需用户确认）

- 删除/覆盖 `~/.hangar/accounts.json`、`~/.codex/auth.json`
- 修改 `SCENARIOS.md` 的"已知接受的风险"章节（等于推翻已论证决策）
- 在用户真实 HOME 环境跑任何写路径测试（测试一律用隔离 HOME/CODEX_HOME）
