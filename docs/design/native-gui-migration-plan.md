# 原生 GUI 迁移计划

> 状态：Phase 3/4 进行中；N16–N21 已完成，N22 按缩减范围收口，N24 版本与安装身份已确定（2026-09-23）
> 当前开发宿主：macOS 27.0 / Apple Silicon（arm64），Xcode 27.0，Swift 6.4
> 目标：停止演进 egui 前端，以 Rust 共享核心分别承载 macOS SwiftUI 与 Linux GTK4/Libadwaita 原生应用。

## 1. 已确认决策

1. GUI 最终采用平台原生实现，不再投入成本重构或补齐 egui 界面。
2. macOS 使用 SwiftUI，通过独立 UniFFI 适配层调用 `hangar-core`。
3. Linux 使用 Rust + GTK4/Libadwaita，直接依赖 `hangar-core`，不为了形式统一绕经 FFI。
4. Windows 暂不规划、不创建占位工程、不作为当前迁移的验收条件。
5. CLI/TUI 保持 Rust 实现并直接依赖 Core；原生 GUI 不得成为 CLI 的启动或运行前提。
6. 账号视图改为总览仪表盘：典型 7～8 个账号全部同时可见，不再以“列表选中后才看到详情”为主要结构。
7. 当前只展示、比较和推荐“周剩余额度”；5h 额度未恢复前不进入界面和推荐规则。
8. 已发布的 egui GUI 0.6.0 只作为历史可用版本保留在 Release/Git 历史中；主干源码已删除，不建立长期 legacy 维护分支。

## 2. 宿主平台开发规则

### 2.1 硬约束

原生 GUI 遵循“在哪个平台开发，就只实现哪个平台”的规则：

- macOS 主机只实现和修改 `apps/macos/**`，并在 macOS 上构建、运行、调试和验收 SwiftUI 应用。
- Linux 主机只实现和修改 `apps/linux/**`，并在目标 Linux 桌面会话中构建、运行、调试和验收 GTK4/Libadwaita 应用。
- Windows 未进入范围；任何主机都不得顺手生成 WinUI 占位代码。
- 不得在 macOS 上生成 Linux GUI 源码后声称“待验证”，也不得在 Linux 上代写 macOS 原生 UI。Linux 实现应等切换到 Linux 机器后，依据本文档和共享契约独立落地。

允许在当前宿主修改的共享范围：

- `crates/hangar-core/**`：无 UI 的业务规则、存储、网络和领域模型。
- `crates/hangar-uniffi/**`：仅服务 macOS 的桥接适配；因此在 macOS 阶段实现。
- `apps/cli/**`、共享测试、文档、构建脚本和 CI 编排。

共享改动仍需评估全部实际消费者，但“评估影响”不等于在错误宿主上实现另一个平台。若共享契约尚不能被未实现的 Linux 前端消费，在本文任务表中记录兼容要求，由 Linux 阶段在 Linux 主机闭环。

### 2.2 开始平台任务前的环境证据

每次进入平台阶段，先记录宿主、CPU、SDK/系统库与桌面会话；源码编辑、编译、运行和交互验收分开记录。

| 平台 | 当前状态 | 最低检查 | 可宣称范围 |
|------|----------|----------|------------|
| macOS arm64 | 当前宿主已检查 | `sw_vers`、`uname -m`、`xcodebuild -version`、macOS SDK、`swift --version`、Rust host | 本机实际完成的构建、运行与交互 |
| macOS x64 | 未验证 | 对应架构构建环境与真实运行环境 | 只有真实运行后才能宣称可用；交叉编译仅算构建证据 |
| Linux | 等切换到 Linux 主机 | `/etc/os-release`、CPU、GTK4/Libadwaita、Wayland/X11、可用桌面会话、Rust host | 只记录 Linux 主机实际完成的结果 |
| Windows | 不在范围 | — | 不声明支持 |

当前 Mac 环境快照只说明开发机现状，不等于最低系统版本：macOS 27.0（26A428）、arm64、Xcode 27.0（27A266a）、macOS SDK 27.0、Swift 6.4、Rust 1.98.1。最低 macOS 版本应在采用具体 SwiftUI/UniFFI API 前单独决定。

## 3. 目标目录与依赖方向

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
crates/
  hangar-core/            # 业务事实、凭据、配额、推荐与持久化
  hangar-uniffi/          # Swift DTO / 错误 / 任务适配，不含 UI
apps/
  cli/                    # CLI / TUI / classic，直接依赖 Core
  macos/                  # SwiftUI / Xcode，仅在 macOS 阶段创建和维护
  linux/                  # GTK4 / Libadwaita，仅到 Linux 主机后创建
