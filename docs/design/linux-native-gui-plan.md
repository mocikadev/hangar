# Linux 原生 GUI 实施方案与任务

状态：本轮源码实施与无凭据验收完成，2026-09-23；完整平台验收仍有人工项。承接 N28–N33；用户已授权制定方案后实施；后续明确本轮仅完成实现与无凭据验收，真实账号留待用户手动验收。

## 范围与架构决策

新增 `apps/linux`（包 `hangar-linux`、命令 `hangar-desktop`），直接依赖 `hangar-core`。这是既定原生迁移的 L3 施工，不改变 Core、账号格式、OAuth 白名单、S11 或 macOS 桥接契约。GTK 对象只在主线程；凭据只存在后台 Core 调用中，UI 只接收脱敏快照。

比较：直接调用 Core 并在 Linux 应用编排任务符合既定依赖；复用 UniFFI service 会将 Linux 绑定到 Swift 适配；提取共享桌面 service 会扩大公共 API 并要求 macOS 联验。本轮选择第一项，以私有模块隔离编排，不复制推荐、缓存或写入规则。

模块职责：`model` 持有脱敏显示状态、稳定排序与 generation 守卫；`worker` 串行执行 Core 操作并发送结果；`login` 管理 Core OAuth 会话与一次性提交；`ui` 及卡片/对话框模块只负责 GTK 控件、焦点、动作和反馈。以有类型的命令/事件跨线程，业务 worker 使用标准库线程与通道；SNI 适配器的 ksni 依赖内部 Tokio 运行时（2026-10-09 新增），不在 GTK 主线程执行阻塞 API，不新增数据库或 FFI。

## N28：环境和兼容范围

2026-09-23 实测：Ubuntu 26.04.1 LTS x86_64、GNOME Shell 50.1、Wayland（DISPLAY=:0、WAYLAND_DISPLAY=wayland-0），Rust/Cargo 1.98.1，GTK 4.22.4、Libadwaita 1.9.1、GLib 2.88.0。pkg-config、构建工具、AT-SPI、Xvfb 可用。D-Bus StatusNotifierWatcher 存在，但不以可选扩展为应用运行前提。

首个实际支持目标为本机 Ubuntu 26.04 x64/Wayland；X11 单独冒烟，不由 Wayland 结果推断。API 基线选择 GTK >=4.14、Libadwaita >=1.5（原生自适应 Dialog/AlertDialog/ToolbarView），Rust 绑定采用同一兼容版本族并提交 Cargo.lock。API 下限不等于旧发行版或 glibc 可用性承诺。首个安装产物为本机动态链接 tar.gz；Flatpak、deb、其他桌面与 arm64 暂不承诺。

## GNOME 设计依据

2026-09-23 核查当前官方在线文档（HIG 无统一版号；Libadwaita main 是开发版，不能直接作为最低版本）：

- https://developer.gnome.org/hig/principles.html ：为人设计、简单、减少操作成本、避免打断。
- https://developer.gnome.org/hig/patterns/containers/header-bars.html ：左侧主要动作、中间标题、右侧菜单，少量控件并有 tooltip。
- https://developer.gnome.org/hig/guidelines/adaptive.html ：从窄窗口设计、保持全部功能、宽内容限制最大宽度。
- https://developer.gnome.org/hig/guidelines/accessibility.html ：键盘、语义名称、主题和缩放。
- https://gtk-rs.org/gtk4-rs/stable/latest/book/main_event_loop.html ：GTK 主线程与后台 I/O。
- https://docs.rs/libadwaita/ 、https://docs.rs/gtk4/ ：锁定版本 Rust API 核查。

界面：AdwApplicationWindow + ToolbarView + HeaderBar，单一账号总览，不增加无用途侧栏。卡片网格 1–3 列，默认约 1180×800，可垂直滚动；360px 窄窗口功能完整。逐卡加载、空状态 StatusPage、操作 Toast、重要失败 Banner/文字状态。使用系统字体、语义 CSS 类和符号图标；不覆盖系统配色。邮箱可省略但 tooltip/无障碍名称完整；状态不用颜色单独表达。

每卡包含邮箱、当前/推荐/失效、周剩余和重置时间、套餐、重置卡及最早到期；不展示 AT/RT 技术状态。未知显示“未知”，失败保留旧值并标注；5h 不进入 UI。当前优先、推荐其次、周剩余降序、未知、失效，同组稳定；刷新期间位置固定，终态重排。

## 任务与生命周期契约

