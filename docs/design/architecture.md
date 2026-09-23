# 技术设计：hangar

> 架构、模块边界与关键机制。场景级风险矩阵见 `scenarios.md`。

> 当前源码已完成原生迁移 Phase 1–2 与 macOS SwiftUI 本机实现；macOS GUI 尚未发布，Linux GTK4/Libadwaita GUI 尚未实现，egui 已删除。原生目标架构见 [`native-gui-migration-plan.md`](native-gui-migration-plan.md)，Linux 主机工作从 [接手说明](linux-native-gui-handoff.md) 开始。

## 分层架构

```
┌──────────────────── apps/cli（bin: hangar）────────────────────┐
│  main.rs      入口：TTY 检测 → tui::run() / classic_loop()             │
│  args.rs      一次性 CLI 参数协议与兼容 flags                           │
│  commands.rs  子命令用例编排、退出语义                                  │
│  selector.rs  账号 ID/唯一邮箱选择器                                    │
│  output.rs    human/JSON 脱敏输出                                       │
│  tui.rs       全屏 TUI（ratatui）：列表/详情/配额/命令面板/覆盖层        │
│  tui_overview.rs 账号总览表：响应式列、令牌/配额快照映射                  │
│  classic.rs   经典菜单：stdin 编号交互（非 TTY / --classic 回退）        │
│  ui.rs        ANSI 样式包装（经典模式）；静默开关代理自 core::emit       │
└──────────────────────────────┬────────────────────────────────────────┘
                               │ 直接依赖 core 公开 API
┌──────────────────────────────▼────────────────────────────────────────┐
│                 crates/hangar-core（lib: hangar-core）               │
│  account.rs   账号库：load/save（原子写+.bak+锁）、harvest、切换、      │
│               复活（reauth）、删除、JWT 工具（exp/email/account_id）    │
│  oauth.rs     OAuth PKCE：授权 URL（hosted login）、1455/1457 回调、    │
│               code 换 token、RT 静默刷新（错误脱敏）                    │
│  quota.rs     wham/usage 配额解析（窗口按时长分类）、重置卡只读、       │
│               本地时间格式化（libc localtime_r，无 chrono）             │
│  quota_cache.rs 版本化配额快照、30 分钟有效期、跨进程合并写              │
│  doctor.rs    离线自检：库解析/权限/凭据完整性/官方一致性/锁            │
│  login.rs     登录+入库（三元组去重，不自动切换）组合动作                 │
│  process.rs   Codex 进程检测（pgrep/ps/tasklist，排除自身）            │
│  emit.rs      输出总线 + 线程级静默（TUI 后台线程不打花屏幕）           │
│  token_health.rs 纯令牌健康规则（JWT 优先、账本回退、投影判定）           │
└──────────────────────────────────────────────────────────────────────┘
```

**边界规则：**

- `hangar-core` 不依赖任何前端 crate；前端不绕过 core 直接读写账号库
- 业务提示一律走 `emit::emit/emit_err`（可被 TUI 静默）；只有前端做 ANSI 着色
- TUI 与 classic 共享 `do_login/doctor_lines/switch_account` 等组合函数，不允许复制业务逻辑
- egui GUI 已从主干删除；已发布 0.6.0 与历史设计文档只作归档

### 原生架构（macOS 已实施，Linux 待实施）

```text
apps/macos (SwiftUI) -> crates/hangar-uniffi -> crates/hangar-core
apps/linux (GTK4/Libadwaita) -------------> crates/hangar-core
apps/cli (CLI/TUI/classic) ----------------> crates/hangar-core
```

- egui 前端已从主干删除；已发布 0.6.0 由 Release 与 Git 历史保留。
- macOS 通过 UniFFI 消费脱敏 DTO；Linux 在 Rust workspace 内直接依赖 Core；CLI 始终直接依赖 Core。
- 推荐规则、账号/配额语义、凭据守卫和数据路径归 Core；导航、焦点、卡片布局、窗口和菜单归平台 UI。
- 原生 UI 按宿主平台落地：macOS 任务只在 macOS 主机实现和验收，Linux 任务只在 Linux 主机实现和验收。不得为不存在或当前不可验证的平台生成占位 UI。

### CI 与发布边界

- CLI/Core CI 已在 Linux、macOS、Windows runner 执行定向检查；`.github/workflows/native-macos-ci.yml` 已在 macOS runner 跑通 Core/UniFFI、Swift 排序与 Debug/Release App 构建。CI 构建不等于真机交互或已签名发行。
- `vX.Y.Z` 是 CLI 发布通道，只生成 Linux/macOS 独立二进制，并保持为 GitHub `latest`，供安装脚本和 CLI 自升级使用。
- 旧 egui GUI 发布工作流已删除；macOS 原生 GUI 使用独立 `macos-gui-vX.Y.Z` 通道，不设为 GitHub `latest`，不触发 CLI 安装脚本或自升级。首个原生版 0.7.0 沿用旧 GUI 的 `dev.mocika.hangar` 与 `Hangar.app`，而 CLI 仍是独立 `hangar` 命令。Linux 阶段在 Linux 主机另定发行入口，不复用三平台 Rust GUI 矩阵。
- 当前 crate 依赖方向为 `apps/cli → crates/hangar-core` 与 `crates/hangar-uniffi → crates/hangar-core`；桥接不得成为 Core 或 CLI 的反向依赖。

