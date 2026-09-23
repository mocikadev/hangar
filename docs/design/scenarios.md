# Token 一致性场景矩阵与方案设计

## 工程结构（Cargo workspace）

```
hangar/
├── Cargo.toml                    # [workspace]：hangar-core + cli
├── crates/
│   └── hangar-core/             # hangar-core（库）：业务层，UI 无关
│       └── src/
│           ├── account.rs       # 账号库 CRUD/harvest/切换/复活/删除、原子写+文件锁、JWT 工具
│           ├── oauth.rs         # OAuth PKCE 登录、token 交换/刷新
│           ├── quota.rs         # wham/usage 配额、重置卡（只读）、本地时间格式化
│           ├── doctor.rs        # 离线自检
│           ├── login.rs         # 登录+入库+切换组合动作
│           ├── process.rs       # Codex 进程检测
│           └── emit.rs          # 输出总线 + 线程级静默（TUI 用）
└── apps/
    └── cli/                     # hangar（bin）：TUI + classic 共享 core
        └── src/
            ├── main.rs          # 入口：TUI / classic 分发
            ├── tui.rs           # 全屏 TUI（ratatui）
            ├── classic.rs       # 经典菜单（非 TTY / --classic 回退）
            └── ui.rs            # ANSI 样式（经典模式用）

扩展方向：新增前端（守护进程/HTTP API）→ 新 crate 依赖 core 即可；
支持其他 AI CLI → core 增加 provider 抽象。
```

## 背景

cockpit-tools 作为后台常驻进程，能实时观察官方 auth.json 变化、主动刷新。
hangar 是**按需运行的 CLI**（每次调用存活几秒），没有常驻能力。
因此方案必须围绕「**每次命令执行时做完整的一致性收敛**」设计，而非后台监听。

核心不变量（invariant）：
> **官方 auth.json 永远是最终权威（Authority）。** Codex 客户端运行中会自行
> 刷新并轮换 refresh_token，它写下的就是最新事实。我们的账号库是缓存，
> 每次操作前必须先向权威源收敛。

## 生命周期时序

```
账号库(~/.hangar/accounts.json)          官方 auth.json
        │                                              │
        │ ① 登录(唯一写入方是我们)                        │
        ├──────────────────────────────────────────────▶│ 写入
        │                                              │
        │                        ② Codex 运行中自行刷新  │ RT 轮换
        │                                              │
        │ ③ 我们任何操作前: harvest(收敛) ◀──────────────│ 读取
        │ ④ 过期则 refresh(用最新RT)                     │
        │ ⑤ 写入目标账号 → 官方文件被覆盖                  │
        │                    ⑥ Codex 下次启动用新凭据刷新 │ RT 再轮换
        │ ⑦ 我们下次任何操作: 再次 harvest ◀─────────────│
```

## 场景矩阵

| # | 场景 | 风险 | 方案 | 状态 |
|---|------|------|------|------|
| S1 | 登录新账号 | 自动切换会意外覆盖当前官方登录态 | OAuth 完成后仅入库，不改 current/auth.json；用户显式选择后切换 | ✅ 已实现 |
| S2 | Codex 长时间运行后，用户切回该账号 | 库中 RT 已被官方轮换 | **每次操作前 harvest**（读官方 auth.json，按 id_token 的 email 归属，采纳更新的 token 回存库） | ✅ 已实现 |
| S3 | 切换到长期未用的账号，AT 过期 | AT 失效 | JWT `exp` 优先、账本回退：过期/5min 内将过期 → 用 harvest 后的最新 RT 静默刷新 → 回存；缺 RT 或临时刷新失败均拒绝投影 | ✅ 已实现 |
| S4 | refresh_token 也失效（服务端撤销/轮换链断裂） | 静默刷新 401 | 标 stale + 拒绝污染官方 + `r` 定向复活（原位覆盖，邮箱Mismatch拒绝） | ✅ 已实现 |
| S5 | 用户在 Codex CLI 里手动 `codex login` 换了个全新账号 | 官方文件是新账号，库中无此人 | harvest 反向收编：官方 email 不在库中时自动入库（含 account/org 三元组） | ✅ 已实现 |
| S6 | 用户手动改了/删了官方 auth.json | 内容为空或非法 | harvest 容错：解析失败跳过；账号库损坏时从 `.bak` 自动回滚 | ✅ 已实现 |
| S7 | 删除账号时该账号正是官方当前登录 | 官方文件仍指向它 | **硬约束**：禁止删除使用中账号（UI 拦截 + delete_account 守卫），杜绝官方 auth.json 残留无主明文凭据；先切换再删 | ✅ 已实现 |
| S8 | 多终端/手动拷贝 accounts.json 合并 | 同 email 两条记录 | 按 email+account/org 三元组去重（老库退化为 email） | ✅ 已实现 |
| S9 | 切换发生在 Codex 正在运行时 | 我们覆盖后 Codex 又回写旧 token，或内存凭据打架 | `pgrep/ps/tasklist` 多模式检测（排除自身），提示重启生效 | ✅ 已实现 |
| S10 | 时钟偏差导致误判过期 | 提前刷/漏刷 | skew 阈值 300s 已覆盖秒级偏差；不做 NTP 级处理 | ✅ 已覆盖 |
| S11 | 我们刷新成功但写库前崩溃 | 新 RT 丢失（旧 RT 已被服务端作废） | 刷新后**立即先写库再写官方文件**；激活账号在配额刷新发生轮换时也按此顺序同步投影，非激活账号只落库 | ✅ 已按此顺序 |
| S12 | 并发运行两个 hangar 实例 | harvest+switch 竞态写库 | 低概率（人手操作），accounts.json 原子写保证不损坏；最多丢一次 harvest | 🟢 接受 |
| S13 | 临期/过期 AT 刷新遇到网络或服务临时故障 | 继续切换会用坏 AT 覆盖当前官方登录态 | 临时错误不标 stale；仅当原 AT 仍新鲜时允许继续，否则中止且不写 auth.json | ✅ 已实现 |