resources/shared/         # 仅放确实跨端共用的品牌资源
scripts/                  # 可复现的绑定、构建、打包入口
build/                    # Xcode/生成绑定/暂存输出，不提交
dist/                     # 最终发行物，不提交
```

依赖方向：

```text
apps/macos (SwiftUI) -> hangar-uniffi -> hangar-core
apps/linux (GTK4) --------------------> hangar-core
apps/cli ----------------------------> hangar-core
```

边界要求：

- Core 不引用 SwiftUI、GTK、窗口、托盘、导航或展示模型。
- 推荐规则从 CLI 私有模块迁入 Core，CLI 与两个原生 GUI 共享同一结果，不复制算法。
- UniFFI 只导出显式脱敏 DTO；禁止导出 `access_token`、`refresh_token`、`id_token` 或可直接序列化的完整 `Account`。
- GUI 与 CLI 使用同一个账号库和 Codex 配置路径解析，不允许平台应用悄悄回退到另一份空目录。
- 保持 S11：刷新/切换后先写账号库，再投影官方 `auth.json`。
- 当前只支持 Codex 官方默认的 `file` 凭据存储；显式 `keyring`/`auto` 必须在刷新和写入前返回类型化失败，不能降级为只写文件或无条件访问钥匙串。
- 构建产物来自真实构建结果；Cargo、绑定、Xcode 暂存与最终发行物分目录，禁止提交本机输出。

## 4. macOS 信息架构与交互

### 4.1 主窗口

主窗口是账号总览仪表盘，而不是“左侧账号 + 右侧详情”：

- 宽窗口 3 列、中等窗口 2 列、窄窗口 1 列，自适应但不隐藏账号。
- 账号卡片与状态栏账号菜单共用展示顺序：当前账号、推荐账号、其余按周剩余降序、额度未知、需重新登录；当前账号即使失效也置顶，同组同额度保持账号库原顺序。仅改变 macOS 展示投影，不重排账号库，也不改变 CLI/TUI 或 Core 推荐规则。
- 启动时按缓存结果排序；逐账号刷新期间保持已有位置，新账号暂列末尾；整轮刷新完成后统一重排，避免卡片和菜单在返回过程中跳位。
- 顶部摘要显示：当前账号、建议账号、最后刷新时间，以及“刷新全部”“添加账号”。
- 7～8 个账号应能快速纵向浏览；不为了首屏塞满而压缩可读性。

每张卡片至少显示：

- 邮箱；使用中、建议、需重新登录等状态徽标。
- 周剩余额度（主视觉）、具体重置时间、计划名称、重置卡数量与可用卡最早到期时间；没有可用重置卡时明确显示 0 张，不让卡片高度塌缩。
- AT 健康与 RT 可续期状态，不显示 token 内容。
- 主操作“切换”；失效时替换为“重新登录”；删除、诊断等低频操作进入更多菜单。

“建议”采用稳定规则：只考虑非 stale 且周剩余已知的账号；当前账号与最高值相差不超过 10 个百分点时继续建议当前账号。建议只读，不自动切换。

### 4.2 状态与并发

每张卡片独立表达 `未查询 / 查询中 / 成功 / 失败 / 未知 / 需重新登录`，不得用一个全局 busy 遮住全部账号，也不得把未知显示为 0%。

- 启动先显示共享配额缓存；只按账号库顺序串行查询到期的健康账号，降低集中刷新和接口压力。手动刷新强制查询全部健康账号；细则见配额快照计划。
- 每轮刷新携带递增 generation/request id；迟到的旧结果不得覆盖新一轮状态。
- 长任务在 Rust 后台执行；SwiftUI 展示状态只在 `MainActor` 更新，禁止 UI 线程阻塞等待网络或 OAuth。
- 窗口销毁后先解除订阅；应用级任务可按策略完成，但不得继续访问已释放的 Swift 对象。

### 4.3 窗口、菜单栏与 Dock

- 关闭主窗口只关闭窗口，不退出应用；进程继续驻留菜单栏。
- macOS Bundle ID 沿用已发布 GUI 的 `dev.mocika.hangar`，Debug/Release 不得改为开发者个人反向域名；迁移后用构建产物 `Info.plist` 回读。
- 主窗口关闭后 Dock 不显示应用图标，只保留菜单栏入口。
- 从菜单栏选择“显示 Hangar”时恢复 Dock 与主窗口；选择“退出”才结束进程。
- 菜单栏至少提供：当前账号、每账号周剩余及旧数据/推荐提示、账号快捷切换、刷新周额度、显示主窗口、退出。菜单操作复用 Core 守卫和主窗口快照，不另写切换或查询规则。
- 关闭、恢复、退出和任务清理必须作为不同生命周期事件测试。

### 4.4 OAuth

OAuth 在桥接层采用显式会话接口，不把 stdin 或 Swift 回调对象塞进 Core：

```text
start_login / start_reauth -> session_id + authorization_url
submit_callback(session_id, callback_url)
cancel_login(session_id)
poll_result / typed completion
```

实现时必须继续使用白名单 `http://localhost:{1455,1457}/auth/callback`，并定义会话取消、超时、完成竞态和释放顺序。具体 UniFFI async/callback 形式以锁定版本能力验证后确定，不在计划阶段虚构 API。

### 4.5 macOS 版本与安装身份