- 启动后台读取共享缓存先显示，再 harvest 并按库顺序查询到期健康账号。30 分钟 TTL 与推荐调用 Core；失败重试冷却由本进程保存，手动刷新跳过冷却。
- 一个后台 worker 串行化刷新、切换、删除、自检和 OAuth 提交，UI 不等待文件/网络。通道事件带 generation；取消停止后续账号、丢弃迟到结果，但已开始的原子写/RT 提交必须完成，不能强杀线程。
- 同一轮只刷新一个账号；单卡失败继续其他账号；读取失败保持已有界面并明确报告。切换前 harvest，检测当前/失效状态，调用 checked API；检测 Codex 运行并提示重启。
- 添加和定向重新登录复用 OAuthLoginSession：浏览器打开、手工回调、等待、交换、取消、超时和提交。UI 不持有完整 Account；交换中不可取消已发出的请求，提交只执行一次。关闭/退出等待不可中断提交完成。
- 当前账号不能删除。非当前删除有原生确认（凭据不可可靠撤销，符合 HIG 例外），实际守卫仍由 Core 负责。
- Linux 提供可选 SNI 状态栏（2026-10-09 更新）：注册成功时关闭隐藏，状态栏丢失恢复窗口；不可用时关闭安全退出。最小化保留定时刷新，再次启动通过 GApplication 激活窗口。显式退出始终等待在途提交后结束进程，不创建独立常驻服务。
- 快捷键：Ctrl+R 刷新，Ctrl+N 添加，Ctrl+Shift+D 自检，Ctrl+Q 退出，Ctrl+W 关闭；Tab、Shift+Tab、Enter、Escape 使用原生控件语义。焦点不因逐卡刷新丢失。

## 实施清单

### N28 环境与设计
- [x] 读取接手说明、Spec、S11、缓存计划、测试策略与 rust-native-desktop。
- [x] 记录本机系统库、桌面、Rust 和状态图标探测。
- [x] 确定 UI、依赖方向、生命周期与兼容范围。
- [x] 锁定兼容的绑定版本并完成隔离 Core/CLI 基线。

### N29 骨架
- [x] 加入 workspace，默认成员保留 Core/CLI/桥接以免 macOS/CLI 被 GTK 系统库阻断。
- [x] 原生窗口、应用身份、空页、后台 Core 快照、单实例激活。
- [x] 本机隔离空 HOME 构建/启动，确认真实桌面窗口与退出。

### N30 总览
- [x] 脱敏模型、卡片全部字段、稳定排序与缓存新鲜度。
- [x] 逐卡状态、刷新期间位置稳定、终态重排、失败/未知/失效。
- [x] 系统主题、窄窗口与无障碍账号上下文实现；程序化焦点保留由图形测试验证，真实键盘/读屏留 N32。

### N31 操作
- [x] 串行 worker、generation、取消、自动/强制刷新与冷却。
- [x] 显式切换、Core 守卫、Codex 重启提示。
- [x] 添加/重登、手工回调、取消/超时/一次性提交。
- [x] 删除确认和 Core 自检。
- [x] 关闭/最小化/恢复/退出与在途任务收尾实现；原生关闭/退出无凭据验证，GNOME 启动器实际恢复留 N32。
- [x] 排序、事件过滤、取消、失败/冷却、脱敏与隔离文件行为自动化测试。

### N32 验收
- [x] 隔离 HOME/CODEX_HOME：fmt → clippy → test（含 Linux 包）。
- [x] 真实 Wayland：Debug/解包 Release 空窗口、单实例与退出；GTK/Xvfb：自检、OAuth 错误/取消/重启与原生关闭。
- [ ] Wayland 完整对话框交互与启动器恢复人工复验（外部 AT-SPI 子树不稳定，不能用 Xvfb 替代）。
- [ ] 窄窗口、浅/深色、高对比、键盘与输入法、读屏；逐项记录。
- [x] X11 范围单独记录：仅 Xvfb 布局与对话框无头证据，独立 X11 桌面未验收。
- [ ] 经单独授权，在隔离真实账号副本验收 7～8 卡片、缓存/刷新/推荐、切换、OAuth 成功和重登；不足账号或无法执行的项目保持开放。

### N33 构建与交付
- [x] Linux 专属 CI 配置，固定 Ubuntu 24.04，Core/CLI 保持独立（2026-10-09 远端运行通过，见 P06）。
- [x] 桌面入口、图标、安装/卸载说明，GUI 不覆盖 hangar CLI。
- [x] 从 Cargo 构建消息取真实产物，打包并回读 ELF、系统依赖、版本与 SHA，生成清单。
- [x] 本机包解压启动、单实例、退出和 SHA 回读；远端 CI 已通过；跨发行版完整桌面验收仍开放。
- [x] 同步 Spec、架构、接手说明、迁移计划、README 和验证记录。

