# Linux 原生 GUI 接手说明

> 状态：**尚未实现**。截至 2026-09-23，仓库没有 `apps/linux/`、Linux 原生 GUI CI 或 Linux 原生发行物；已发布的 Linux egui GUI v0.6.0 仅作历史版本保留，不能作为新实现继续维护。
>
> 本文是切换到 Linux 开发机后的执行入口，不是 Linux 功能已完成的声明。权威任务状态与跨端行为契约见 [原生 GUI 迁移计划](native-gui-migration-plan.md) 的第 2、3、5、6、8 节。

## 到 Linux 机器后先做什么

1. 先读根目录 `AGENTS.md`、本文、[项目 Spec](../requirements/project-spec.md)、[原生 GUI 迁移计划](native-gui-migration-plan.md)、[配额缓存计划](quota-cache-refresh-plan.md)、[S11 场景](scenarios.md)和[测试策略](../quality/test-strategy.md)。若分支已有新提交，先核对任务勾选与代码现状，不沿用本交接日期的结论。
2. 执行 N28：记录 `/etc/os-release`、`uname -m`、`rustc -vV`、`pkg-config --modversion gtk4`、`pkg-config --modversion libadwaita-1`、桌面会话类型（Wayland/X11）及可用的图形登录环境。确定目标发行版、最低 GTK/Libadwaita 版本和依赖安装方式后，才选择 Rust crate 版本与 feature；不要从 Mac 构建或浮动 CI runner 推断 Linux 兼容性。[gtk-rs 的 Linux 安装说明](https://gtk-rs.org/gtk4-rs/git/book/installation_linux.html)可作为目标发行版依赖核查入口。
3. 检查工作区与 CLI/Core 基线，所有会写账号库或官方认证文件的测试使用隔离 `HOME`/`CODEX_HOME`。真实账号交互需单独获得用户授权并在隔离副本上验证；不得在真实 HOME 跑写路径测试。

## 目标边界

| 所属 | 当前基线与 Linux 实现责任 |
|------|--------------------------|
| `crates/hangar-core` | 账号、OAuth、配额缓存、推荐、令牌健康、写入守卫的唯一业务实现；Linux GUI 直接依赖，不复制规则。 |
| `apps/cli` | 已发布 v0.7.0 CLI/TUI，继续直接依赖 Core；共享 API 变化需回归。 |
| `crates/hangar-uniffi`、`apps/macos` | macOS 适配与 SwiftUI 实现，可参考脱敏字段和用户行为，但不是 Linux 的 FFI 或 UI 模板。 |
| `apps/linux` | **待在 Linux 主机创建**的 Rust + GTK4/Libadwaita 应用。GTK 控件、导航、焦点、窗口和状态图标只归这一层；不得让 Core 依赖 GTK。 |

Linux 与 CLI/macOS 共享 `~/.hangar/accounts.json`、`~/.hangar/quota-cache.json` 和官方 Codex 配置路径，不另建 GUI 专用账号库。保留 S11“先写账号库、后写官方 `auth.json`”、OAuth 1455/1457 回调白名单、默认 `file` 凭据存储与 `keyring`/`auto` 拒绝策略。新增账号和定向重新登录都不能自动切换；只有用户显式切换才改当前账号。界面只接收脱敏摘要，不输出或日志化 token。

## 按任务推进与验收

| 任务 | Linux 主机上的最小可验收结果 |
|------|----------------------------|
| N28 环境基线 | 记录发行版/架构、GTK/Libadwaita 实装版本、图形会话、状态图标支持情况及目标兼容范围。若状态图标不可靠，先确定可恢复窗口的降级入口，不能直接照搬 macOS 菜单栏或旧 egui SNI。 |
| N29 应用骨架 | 新建 `apps/linux` 并纳入 Cargo workspace；在真实 Linux 桌面启动窗口，从 Core 读取一次脱敏账号快照。CLI 无 GUI 初始化依赖，Core 不引入 GTK。 |
| N30 总览 | 典型 7～8 张卡片可同时浏览；当前/失效/推荐、周剩余、重置时间、计划、重置卡和 AT/RT 健康可见。启动先显示共享缓存；旧/失败/未知不伪装为 0%，旧数据不能产生推荐。排序只改变展示，不改账号库顺序。 |
| N31 操作与生命周期 | 后台串行额度刷新、generation/取消守卫、显式切换、添加与定向重新登录、删除、自检；关闭窗口、驻留/恢复、真正退出分别定义并实测。网络与文件任务不得阻塞 GTK 主循环。 |
| N32 真机验收 | 在目标 Linux 图形会话分别验证账号总览、周额度/推荐、OAuth、切换、键盘/输入法、Wayland/X11 范围、状态图标或约定的降级入口。记录实际验收账号数；不足 7～8 个时不要勾选完整条目。 |
| N33 CI/打包 | 仅在 Linux 目标环境建立原生 GUI 构建、依赖与产物校验；包名、架构、运行依赖和 SHA 取真实构建结果。CLI 的 `v*` 发布通道不因 GUI 改动触发。 |

配额规则使用 Core 已实现的 30 分钟 TTL、失败保留旧值和仅新鲜周剩余参与的 10 个百分点稳定推荐；5h 额度暂不进入界面或推荐。前台/驻留定时器由 Linux 应用拥有，完全退出后不新增常驻服务。具体跨端行为表见[迁移计划 §5.1](native-gui-migration-plan.md#51-跨端行为基线)。

## 验证与发布边界

- 每批代码先在隔离 `HOME`/`CODEX_HOME` 下依次运行 `cargo fmt`、`cargo clippy -- -D warnings`、`cargo test`，再在 Linux 图形会话构建、启动并验收受影响流程。GTK/Libadwaita 缺失时先报告环境缺口，不能用 Core/CLI 测试通过代替 Linux GUI 验收。
- macOS arm64 的 N23 只证明本机 Release 配置和六账号隔离副本只读展示；不能证明 Linux 构建、托盘、发行格式、x64 或 Wayland 行为。Linux N28–N33 当前一律未完成。
- 本轮决定等 Linux 原生版实现并验收后再协调发布。不要复活旧 egui 工作流，不创建 GUI 标签/Release，不把 macOS 的 ad-hoc 签名 App 或 CLI 的 `latest` 当作 Linux GUI 发行结果。
