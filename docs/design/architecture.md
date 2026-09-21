# 技术设计：hangar

> 架构、模块边界与关键机制。场景级风险矩阵见 `scenarios.md`。

## 分层架构

```
┌─────────────────── crates/cli（bin: hangar）───────────────────┐
│  main.rs      入口：TTY 检测 → tui::run() / classic_loop()             │
│  tui.rs       全屏 TUI（ratatui）：列表/详情/配额/命令面板/覆盖层        │
│  classic.rs   经典菜单：stdin 编号交互（非 TTY / --classic 回退）        │
│  ui.rs        ANSI 样式包装（经典模式）；静默开关代理自 core::emit       │
└──────────────────────────────┬────────────────────────────────────────┘
                               │ 仅依赖 core 公开 API
┌──────────────────────────────▼────────────────────────────────────────┐
│                    crates/hangar-core（lib: hangar-core）             │
│  account.rs   账号库：load/save（原子写+.bak+锁）、harvest、切换、      │
│               复活（reauth）、删除、JWT 工具（exp/email/account_id）    │
│  oauth.rs     OAuth PKCE：授权 URL（hosted login）、1455/1457 回调、    │
│               code 换 token、RT 静默刷新（错误脱敏）                    │
│  quota.rs     wham/usage 配额解析（窗口按时长分类）、重置卡只读、       │
│               本地时间格式化（libc localtime_r，无 chrono）             │
│  doctor.rs    离线自检：库解析/权限/凭据完整性/官方一致性/锁            │
│  login.rs     登录+入库（三元组去重）+切换 组合动作                     │
│  process.rs   Codex 进程检测（pgrep/ps/tasklist，排除自身）            │
│  emit.rs      输出总线 + 线程级静默（TUI 后台线程不打花屏幕）           │
└──────────────────────────────────────────────────────────────────────┘
```

**边界规则：**

- `core` 不依赖任何前端 crate；前端不绕过 core 直接读写账号库
- 业务提示一律走 `emit::emit/emit_err`（可被 TUI 静默）；只有前端做 ANSI 着色
- 两个前端共享 `do_login/doctor_lines/switch_account` 等组合函数，不允许复制业务逻辑

## 关键机制

### 1. harvest 收敛（S2/S5）

每次用户交互前执行（菜单循环每轮，非仅进程启动）：

1. 读官方 `auth.json`，解 `id_token` JWT 的 email
2. 按 `email + account_id + organization_id` 三元组匹配库中账号（同邮箱多 org 不合并）
3. 匹配到 → 采纳官方更新的 token（拒收过期 id_token）；未匹配 → 收编为新账号
4. 全程持账号库锁

### 2. 切换投影（S11）

`stale/空 AT 守卫 → 静默刷新（若临期）→ 先写库 → merge 写官方 auth.json → 清理 config.toml 路由 → macOS keychain 同步`。

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

## 外部接口（非公开契约，全容错）

| 接口 | 用途 | 已验证来源 |
|------|------|-----------|
| `POST auth.openai.com/oauth/authorize|token` | OAuth | openai/codex server.rs |
| `GET chatgpt.com/backend-api/wham/usage` | 配额 | cockpit-tools codex_quota |
| `GET chatgpt.com/backend-api/wham/rate-limit-reset-credits` | 重置卡明细 | 同上 |
| `security add-generic-password` | macOS keychain | 同上（`ps` 泄漏风险已声明） |

服务端响应字段随时可能变化：解析全部容错，缺失即显示"未知"，失败只影响单行展示。

## 扩展路径

- 新前端（守护进程/HTTP API）→ 新 crate 依赖 hangar-core
- 多 CLI 工具支持（cursor/zed 等）→ hangar-core 引入 provider 抽象，account/quota 按 provider 分模块