### CLI 协议边界

- 参数解析、账号选择、输出格式和退出码属于 `apps/cli`，不得进入 core
- core 的 `Account` 含明文凭据，禁止直接序列化到 stdout；JSON 必须映射为显式脱敏对象
- 一次性命令不隐式自升级；默认无子命令时才沿用 TTY → TUI、非 TTY → classic 的交互分发
- selector 只接受完整内部 ID或唯一邮箱；歧义时失败，不使用易漂移的列表编号
- 切换链路通过 core 的稳定错误类别 `State/Auth/External/Internal` 映射 CLI 退出码；CLI 不解析中文错误文案

## 关键机制

### 1. harvest 收敛（S2/S5）

每次用户交互前执行（菜单循环每轮，非仅进程启动）：

1. 读官方 `auth.json`，解 `id_token` JWT 的 email
2. 按 `email + account_id + organization_id` 三元组匹配库中账号（同邮箱多 org 不合并）
3. 匹配到 → 采纳官方更新的 token（拒收过期 id_token）；未匹配 → 收编为新账号
4. 全程持账号库锁

### 2. 切换投影（S11）

`凭据存储模式预检（仅 file）→ 令牌健康判定 → stale/空 AT 守卫 → 静默刷新（若临期）→ 再次确认可投影 → 先写库 → merge 写官方 auth.json → 清理 config.toml 路由`。

有效期以 access token JWT `exp` 为第一依据，仅在无法解析时回退本地 `expires_at`。明确的 OAuth 鉴权拒绝会标 stale；网络、超时或临时服务错误不会误标 stale，但当原 AT 已过期或临期时仍中止切换，保证官方登录态不被不可用凭据覆盖。

中间任一步崩溃：库中新 RT 已落盘，下次操作自愈；官方文件不会被半截写入（原子写）。

### 3. 原子写 + 备份 + 锁

- 唯一临时文件（pid+nanos）→ rename，避免双实例互踩
- 写前 copy `.bak`（600 权限）；`load` 解析失败自动从 `.bak` 回滚
- `.accounts.lock`：`create_new` 独占 + 120s 过期自愈（覆盖 refresh 网络持有期）+ 同线程可重入

### 4. OAuth 白名单

`redirect_uri` 仅允许 `http://localhost:{1455,1457}/auth/callback`（官方 Hydra 白名单）；绑定直接 try 两个端口（无 probe，无 TOCTOU）；5 分钟超时后支持手动粘贴回调 URL（校验 host/path/state）。

### 5. TUI 与业务层协作

- 网络/长耗时任务在后台线程执行，经 `mpsc` 回事件刷新 UI
- 需要整屏交还终端的流程（浏览器登录/手动粘贴）用挂起-恢复：退出 alt-screen → 跑阻塞流程 → 重进 TUI
- 后台线程 `set_quiet(true)`，业务 emit 不直写终端
- 首屏先载入共享配额快照，后台只查询到期的非 stale 账号；运行期间每分钟检查一次，手动刷新强制查询。旧值可见并标记，失败保留旧值；未查询/查询中/失败分别显示，不把缺失配额解释为 0%
- `recommendation` 只读取非 stale 账号且新鲜、成功、周剩余已知的快照；当前账号距最高值不超过 10 个百分点时建议保持当前账号。该模块不联网、不刷新令牌、不切换或写文件
- 日志默认折叠为状态区，`l` 展开；窄终端由 `tui_overview` 降级非关键列

### 6. 原生 GUI 边界（macOS 实施中）

- macOS SwiftUI 通过 `hangar-uniffi` 消费脱敏 DTO；网络和凭据任务由 Rust 后台执行，展示状态回到 `MainActor`
- `hangar-uniffi` 持有配额/OAuth 任务句柄和 generation；SwiftUI 只轮询脱敏快照。Xcode 构建阶段通过 `scripts/generate-swift-bindings.sh` 重建静态库与 Swift 绑定，生成物留在忽略的 `build/`。
- 菜单栏回调只提交类型化请求，不直接复制切换、刷新或 stale 守卫
- 主窗口和菜单栏共用 UniFFI 快照及 `quota-cache.json`；菜单按账号显示周剩余、旧数据与推荐标记，不额外请求接口。驻留进程每分钟及唤醒时检查到期，退出后停止
- 关闭窗口、隐藏 Dock、恢复窗口与退出进程是四个独立生命周期事件
- Linux GUI 只在 Linux 主机实现，其 GTK/Libadwaita 生命周期和状态图标策略不得由 macOS 阶段预设

## 外部接口（非公开契约，全容错）

| 接口 | 用途 | 已验证来源 |
|------|------|-----------|
| `POST auth.openai.com/oauth/authorize|token` | OAuth | openai/codex server.rs |
| `GET chatgpt.com/backend-api/wham/usage` | 配额 | cockpit-tools codex_quota |
| `GET chatgpt.com/backend-api/wham/rate-limit-reset-credits` | 重置卡明细 | 同上 |
服务端响应字段随时可能变化：解析全部容错，缺失即显示"未知"，失败只影响单行展示。

## 扩展路径

- 新前端（守护进程/HTTP API）→ 新 crate 依赖 hangar-core
- 多 CLI 工具支持（cursor/zed 等）→ hangar-core 引入 provider 抽象，account/quota 按 provider 分模块