- Xcode 部署目标定为 macOS 14.0；这是构建声明，不是 macOS 14 真机兼容证明。目前只验证 macOS 27.0 arm64。早期系统与 x64 运行均保持未验证，发行范围不得由部署目标推断。
- 原生 App 沿用旧 GUI 的 Bundle ID `dev.mocika.hangar`，包名和显示名为 `Hangar.app` / Hangar；替代旧 GUI 时先退出旧进程，再安装新 App，不让同 Bundle ID 的两版并行运行。CLI 仍为独立的 `hangar` 命令，安装于 `~/.local/bin`，与 App 共用 Core 的账号库和官方 Codex 配置路径，不互相捆绑或改写安装位置。
- CLI 保留 `vX.Y.Z` 标签、GitHub `latest`、安装脚本和自升级通道。macOS 原生 GUI 采用独立 `macos-gui-vX.Y.Z` 标签和独立发布流水线，不触发 CLI 自升级；其 GitHub Release 不设为 `latest`。GUI 首个原生版本为 0.7.0，承接已发布旧 GUI 0.6.0，不因 CLI 当前也是 0.7.0 而要求以后两端同步升级。`hangar-core` 与 `hangar-uniffi` 的 0.1.0 仅为内部 crate 版本，不是桌面发行版号。
- 用户决定本轮暂不公开发布原生 GUI：待 Linux 原生版在 Linux 主机完成并验收后，再协调 macOS/Linux 的发布时间。协调发布不等于共用一份安装包或由 macOS 构建证明 Linux 可用；macOS arm64/x64 DMG、Linux 对应产物仍需各自打包和验证。
- Xcode Debug/Release 的 `MARKETING_VERSION` 均为 0.7.0，`CURRENT_PROJECT_VERSION` 为符合 [Apple `CFBundleVersion` 数字分段格式](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/CoreFoundationKeys.html)的 1.7.0（首个原生代际为 1，后两段映射 GUI 的 minor/patch）；后续公开构建必须严格递增且不得把不同公开字节复用同一版本/构建号。计划中的 arm64 资产名为 `hangar-0.7.0-desktop-macos-arm64.dmg`；x64/universal 不能仅靠命名宣称支持。N25/N26 落地前不创建标签、DMG 或公开发行；安装替换、回退旧 0.6.0 DMG 与共享数据兼容性须在候选阶段单独验证。

## 5. Linux 阶段交接契约

Linux 阶段只在 Linux 主机开始，先读取本文已定共享语义，再根据 GTK4/Libadwaita 原生模式实现：

### 5.1 跨端行为基线

下表是未来 Linux（以及重新进入范围后的 Windows）的功能验收基线，不要求复制 SwiftUI 布局或 macOS 系统 API。业务事实以 `hangar-core` 为准；`hangar-uniffi` 的 DTO/任务接口是 macOS 适配，不是 Linux/Windows 必须照搬的 API。

| 行为 | 各端必须一致 | 平台可自行决定 |
|------|--------------|----------------|
| 数据身份 | 与 CLI 共用账号库、官方 Codex 配置路径；已有账号直接显示，不另建 GUI 库 | 配置路径诊断的原生呈现方式 |
| 总览 | 典型 7～8 个账号可直接比较；邮箱、当前/失效/推荐、周剩余、周重置时间、计划、重置卡数量及最早到期、AT/RT 健康可见；未知不显示 0% | 网格列数、卡片尺寸、滚动与视觉样式 |
| 排序 | 展示层当前优先、推荐其次、其余按可见周剩余降序（旧缓存仍可参与展示排序，但不能参与推荐）；未知和失效靠后，同值保留账号库顺序；整轮刷新后重排，不在逐卡刷新期间跳位；不改账号库存储顺序 | 原生列表/卡片容器；CLI/TUI 不必照搬 GUI 排序 |
| 配额与推荐 | 启动先显示共享 `quota-cache.json`；30 分钟有效期内不重复自动查询，到期串行刷新，手动强制刷新；失败保留旧值但标旧/失败且不进入推荐；仅新鲜、非失效、周剩余已知的账号参与 10 个百分点稳定推荐；绝不自动切号 | 前台/驻留计时器和系统唤醒事件接入 |
| 登录与切换 | 新增账号只入库、不自动切换；定向重新登录只更新该账号，非当前账号不得改官方 `auth.json`；切换必须显式触发并遵守 stale、凭据存储模式与 S11 守卫；成功后提示运行中的 Codex 重启 | OAuth 浏览器、手工回调、确认和错误提示的原生控件 |
| 删除与自检 | 当前账号不可删除；自检使用 Core 的路径、权限和一致性检查；错误只展示脱敏类别/提示，不输出凭据或 HTTP body | 删除确认框、自检详情页/弹窗 |
| 菜单与生命周期 | 若提供后台驻留入口，菜单与主窗口使用同一快照和操作守卫，不因展开菜单重复请求；关闭窗口、恢复窗口与退出进程需分别验收 | macOS MenuBarExtra/Dock 的具体策略不强加给 Linux/Windows；先核对桌面环境是否有可靠状态图标 |

实现入口：共享摘要与推荐见 `crates/hangar-core/src/{summary,recommendation,quota_cache}.rs`；账号写入/OAuth 见 `account.rs`、`login.rs`、`oauth.rs`；macOS 脱敏任务边界见 `crates/hangar-uniffi/src/{dto,service}.rs`。配额 TTL、失败与跨进程合并的细则见 [`quota-cache-refresh-plan.md`](quota-cache-refresh-plan.md)，写入风险见 [`scenarios.md`](scenarios.md) S11。Linux/Windows 不得从当前 macOS 真机记录推断自身已通过；Windows 仍未排入实现计划。

### 5.2 Linux 实施条件