## 实现优先级（已落地）

- S4/S5/S8/S9 均已实现；`r` 复活、端口回退+手动回调、`.bak` 回滚、文件锁已补齐

## 一句话架构

```
每次交互前（菜单循环每轮，而非仅进程启动时）:
    1. harvest : 官方 auth.json → 账号库（已知账号更新 / 未知账号收编，归属账号标为使用中，全程持锁）
    2. refresh : 目标账号 AT 过期 → 用最新 RT 静默刷新（失败则标 stale，401 细分 error_code）
    3. project : 预检官方凭据存储为 file → merge 为 AuthDotJson 格式 → 原子覆盖官方 auth.json → 重置 config provider
```

## 新功能（切换之外）

- `u` 配额：手动操作逐账号实时查 `wham/usage`，查前保证 AT 新鲜，401 则强制刷新重试一次，`stale` 账号跳过。成功结果写共享 `quota-cache.json`；TUI/原生 GUI 启动先显示缓存，只自动查询 30 分钟到期的账号，失败保留标记为旧的值。推荐仅使用新鲜、成功、周剩余已知的结果，不自动切换；classic 保持基础兼容
- `doctor` 自检：纯离线，覆盖账号库解析/备份/权限、逐账号凭据完整性与过期、`auth.json` 一致性、`config.toml` 冲突路由、残留锁

## 交互（TUI）

- 默认全屏 TUI（`ratatui`）：`j/k` 移动，`/` 过滤，回车切换，`q` 退出——主界面只留安全键
- `:` 命令面板（两段式，参照 CLI agent）：默认 `j/k` 上下选 + 回车执行；`/` 进入过滤（中英文关键字），`ESC` 逐级退出
- 所有动作（添加/复活/删除/配额/自检/收敛）收进面板，删除在面板选中后仍需 `y` 二次确认——杜绝单键误触
- 守卫：回车切换使用中账号只提示不重写 auth.json；使用中账号禁止删除（UI + 底层双层拦截）
- 网络任务走后台线程 + spinner，界面不卡；浏览器登录/手动粘贴 URL 时挂起 TUI 跑原阻塞流程后恢复
- 后台线程与 TUI 主线程经线程级静默开关隔离 stdout；非 TTY 或 `--classic` 自动回退经典菜单

## 交互（GUI/托盘）

- GUI 与 CLI 共用 `~/.hangar/accounts.json` 和 core 业务 API，不直接读写凭据文件
- 添加账号仅入库；重新登录账号只原位更新凭据，不改变当前账号。目标恰为当前账号时，按 S11 在账号库落盘后同步官方 `auth.json`
- 托盘切号复用 GUI 的 current/stale/busy 守卫，切换结果经 worker 事件回主线程收敛
- Linux 托盘菜单已真机验证；macOS/Windows 验收状态见 `v0.4.0-closure-plan.md`

## 安全声明（明文存储）

- `~/.hangar/accounts.json` 与官方 `auth.json` 均为明文 token，靠目录 `700` + 文件 `600` 收紧；
- 未做系统钥匙串加密（`Linux Secret Service / macOS keychain / Windows DPAPI` 为后续项）；当前只支持官方默认的 `cli_auth_credentials_store = "file"`；
- 显式配置 `keyring` 或 `auto` 时，切换会在刷新和写入前失败，不会只改 `auth.json` 后误报成功；
- 错误日志只记 `HTTP status + error_code + body_len`，不回显完整 body；上报 issue 前请脱敏 email/token。

## 已知接受的风险（论证后不修）

**S12 双实例并发（已缓解，残余自愈）**

`with_accounts_lock` 已让 harvest/switch/add/delete 全程持锁（同线程可重入），
`.lock` 120s 过期自愈 + 10s 获取超时；较长阈值覆盖持锁刷新网络请求，残余跨机器拷贝合并仍靠 harvest 自愈。

**其他记录**
- `config.toml` 不管理：官方文件中的 `http_headers.Authorization` 等配置
  不属于登录态，切换只负责 auth.json
- 仅支持 OAuth（chatgpt 模式）账号，不实现 API-Key 模式