## 回退与验收边界

无存储 schema 迁移。回退可移除 Linux 可执行文件/桌面入口并继续 CLI，不能通过替换二进制恢复服务端已轮换 token。禁止复活 egui 或创建 GUI tag/Release。测试一律隔离配置；真实凭据副本仍可能轮换服务端 RT，未经授权不复制或请求真实账号。构建/启动/交互/发行分别记证据，任务父项只在全部条件满足后勾选。

## 本轮验证入口

[Linux 验证记录](../quality/linux-native-gui-validation.md)记录命令、范围与未验项。`scripts/check-linux.sh` 运行完整隔离门禁；`scripts/test-linux-desktop.py` 执行空窗口/单实例/退出冒烟；`GDK_BACKEND=x11 xvfb-run -a scripts/test-linux-layout.sh` 执行无头 GTK 布局与对话框测试；`scripts/run-linux-qa.sh` 供人工查看空隔离窗口；`scripts/package-linux.py` 构建本地验证包。源码和无凭据交付不等于 N32 真实账号或 N33 公开发行验收完成（远端 CI 已单独通过）。

## 2026-10-09 桌面反馈审计后续

远端基线 `f8edcc7`。本批 L2：Linux worker 增加带 generation 的实际刷新计数事件；model 持有任务种类、进度与停止状态，busy 从任务存在性推导；GTK 在固定顶部区域显示任务说明及单个原生 Spinner。展示状态独立于账号业务，不改 Core、S11、存储、依赖或线程模型。

反馈延迟 500ms，避免快速的无请求自动检查闪烁；停止反馈立即显示，并等待当前请求及持久化完成。计数是本轮实际待刷新账号与已处理账号（含失败），不能以全部卡片数量代替，不估算剩余秒数。空任务不显示 0/0 进度。完成/失败/取消均清除忙碌状态，过期 generation 不能恢复已结束的反馈。日期继续使用 Core 的本地时间格式。