- Rust GUI crate 直接依赖 `hangar-core`，不依赖 `hangar-uniffi`。
- 保持与 macOS 相同的账号卡片字段、推荐结果、错误类别和写入守卫；布局、Header Bar、菜单和后台驻留方式按 GNOME/Linux 原生惯例实现，不追求像素一致。
- 开始前确定目标发行版、GTK/Libadwaita 最低版本、Wayland/X11 范围和发行格式。
- 必须在真实图形会话验证窗口、键盘、输入法、托盘/状态图标可用性与退出行为；无头编译或 macOS 上的代码生成不能代替。
- 若目标桌面不保证标准托盘能力，应先记录支持矩阵和降级交互，再实现，不能照搬旧 egui 的 SNI 结论。

## 6. 分阶段任务清单

### Phase 0：决策与冻结

- [x] N01 确认平台原生路线：macOS SwiftUI + UniFFI，Linux GTK4/Libadwaita，Windows 延后
- [x] N02 确认仪表盘卡片布局、周额度唯一推荐指标与菜单栏/Dock 生命周期
- [x] N03 将宿主平台开发规则写入 Spec、执行协议、架构和 AI 导航
- [x] N04 宣布 egui 源码冻结：除迁移阻断或高风险安全缺陷外不再追加功能

### Phase 1：目录重构与旧 GUI 退场（当前 Mac 可执行）

- [x] N05 将 `crates/core` 迁为 `crates/hangar-core`，保持 crate 名和公开行为不变
- [x] N06 将 `crates/cli` 迁为 `apps/cli`，保持二进制名、版本和命令契约不变
- [x] N07 删除 `crates/gui`、egui/eframe/tray 依赖及旧 GUI CI/打包配置；保留 v0.6.0 发布文档与 Git 历史
- [x] N08 更新 workspace、脚本、CI、文档路径与 `.gitignore`（加入 `build/`、`dist/`）
- [x] N09 在隔离 HOME/CODEX_HOME 下回归 CLI/TUI，确认目录移动未改变数据路径和命令输出

### Phase 2：共享桌面语义与桥接（当前 Mac 可执行）

- [x] N10 将稳定推荐规则从 CLI 移入 Core，补齐当前账号阈值、未知与 stale 单测
- [x] N11 定义脱敏 `AccountSummary`、`QuotaSummary`、`TokenHealthSummary`、稳定错误码与任务状态
- [x] N12 建立 `crates/hangar-uniffi`，锁定 UniFFI 版本和绑定生成入口
- [x] N13 实现账号快照、顺序配额刷新、generation 过滤与取消/释放协议
- [x] N14 实现显式 OAuth 会话接口，验证取消、超时、state 错误和 S11 写入顺序
- [x] N15 从 Swift 调用一次真实 Core 只读接口，证明绑定、链接、架构和数据路径闭环

### Phase 3：macOS SwiftUI 应用（只在 macOS 主机执行）

- [x] N16 创建 `apps/macos` Xcode/SwiftUI 工程和可复现构建入口
- [x] N17 完成真实账号卡片仪表盘与 3/2/1 列自适应布局
- [x] N18 完成全账号周配额串行加载、单卡状态和稳定建议标记
- [x] N19 完成切换账号及 Codex 重启提示，验证官方 `auth.json` 与账号库一致
- [x] N20 完成 MenuBarExtra、关闭窗口、隐藏/恢复 Dock、显示窗口和真正退出
- [x] N21 完成添加/重新登录 OAuth、删除、自检与错误恢复
- [x] N22 完成键盘导航、深浅色、高对比、窄窗口及模拟唤醒验收；VoiceOver 听读与真实休眠唤醒按用户决定跳过，明确列为未验证例外
- [ ] N23 在 macOS arm64 构建 Release 候选并用真实隔离配置副本冒烟；x64 保持未验证，直到真实环境补证

### Phase 4：macOS 切换与发行

- [x] N24 明确最低 macOS 版本、应用标识、版本策略和 CLI/GUI 共存安装名
- [x] N25 配置 macOS 专属 CI：Core/绑定/App 编译测试；GitHub Actions 首次运行通过
- [ ] N26 完成签名、公证、DMG、最终 SHA/产物清单与回退说明
- [ ] N27 真机确认 7～8 个真实账号总览、刷新、推荐、切换、重登、菜单栏和 Dock 生命周期

### Phase 5：Linux 原生应用（切换到 Linux 主机后执行）

- [ ] N28 记录 Linux 发行版、架构、GTK4/Libadwaita、桌面会话和 Wayland/X11 基线
- [ ] N29 创建 `apps/linux` crate；直接依赖 Core，不使用 UniFFI
- [ ] N30 依据共享卡片语义实现 GTK4/Libadwaita 仪表盘和平台原生菜单
- [ ] N31 实现后台任务、generation 过滤、切换、OAuth、自检和 Linux 生命周期
- [ ] N32 在目标 Linux 桌面真机验证 7～8 账号、窗口、键盘/输入法、状态图标及退出
- [ ] N33 建立 Linux 专属 CI/打包/依赖基线并生成可追溯发行物

配额缓存与定期刷新按 [`quota-cache-refresh-plan.md`](quota-cache-refresh-plan.md) 执行；macOS 与 TUI 共享 Core 文件和有效期，Linux 阶段只接入既有契约。

## 7. 阶段门禁与回退