设计依据：2026-10-09 核查 [GNOME Spinner](https://developer.gnome.org/hig/patterns/feedback/spinners.html) 与 [Progress Bars](https://developer.gnome.org/hig/patterns/feedback/progress-bars.html)，页面未标修订日期。采用就近、单一、延迟显示的活动指示与明确子任务数量；不把账号处理比例伪装为剩余时间比例。

- [x] F01 明确范围、状态归属与验收标准。
- [x] F02 区分刷新、切换、删除、自检、添加/重新登录；发送实际刷新进度。
- [x] F03 实现固定顶部反馈、短任务延迟与停止等待提示。
- [x] F04 验证任务守卫、迟到进度、停止等待、空任务与 GUI 窄窗口/焦点；运行隔离门禁和桌面冒烟。
- [x] F05 记录新鲜证据与剩余人工验收项。
- [ ] M01（Mac 宿主后续）修正 `DashboardView` 使用全部卡片数量的刷新提示：优先改为不含数量的“正在刷新周额度…”，或由实际任务提供计数；覆盖 stale/有效缓存跳过场景，Mac 上构建与交互验收。本 Linux 批次不修改 macOS UI。

## 2026-10-09 主题与桌面功能补齐

承接用户授权的第二轮审计修复，L2：新增 Linux 私有 `tray` 适配器，使用锁定 ksni 0.3.6 的 blocking API（库内部管理 SNI 异步运行时），仅该前端依赖；独立后台线程启动/更新服务，GTK 仅传递脱敏菜单投影与接收动作。Core、UniFFI、账号结构和 S11 不变；不自写 SNI/DBusMenu 协议。

- 主题：显式声明 PreferLight（有系统深色偏好时跟随），无独立皮肤。人工 QA 保留真实桌面 session bus/portal，HOME/CODEX_HOME/XDG 配置数据缓存目录隔离；使用唯一 QA application ID，避免激活真实应用。私有 D-Bus 无头测试仍保留，不冒充系统主题验证。
- 登录：后台事件携带重登目标邮箱；对话框区分添加/重登。仅 Core 成功保存发出类型化提交成功事件；登录后强制刷新（与 Mac 相同），定向重登清除目标失败冷却；切换成功后按到期规则刷新；取消、超时、失败无补刷。
- 状态栏：当前账号、每账号周剩余/旧值/失效/推荐、快捷切换、刷新、显示窗口、退出。菜单读取现有快照，不因展开联网。current/stale/busy 守卫与主窗口一致，接收菜单动作后再次检查最新模型。
- 生命周期：仅 SNI 注册成功时关闭隐藏并继续刷新；注册未就绪/不可用时安全退出。入口离线后主动恢复隐藏窗口；再次在线恢复状态栏能力。Ctrl+W 走窗口关闭策略，Ctrl+Q/退出始终结束进程。隐藏状态错误恢复窗口。不安装桌面扩展、不设置开机启动。
- 唤醒：订阅 login1 PrepareForSleep(false) 后到期检查；忙时合并为一次延后检查，退出时不启动新任务；系统服务不可用则保留定时/激活检查。
- 图标：从共享 SVG/GResource 解码为 SNI ARGB pixmap，免安装也可显示品牌图标；GTK 对象只在主线程。

任务：
- [x] D01 修复主题 QA 会话，验证系统偏好与预览一致、动态主题信号。
- [x] D02 重登目标提示、类型化成功补刷、取消/失败不补刷。
- [x] D03 可选状态栏菜单、回调守卫及离线/重连恢复。
- [x] D04 区分关闭/退出，接入唤醒检查。
- [x] D05 隔离单测、GTK 图形测试、真实桌面 SNI/DBusMenu 和主题探测，同步使用说明和验收证据。

无凭据测试覆盖调度与展示，OAuth 成功/真实请求仍由用户手动验收。系统主题全局设置不由自动验收改写；使用隔离设置 portal 发信号验证动态响应，再只读比较真实主题。Mac M01、跨发行版、物理休眠、读屏验收不因此关闭。

D01–D05 本机无凭据验证已完成，证据见 [2026-10-09 主题与状态栏](../quality/linux-native-gui-validation.md#2026-10-09-主题与状态栏补齐验收)。新增 ksni API 以本机 Cargo registry 的 0.3.6 源码/示例核对；Settings portal 测试遵循 [官方协议](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html)，包括旧 Read 双层 variant 与 ReadOne 的区别。


## 2026-10-09 账号卡片内容对齐

用户确认：原生 GUI 不需要 AT/RT 展示；此要求替代早期总览中的令牌健康展示条目。Core/桥接与 CLI/TUI 健康规则不变。

架构判断 L2：Linux 卡片负责 GTK 控件与动作；私有 `card::presentation` 负责无 I/O 的状态文案及日期展示，便于独立验证缺字段分支。已有 Activity 增加可选目标账号 ID，生命周期仍由 UI model 唯一拥有；UI 从已创建的类型化命令记录目标，卡片只接收目标操作，不增加线程、依赖、持久化或业务规则。沿用现有 worker/generation 和全局 busy 守卫。

展示顺序：邮箱与套餐 → 当前/推荐标记 → 周剩余与进度条 → 周重置、重置卡数量及到期时间 → 缓存状态 → 操作。当前卡片使用系统强调色边框和带图标的“使用中”，不再用禁用按钮；推荐使用符号及语义强调色。无标记不显示“可用账号”。

状态契约：
- 首次加载/失败与带缓存刷新/失败分别提示；无成功时间不显示“更新于 未知”。
- stale 收起额度详情，只保留明确的重新登录提示和主操作；不展示失效缓存占位。
- 周额度未知不隐藏独立已知的重置卡信息。0 张明确显示“重置卡 0 张 / 暂无可用重置卡”；正数无到期时间与数量未知分别提示。
- 日期仍通过 Core `fmt_ts_local` 获得本地时间，只在卡片格式化为“yyyy年M月d日 HH:mm”，不改变 CLI 或 Core 时区计算。
- 更多菜单隐藏当前账号删除、stale 的重复重登；没有可用菜单项时隐藏入口；忙时保留菜单焦点，只禁用内部操作。切换/删除/重登在目标卡片显示操作文字，完成或失败立即清除；全局保留唯一延迟 Spinner，避免重复动效。

2026-10-09 核查 [GNOME 原则](https://developer.gnome.org/hig/principles.html) 与 [Spinner 指南](https://developer.gnome.org/hig/patterns/feedback/spinners.html)，页面未标修订日期。沿用 GTK 4.14 / Libadwaita 1.5 项目最低 API，不为样式增加依赖。

- [x] C01 更新 Spec、跨端卡片契约和本计划，移除 GUI 的 AT/RT 展示要求。
- [x] C02 调整 Linux 卡片层级、当前/推荐强调、日期与操作入口。
- [x] C03 修复空数据/旧数据/失败/stale/重置卡缺字段文案，增加目标账号操作反馈。
- [x] C04 隔离门禁、状态回归、8 卡片宽窄/浅深色渲染及 Wayland 启动冒烟，记录证据。
- [ ] M02（Mac 宿主后续）周剩余未知时继续显示独立已知的重置卡信息；Mac 上实现、构建和交互验收。本轮 Linux 不修改 macOS UI；macOS 原本未显示 AT/RT，无需为此改代码。


### C05 顶部空间精简（2026-10-09）

用户反馈账号数量和当前邮箱摘要重复占用空间。L1 展示调整：移除 Linux 总览顶部摘要控件及对应渲染代码，卡片区直接承接工具栏；当前账号仍由卡片标记与边框表达，任务反馈、错误 Banner 和空状态保留。无模型、持久化或操作语义变化，使用现有隔离门禁与布局回归验证，不为删除静态展示增加同构单测。

- [x] C05 移除冗余摘要，验证宽窄卡片布局并重启已授权的真实账号预览。


## 2026-10-09 Linux 构建与打包收口

用户最终范围：UI 暂时冻结，仅为个人 Ubuntu 使用提供 deb；不提供 AppImage 或 tar.gz。渠道为 Linux 原生 GUI 本地/CI 验证产物，版本取 `apps/linux/Cargo.toml`（当前 0.7.0），不修改 CLI `v*` 发布通道，不创建标签或发布 Release。此前 tar.gz 属历史验证产物。

架构判断 L2：`package-linux.py` 负责原生构建、deb 装配和产物清单；`verify-linux-package.py` 负责最终资产、解包/版本/独立 dpkg 数据库登记移除验证及可选隔离 GUI 冒烟。deb 由 apt/dpkg 安装卸载，不另写安装器；无新增运行依赖或 Core/UI 修改。

- Cargo JSON 获取实际可执行文件，显式指定本机 GNU Linux target，校验版本、ELF 架构、动态依赖与 desktop 文件。x64/amd64、arm64/arm64 映射不表示已经验证 arm64。
- 使用 `hangar-desktop` 独立包名，安装到 `/usr`，不覆盖 CLI；`dpkg-shlibdeps` 自动生成依赖，附加 GTK >=4.14 / Libadwaita >=1.5 API 下限。deb 无安装脚本，不启动应用或写用户 HOME。
- manifest schema 2 记录 target/架构、构建系统/库、Git commit/dirty、Release profile、依赖、资产字节与 SHA、未签名/验证边界；外部 SHA256SUMS 覆盖 deb 与 manifest。唯一 build-id 防覆盖，暂存位于 build/staging。
- 不设置 GTK_THEME/GDK_BACKEND，继续跟随系统主题并支持现有 Wayland 行为。
- CI 固定 Ubuntu 24.04 x64，保留门禁与图形回归，增加 deb 检查、独立 dpkg 数据库登记/移除与解包 Release 冒烟；临时 runner 用 apt 安装/卸载验证真实依赖解析，上传 deb/manifest/SHA，无发布权限。
- 本机 Ubuntu 26.04 产物记录真实依赖，不声称在 Ubuntu 24.04 可用；兼容基线由对应 CI 构建/验收决定。

官方参考（2026-10-09 核查）：[Debian control](https://www.debian.org/doc/debian-policy/ch-controlfields.html)、[dpkg-shlibdeps](https://manpages.debian.org/trixie/dpkg-dev/dpkg-shlibdeps.1.en.html)、[GitHub artifacts](https://docs.github.com/en/actions/tutorials/store-and-share-data)。

- [x] P01 审计并按用户最终选择限定为 deb。
- [x] P02 deb 构建、依赖与架构校验、完整产物清单和 SHA。
- [x] P03 deb 隔离解包、登记/移除与内容回归。
- [x] P04 CI 接入资产验收、Release 冒烟与 runner 安装回归。
- [x] P05 本机实际打包、隔离启动验收，文档与证据同步。
- [x] P06 远端 Ubuntu 24.04 CI 实际通过：[运行 37884056743](https://github.com/mocikadev/hangar/actions/runs/37884056743)，包含 deb 构建/隔离启动/apt 安装及卸载；下载产物 SHA/来源与本机 Wayland 复验通过。

后续用户已授权并完成本机 apt 安装及远端 CI，证据见 Linux 验证记录“本机安装与远端 CI”。其他 CPU/发行版完整桌面、正式签名与公开发布保持独立验收。