- Phase 1 只改变源码组织与移除未继续维护的 egui 源码；CLI/TUI 必须始终可构建、可运行。
- Phase 2 的桥接不得成为 Core 或 CLI 的反向依赖；删除 `hangar-uniffi` 后 CLI 仍应正常构建。
- Phase 3 每完成一个纵向能力都需有可运行应用：账号快照 → 配额 → 推荐 → 切换 → 生命周期 → OAuth，避免最后才发现桥接不可用。
- 删除 egui 不删除已发布的 0.6.0 资产；需要临时使用旧 GUI 时从已发布版本或 Git 历史获取，不在主干恢复双 GUI 维护。
- macOS 完成不代表 Linux 完成；Linux 阶段所有复选项保持未完成，直到 Linux 主机给出新鲜证据。

### Phase 1 验证记录（2026-09-22，macOS arm64）

- Cargo workspace 只包含 `crates/hangar-core` 与 `apps/cli`；依赖树不再包含 `hangar-gui`、eframe、egui、tray-icon 或 ksni。
- 按顺序执行 `cargo fmt`、`cargo clippy -- -D warnings`、`cargo test`，70 项测试全部通过。
- 使用 `mktemp` 创建隔离 `HOME`/`CODEX_HOME`，执行 `./target/debug/hangar list --json` 得到空账号结构化输出，`doctor` 报告的账号库路径位于隔离目录；未读写用户真实 HOME。
- 旧 GUI 源码、资源及 GUI CI/Release 工作流已删除；v0.6.0 发布资产和历史文档未删除。

### Phase 2 验证记录（2026-09-22，macOS arm64）

- 稳定推荐规则已迁入 Core；脱敏账号/周配额摘要、稳定错误码和任务状态由 Core 定义，CLI 与桥接层复用。
- `hangar-uniffi` 锁定 UniFFI 0.32.1；`scripts/generate-swift-bindings.sh` 可重复构建静态库并生成 Swift、C Header 与 modulemap。
- 全账号配额按账号库顺序串行刷新，刷新轮次使用 generation 隔离，取消和服务关闭均有终态测试。
- OAuth 新增显式会话状态机；PKCE secret 与换回的凭据留在 Core，桥接只提供 session id、授权 URL 和脱敏状态。错误 state、取消、超时及 1455/1457 端口白名单已测试；S11 顺序继续由账号层既有测试覆盖。
- `swiftc` 将生成的 `HangarCore.swift` 与 release 静态库链接为 arm64 冒烟程序，并真实调用 `bridgeInfo()`；N21 扩展契约后接口版本为 2。

### Phase 3 当前验证记录（2026-09-22，macOS arm64）

- 已建立 `apps/macos/HangarMac.xcodeproj`；Xcode Debug 构建会先生成 UniFFI 绑定，再产出 arm64 `Hangar.app`，`xcodebuild` 成功。
- 已实现 SwiftUI 自适应卡片网格、周剩余单指标、逐卡加载/失败/stale 状态以及 Core 稳定推荐标记；状态由 `@MainActor @Observable` 模型轮询脱敏快照。
- 开发版已使用真实账号库的隔离副本启动；用户确认账号卡片显示正常，N17–N18 验收完成。
- N19 已实现后台切换任务、重复操作守卫、Codex 运行检测、成功/失败提示和切换后总览刷新。`scripts/run-macos-qa.sh` 在应用退出后会用离线 doctor 检查隔离账号库 current 与隔离 `auth.json` 是否一致。
- 首轮 N19 隔离验收发现 Core 无条件调用 macOS `security`：临时 HOME 无登录钥匙串时弹错，但旧实现仍向 GUI 返回成功。修复改为读取顶层 `cli_auth_credentials_store`：默认/显式 `file` 只原子投影 `auth.json`，不访问钥匙串；`keyring`/`auto` 在刷新与写入前拒绝。QA 脚本显式固定 `file`。
- 2026-09-23 重新生成 UniFFI 绑定并完成 arm64 Xcode Debug 构建；随后以真实账号库隔离副本执行一次切换，脚本确认账号库 `current_account_id` 与隔离官方 `auth.json` 一致，退出码为 0，N19 验收完成。
- N20 将应用生命周期隔离到 `AppLifecycleController`：关闭最后一个主窗口后切换为 Accessory、保留 MenuBarExtra；菜单栏“显示 Hangar”恢复 Regular、Dock 与窗口，“退出 Hangar”才终止进程。菜单栏账号切换继续调用共享 `HangarModel`，未复制 Core 规则。
- 原生工程提供完整 macOS AppIcon Asset Catalog（16～1024 px），当前为黑底白色品牌标志；构建产物包含 `AppIcon.icns`，Info.plist 生成 `CFBundleIconName = AppIcon`。macOS 27 对这份旧式 AppIcon 自动添加浅色外框，故运行时 Dock 图标从独立的 1024px `HangarDockIcon` 图像直接设置；Finder/其他系统位置仍使用标准 AppIcon。菜单栏使用同一品牌标志的 18/36/54 px 单色模板 PNG：视觉与 Dock 图标一致，但遵循 macOS 状态栏不使用彩色方形底图的约定。
- 2026-09-23 在隔离真实账号副本上自动操作原生 UI：启动时 Dock 可见；关闭窗口后主窗口不可见、进程存活且 Dock 项消失；菜单栏仍可打开账号菜单；“显示 Hangar”恢复窗口与 Dock；“退出 Hangar”结束进程。N20 验收完成。
- N21 已实现添加/重新登录 OAuth Sheet、浏览器与手工回调、显式取消/释放、非当前账号确认删除及原生自检结果 Sheet；UniFFI 契约只传脱敏状态，自检与删除在 Swift 后台任务执行，凭据和 S11 提交继续留在 Core。
- N21 隔离验收发现 Debug App 曾错误链接 release Rust 静态库，导致 `HANGAR_TEST_HOME` 无效；已将 Xcode Debug/Release 分别绑定 Cargo debug/release 产物，并补全 Bundle Identifier/Executable。重新验收时自检路径明确位于临时目录，OAuth 启动→取消→再次启动成功，隔离账号删除后卡片由 6 张变为 5 张。
- N21 真实浏览器回调发现监听器曾用 `http://localhost + request.url()` 重建地址，丢失实际绑定的 1455/1457 端口，导致合法回调被白名单误判为 80 端口。现改为以会话自身的 `redirect_uri` 为基准解析请求目标，并补充 1457 回调重建回归测试；白名单、state、PKCE 与 S11 语义均未改变。
- 2026-09-23 在真实账号库隔离副本上完成浏览器 OAuth 回调后，发现复活路径仍沿用“复活并切换”的旧语义。现统一为登录与切换解耦：添加账号只入库；重新登录只原位更新凭据且不改变 `current_account_id`，非当前账号不触碰官方 `auth.json`，目标本来就是当前账号时才按 S11 同步新凭据。CLI/TUI/原生 GUI 提示同步修正，并增加 Core 回归测试。
- Debug QA App 使用真实账号库的隔离副本，不代表真实 HOME 已切换；界面现显示“隔离验证模式”横幅。推荐状态改为与“使用中”独立展示，当前账号被推荐时不再隐藏推荐标记。
- 主窗口使用标准 SwiftUI `WindowGroup + MenuBarExtra`：`WindowGroup` 负责窗口所有权、默认 1180×760 尺寸和原生 key/main 行为，`AppLifecycleController` 只负责 Regular/Accessory 策略切换、唤醒刷新与退出。QA 必须通过 LaunchServices 启动完整 `.app`，避免直接执行包内二进制造成 AppIcon、Asset Catalog 和窗口生命周期失真。完整 OAuth 成功回调与修复后的定向重新登录已分别在隔离副本上由用户完成，N21 已收口。
- 2026-09-23 重新验证：Debug `.app` 的主窗口 frame 为 1180×760；关闭后主窗口 `isVisible=false`、Dock 消失而 `MenuBarExtra` 保留；“显示 Hangar”后窗口恢复可见且 `canBecomeMain=true`、Dock 重现。AppIcon 资源、构建产物 `AppIcon.icns` 与运行进程解析出的蓝底图标像素均有效；用户已确认 Dock 和菜单栏图标均可见，菜单栏采用同品牌单色标志而不是彩色方形底图。
- N22 首轮证据：系统 AX 树确认卡片切换按钮包含账号邮箱、更多菜单包含账号上下文，工具栏包含名称与快捷键帮助；500×700 窗口下五个非当前账号主操作的 x 坐标均为 179，确认网格降为单列，随后恢复 1180×760；通过 `-AppleInterfaceStyle Dark` 启动后 `NSApp.effectiveAppearance` 为 `NSAppearanceNameDarkAqua`；⇧⌘D 打开自检并可用默认动作关闭；模拟 `NSWorkspace.didWakeNotification` 后工具栏先禁用、刷新终态后恢复。当时真实 VoiceOver 朗读、高对比系统设置和真实睡眠/唤醒尚未验证。
- 2026-09-23 N22 续验（macOS 27.0 arm64 / Xcode 27.0）：Debug App 使用 6 账号隔离副本，OAuth 和额度测试端点均指向本机不可用端口，本轮不执行切换。发现自检 Sheet 关闭后卡片“更多”菜单的 AX 名称曾退化为无账号上下文的“更多”；将无障碍名称放入 `Menu` 的 `Label` 后重新构建，六个菜单在 Sheet 前后均保持“更多账号操作，邮箱”。系统原有完整键盘控制为关闭；临时开启后，Tab 依序经过当前账号“更多”及其余卡片的“切换/更多”，未激活任何写操作；⇧⌘D 和默认关闭动作可用。系统深色→浅色及“增强对比度”开关均在真实 App 窗口观察到对应外观，文字和边界可辨；测试后已恢复原有深色、普通对比度和完整键盘控制关闭。此前 500×700 窄窗口证据仍有效。
- N22 范围调整：用户表示 VoiceOver 听读及真实休眠/唤醒不便确认，决定不做；相关表现（包括周剩余数值/推荐标记的实际朗读）保持“未验证”，不写成通过，也不再要求用户补测。N22 仅按已执行范围收口；后续发行说明须披露这项验证缺口，不能据此承诺完整读屏或真实休眠恢复。隔离 Debug App 已退出，临时账号副本已删除。
- 2026-09-23 展示排序改为卡片与状态栏菜单共享同一投影：当前优先、推荐其次、其余已知周剩余降序、未知、失效；当前账号即使失效也置顶以便重新登录。同组同额度保持账号库顺序，逐卡刷新时保持已有位置，终态统一重排。独立 Swift 排序测试与 macOS arm64 Debug App 构建通过；账号库、CLI/TUI 和 Core 推荐算法未改。
- 2026-09-23 排序真机验收：Debug App 使用真实 6 账号的隔离副本，主窗口无障碍树和截图均显示当前/推荐优先，其余按周剩余降序；用户确认状态栏菜单与窗口顺序一致，菜单无障碍树也独立核对。手动强制刷新期间卡片位置固定，整轮结束后同额度账号依原账号库顺序统一重排。隔离自检通过，账号库与官方认证文件一致。本次键盘焦点探测误点了隔离副本的切换按钮；真实账号库和官方认证文件的 SHA-256 与前次基线一致，测试 App 已退出，隔离凭据已删除。此误操作不计作只读验收或 N22 键盘导航通过。
- 2026-09-23 本机门禁重新按 `cargo fmt → cargo clippy -- -D warnings → cargo test` 执行通过（90 项测试），独立 Swift 排序测试与 Xcode 工程检查通过。macOS arm64 Release 配置可编译，产物 Bundle ID 为 `dev.mocika.hangar`、当前开发版本为 `0.1.0`；本地 ad-hoc 重签后 `codesign --verify --deep --strict` 通过，未做 Developer ID 签名或公证。N23 所需 Release App 隔离真实账号启动尚未完成，且 N24 版本策略、N25 原生 GUI 发布流水线/SHA 清单未定，故 N23 仍开放；不将此构建称为可发布候选。
- 2026-09-23 按用户要求重新编译并从完整 Debug `.app` 启动真实 6 账号隔离副本；用户在该副本上对非当前账号完成定向重新登录，界面提示当前账号未改变。只读核对确认隔离账号库的 `current_account_id` 与真实源一致，隔离官方 `auth.json` 字节未变，目标账号 `stale=false`；真实源账号库与官方认证文件的 SHA-256 前后相同。结合前述 OAuth 回调、取消/错误、删除与自检证据，N21 勾选完成。测试 App 暂留给用户查看，退出后由隔离目录清理任务删除临时凭据。
- 2026-09-23 N24：CLI 保持 0.7.0 与独立 `v*` 发布通道；macOS GUI 定为独立 0.7.0 / `macos-gui-v*`，Xcode Debug/Release 包内回读均为 Bundle ID `dev.mocika.hangar`、`CFBundleShortVersionString=0.7.0`、`CFBundleVersion=1.7.0`、最低系统声明 14.0，Mach-O 为 arm64、SDK 27.0。此前临时 `70000` 构建号不符合 Apple 的分段位数约束，已在发行前改正。macOS 14 真机、x64、旧 GUI 原位升级仍未验证。`cargo fmt → cargo clippy -- -D warnings → cargo test` 在隔离 HOME/CODEX_HOME 下通过（90 项测试）。
- 2026-09-23 N25 配置进度：新增 `.github/workflows/native-macos-ci.yml`，仅在 macOS runner 检查 Core/UniFFI、独立 Swift 排序测试、Debug/Release App 编译与包身份/架构回读；不修改 CLI `v*` 发布工作流，也不签名或上传资产。本机复演 Swift 测试、两种构建、Info.plist/架构断言及 YAML 解析通过；GitHub Actions 尚未触发，N25 保持开放。按 `release-check` 门禁，N23 在原生发布流水线与最终资产校验建立前不称为 Release 候选；N26 签名、公证、DMG/SHA 仍未做。
- 2026-09-23 推送 `b9e1667` 后，[macOS 原生 CI](https://github.com/mocikadev/hangar/actions/runs/35839411553) 首次运行成功：Core/UniFFI 的 fmt、clippy、隔离测试，Swift 排序测试，以及 Debug/Release App 构建和 Bundle ID/版本/最低系统/arm64 回读均通过，N25 收口。[CLI/Core CI](https://github.com/mocikadev/hangar/actions/runs/35839411536) 的 Linux、macOS、Windows 任务也全部成功。CI 不覆盖真实账号交互、旧 App 原位替换、macOS 14/x64、签名公证或 DMG/SHA；N23、N26、N27 仍开放。
- 2026-09-23 图标 QA：原亮蓝 AppIcon 的 SVG 与 16–1024px PNG 尺寸齐全，故无证据表明模糊源于缺少高清源。按用户反馈将共享 SVG 底色改为深海军蓝，`scripts/generate-macos-app-icons.sh` 从同一矢量源重新导出全部尺寸；本机 Debug App 重新构建成功。当前桌面截图接口只覆盖应用窗口，系统截屏命令因捕获权限失败，尚无可信 Dock 局部截图；深色图标的实际 Dock 清晰度仍待视觉确认，不标为已修复。
- 2026-09-23 图标边缘续查：原导出脚本对每个小尺寸直接栅格化 SVG；128px 图片中统计白色轮廓与深底色之间的过渡像素，直接渲染为 154 个，1024px 母图缩小为 292 个，后者的抗锯齿边缘更连续。脚本改为先渲染 1024px，再缩小生成其余九个尺寸。另移除启动时 `NSApp.applicationIconImage = NSWorkspace.shared.icon(forFile:)` 的手动覆盖：Apple 将 [`icon(forFile:)`](https://developer.apple.com/documentation/appkit/nsworkspace/icon%28forfile%3A%29) 的返回图初始尺寸定义为 32×32，而 [`applicationIconImage`](https://developer.apple.com/documentation/appkit/nsapplication/applicationiconimage) 用于临时替换 Dock 图标；本应用已有完整 Asset Catalog。Debug App 重新构建成功，实际 Dock 观感仍需隔离运行后确认。
- 2026-09-23 图标底色修正：用户指出前版并非黑底；确认 SVG 使用的是深海军蓝 `#0b1220`，现改为纯黑 `#000000`，仍保留白色标志和 1024px 母图缩小流程。原 Bundle ID `dev.mocika.hangar` 的 Debug App 包内 `AppIcon.icns` 已是黑色，但运行进程经 `NSRunningApplication.icon` 取得的仍是旧浅蓝 `(0,180,237)`；使用仅供隔离验证的 `dev.mocika.hangar.iconqa` 构建后，运行图标呈黑色主体和 macOS 渲染的浅灰外框。该证据支持旧 Bundle ID 的系统图标映射/缓存干扰预览；并非缺少黑底资源。真实 Dock 局部截图仍不可用，最终观感待用户目视确认，正式 Bundle ID 保持不变。
- 2026-09-23 白框定位与修正：对照 `NSRunningApplication.icon` 可见旧蓝图标同样被系统套浅色边框，黑底时对比更明显。将 SVG 改为不透明满幅黑底后白框仍在，且方角暴露，因此恢复圆角源图。用户确认将包内 `.icns` 直接赋给 `NSApp.applicationIconImage` 的隔离试验版不再显示白框；但该 `.icns` 经 AppKit 只提供到 256px，可能重新引入模糊。最终改为由同一 1024px 母图生成独立 `HangarDockIcon` 图像资产并在启动时赋给 Dock；独立 Bundle ID/隔离 HOME 的试验 App 由用户目视反馈“还可以”。正式 Bundle ID `dev.mocika.hangar` 的 Debug App 也构建成功，`Assets.car` 回读到 1024×1024 Dock 图像；源图与 Dock 图 SHA-256 一致。此变化仅影响运行时 Dock，不声称 Finder 等系统图标无白框。
- 同次 QA 的隔离启动最初显示空账号，但桌面自动化重新绑定时意外启动未继承隔离环境的 Debug App，短暂触及真实账号并更新 `quota-cache.json`；未执行切换/登录。发现后立即终止进程；真实 `accounts.json` 与其 `.bak` 字节相同，官方 `auth.json` 修改时间未变。不得将本轮称为全程隔离；后续绑定失效必须停止，不得自动重启 App。
- 2026-09-23 图标修复提交 `149d904` 已推送，[macOS 原生 CI](https://github.com/mocikadev/hangar/actions/runs/35843735583) 全绿：隔离 Rust 测试、Swift 排序测试及 Debug/Release App 编译均成功。本机隔离 HOME/CODEX_HOME 下重新执行 `cargo fmt → cargo clippy -- -D warnings → cargo test`，90 项测试通过；独立 Swift 排序测试通过。
- N23 部分进度：macOS 27.0 arm64 / Xcode 27.0 的 Release App 构建成功，包内 Bundle ID `dev.mocika.hangar`、版本 `0.7.0`/`1.7.0`、最低系统声明 14.0、Mach-O arm64；本地 ad-hoc 签名后 `codesign --verify --deep --strict` 通过，但 `spctl` 拒绝，未做 Developer ID 签名或公证。Release 不接受 Debug 专用的 `HANGAR_TEST_HOME` 与测试 OAuth/usage 端点；为避免隔离副本中的真实 RT 被服务端轮换，仅在禁止网络、禁止读取真实账号目录与写入真实 HOME 的 `sandbox-exec` 进程内运行 Release 可执行文件。隔离副本含 6 个真实账号、官方认证与配额缓存；进程环境指向临时 HOME/CODEX_HOME，主窗口在屏幕上，隔离账号库、认证文件与缓存和源文件字节相同。结束后已终止进程并删除临时凭据副本，真实源文件未改。该直接执行方式不等于通过 LaunchServices 启动完整 `.app`，Dock/菜单栏和卡片内容仍待该轮目视确认；原生 GUI 发布流水线、最终资产清单、x64/macOS 14 真机均未完成，N23 保持开放，不称可发布候选。

## 8. 完成定义

原生 GUI 迁移只有在以下条件分别满足后才可宣称对应平台完成：

1. 平台应用在匹配宿主构建并真实启动，至少完成一次 Core 调用。
2. 账号库与 CLI/TUI 使用同一数据身份和路径，已有账号无需重复登录即可显示。
3. 7～8 个账号可在总览中直接比较周剩余，未知/失败/stale 不伪装为 0%。
4. 推荐结果与 Core 单测一致，且不会自动切换。
5. 切换、刷新、OAuth 继续满足 S11、白名单、脱敏和原子写约束。
6. 关闭窗口、驻留、Dock/状态图标、恢复和退出在目标平台逐项实测。
7. 报告明确区分已构建、已运行、已交互验证和未验证平台/架构。
