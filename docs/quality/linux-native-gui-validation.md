# Linux 原生 GUI 验证记录

日期：2026-09-23。对应 [实施方案](../design/linux-native-gui-plan.md)。用户明确选择本轮仅做实现与无凭据验收，真实账号留待用户手动验收。本轮不读取/复制真实账号，不进行真实登录、额度请求或切换，不安装、不推送、不公开发布。

## 宿主与实现

- Ubuntu 26.04.1 LTS / x86_64，GNOME Shell 50.1 / Wayland。
- Rust/Cargo 1.98.1；GTK 4.22.4 / Libadwaita 1.9.1 / GLib 2.88.0。
- Rust 绑定锁定 gtk4 0.11.5 / libadwaita 0.9.2；使用 GTK 4.14 / Adwaita 1.5 API 基线。该下限不是 Ubuntu 24.04 或其他发行版真机兼容结论。
- 新增 `apps/linux`，Core/CLI/macOS 业务源码未改。默认 workspace 成员保持原三包；Linux 独立构建和验收。
- GTK 主线程仅显示摘要；一个标准库 worker 串行执行 Core 调用；OAuth 凭据与提交留在后台。退出等待在途操作完成，不强杀凭据写入。

## 自动化证据

日志为本机生成文件（不提交）：`build/linux-qa/`。

| 项目 | 结果与范围 |
|---|---|
| 开始前基线 | 空隔离 HOME/CODEX_HOME 下 `cargo test --workspace`，原有 90 项测试通过 |
| 完整门禁 | `scripts/check-linux.sh`：fmt → clippy（workspace/all-targets/-D warnings）→ test，96 项通过，1 项图形测试默认 ignored |
| 新增非图形测试 | 当前/推荐/失效排序、同额稳定与终态重排、旧 generation 过滤、重复操作守卫、失败冷却、失败旧值保留/退出推荐、失效隐藏缓存；worker 空库刷新/自检/OAuth 错误回调/取消/退出 |
| 脱敏边界 | UI 模型使用 Core AccountSummary/QuotaSummary，worker 事件不传 Account；展示测试仅构造无凭据摘要，未制造 token/有效期或真实接口成功 |
| CLI 隔离 | `cargo tree -p hangar -e normal,build --locked` 不含 gtk/adwaita/uniffi |
| 静态入口 | desktop-file-validate、Shell 语法、Python AST 检查；图标嵌入 GResource，安装目录另带同源 SVG |

## 图形与生命周期

使用空隔离 HOME/CODEX_HOME、独立 D-Bus 和运行目录；`scripts/test-linux-desktop.py` 可复跑。只有该脚本自己的子进程可被清理，绝不重绑或重启真实配置应用。QA 的 portal 在独立运行目录可能产生 FUSE doc 挂载；退出时先卸载该临时挂载再删除目录。

已执行的覆盖：

- **真实 Wayland / Debug 与解包 Release**：空账号窗口启动，AT-SPI 树可见；第二次启动不创建第二个窗口；显式退出结束进程。最终可复跑脚本为 `scripts/test-linux-desktop.py`，应用自身日志为空。
- **GTK / Xvfb（无头，不能替代 Wayland 真机交互）**：`GDK_BACKEND=x11 xvfb-run -a scripts/test-linux-layout.sh` 单独通过 1 项 ignored 图形测试。覆盖 8 张纯摘要夹具的宽/窄布局、浅/深色、更新后焦点保留、真实空 HOME 自检对话框、OAuth 错误回调提示保持、取消后再次启动、原生关闭请求及 worker 安全退出。仅测试进程关闭动画，未改系统设置。
- 新增回归确认 OAuth 等待阶段只发送状态变化，不会每 100ms 覆盖浏览器打开失败或错误回调提示；无效回调反馈显示在对话框内。
- 早期 Wayland 探测观察过完整自检/登录子树，但反复运行后，部分对话框 mapped=true、分配为 0×0，外部 AT-SPI 子树不完整，相关自动化不稳定。保留该限制；最终对话框证据按 Xvfb 记录，不宣称 Wayland 读屏或完整交互通过。隔离 D-Bus 的 portal/PipeWire 警告也不算完整桌面集成通过。
- GTK 标准单实例激活已接入；真实最小化后从 GNOME 启动器恢复保持待手动验收。调用 `unminimize/present` 不等于恢复完成，最终由窗口系统决定（[GTK 文档](https://docs.gtk.org/gtk4/method.Window.unminimize.html)）。

截图：`build/linux-qa/layout-{wide,narrow,dark}.png` 与 `dialog-{doctor,oauth}.png`。最终截图来自 Xvfb，均已视觉检查。账号卡片是无凭据展示夹具，不是 8 个真实账号或配额成功路径；对话框使用真实 Core 空配置。真实键盘/输入法/读屏、高对比与睡眠唤醒不由截图推断。

## 本地包与 CI

`scripts/package-linux.py` 从 Cargo JSON 消息提取可执行文件，生成 tar.gz、SHA256SUMS.txt 与 manifest.json。清单包含 Git 提交/dirty 状态、宿主/系统库、Rust、ELF、ldd、glibc 符号要求及最终字节摘要。执行过 SHA 验证、解包和 `--version` 回读，动态库无缺失。

最终本轮包位于 `dist/linux/20260923T111113Z/hangar-0.7.0-desktop-linux-x64-gnu.tar.gz`（1,125,283 bytes），SHA-256 为 `b4a59e958527a9087adc49863dec7d8b9cad14b6e6321c3b2468b27e879a2706`；对应目录提供 manifest 和 SHA256SUMS。解包后再次完成 Wayland 空窗口/单实例/退出检查。该包是 Ubuntu 26.04 x64 动态链接验证包，非跨发行版通用包，不替换 CLI。确切路径与摘要见本轮最终构建清单；重新打包以新 build-id 输出，不覆盖此前验证产物。

`.github/workflows/native-linux-ci.yml` 固定 Ubuntu 24.04，定义依赖安装、隔离门禁、桌面入口校验、构建打包和校验清单上传。当前仅配置完成，未推送或触发 GitHub Actions，不能标记远端通过；不生成 Release/标签，不改 CLI `v*` 发布通道。

## 尚未验收（保持开放）

- 用户手动真实账号：7～8 账号总览、缓存/刷新/推荐、切换、OAuth 成功和定向重登、删除。
- 真实键盘/输入法/读屏、高对比、睡眠恢复、最小化后从 GNOME 启动器恢复。
- 独立 X11 桌面、其他桌面/发行版/CPU。Xwayland 或 Xvfb 若单独通过，也只记录其后端/无头范围。
- 远端 Linux CI、Ubuntu 24.04 实机、正式安装升级/回退及公开发布。

N28/N29 可关闭；N30/N31 已有源码和无凭据证据，但真实链路仍开放；N32/N33 父任务不提前勾选。本轮交付范围按用户选择已排除真实账号验收。

## 2026-10-09 远端合并后的任务反馈验收

基线 `f8edcc7` + 本地 Linux 实现。宿主 Ubuntu 26.04.1 x86_64，GTK 4.22.4 / Libadwaita 1.9.1；本批无新依赖。任务方案见 Linux 实施方案 F01–F05，Mac 的 M01 未执行。

- `scripts/check-linux.sh`：隔离 HOME/CODEX_HOME，依次 fmt、workspace/all-targets clippy、workspace test；103 项通过，1 项 GTK 测试默认忽略。新增覆盖短任务不闪烁、停止立即显示且不被进度覆盖、单调计数、不同任务文案、终态/旧 generation 进度拒绝、实际待请求列表排除 stale/新鲜缓存/冷却（手动绕过后两项）。计划筛选测试只用内存中的空凭据，不代表成功联网验收。
- `GDK_BACKEND=x11 xvfb-run -a scripts/test-linux-layout.sh`：单独 GTK 测试通过，涵盖反馈显示/停止按钮/失败清理/迟到事件、窄窗口换行、更新不移动焦点，及原有浅深色布局、自检、OAuth 无效回调/取消、关闭流程。首次新增截图在文字变更后尚未完成布局时没有 render node；补充主循环布局等待后通过。
- `cargo build --locked -p hangar-linux`：Debug 构建通过。`GDK_BACKEND=wayland /usr/bin/python3 scripts/test-linux-desktop.py`：空隔离窗口、单实例、显式退出通过。未复制或调用真实账号。
- 目视检查 `build/linux-qa/feedback-refresh.png`、`feedback-stopping.png`：360px 窄窗口任务说明可见，停止提示正常换行；这里的 8 卡片和 2/3 进度是纯展示事件夹具。
- 日志保存在 `build/linux-qa/feedback-{check,layout,desktop,build}.log`（忽略文件）。私有会话仍有 portal/PipeWire、Xvfb DRI3、AT-SPI 环境警告；不能据此宣称完整读屏或桌面集成通过。

本批未重打 Release 包；9 月 23 日旧 tar.gz 不包含这些修改。真实服务的长请求/失败/停止、物理键盘与读屏、Wayland 完整对话框交互保持人工验收项；macOS UI 受宿主门禁保持未改。

## 2026-10-09 主题与状态栏补齐验收

基线 `f8edcc7` + 本地 Linux 工作；同一 Ubuntu 26.04.1 x86_64、GTK 4.22.4 / Libadwaita 1.9.1、Wayland 宿主。本轮只修改 Linux UI/适配器、QA 脚本、Linux CI 和文档；Core、UniFFI、macOS UI 未改。ksni 0.3.6 及其内部异步依赖仅加入 Linux 包；Core 普通/构建依赖树未包含 ksni、GTK 或 Tokio。

- `scripts/check-linux.sh`：隔离 HOME/CODEX_HOME，fmt → workspace/all-targets clippy → workspace test；106 项通过，1 项 GTK 测试默认忽略。新测试验证添加/目标重登文案、菜单 current/stale/busy 守卫、未知额度、打开菜单无动作、旧回调重新核对状态。
- `GDK_BACKEND=x11 xvfb-run -a scripts/test-linux-layout.sh`：1 项通过；保留既有布局/对话框/反馈测试，新增可选状态栏隐藏与离线恢复的展示事件测试、成功提交后补刷调度、取消不补刷、唤醒忙时延后、自动检查不清除错误提示。注入完成事件只验证调度，不冒充真实凭据保存。
- `cargo build --locked -p hangar-linux`：Debug 构建通过。`/usr/bin/python3 scripts/test-linux-integration.py` 在私有 D-Bus 和空配置中运行实际 GTK 应用及测试 SNI 宿主：回读 32×32 ARGB 品牌图标、菜单、关闭隐藏、显示窗口、刷新、退出、宿主离线恢复窗口/重连重新注册均通过。
- 同脚本 `--real-session` 在真实桌面 bus、唯一 QA application ID、空 HOME/CODEX_HOME/XDG 配置目录运行：从真实 watcher 按 D-Bus peer PID 找到本次应用，图标与 DBusMenu 回读、关闭隐藏、菜单恢复、刷新、退出通过。应用日志为空。此检查调用的是实际菜单协议，不包含物理鼠标展开顶部图标的目视验收。
- `/usr/bin/python3 scripts/test-linux-theme.py`：私有 Settings portal 发出 dark → light → dark → default，Libadwaita PreferLight 探针动态跟随通过；`--real-session` 只读系统偏好得到 scheme=1（深色），在空 QA 配置中得到 dark=True。未改写用户系统主题。探针验证工具包策略与 portal 传递，不等于全应用人工切换主题的视觉验收。
- 首次主题测试夹具提前加载 Adw 导致真实 portal 抢占私有 bus 名称，随后发现旧 Read 需要双层 variant；按官方协议修正测试服务后通过，未通过的测试没有算作证据。日志仍有测试脚本 Gio 绑定弃用提示及私有会话 AT-SPI/portal 环境警告，不是应用崩溃。
- Linux CI 已加入 Xvfb 布局、私有 SNI/主题协议测试与所需系统测试依赖；尚未推送或远端运行。日志位于 `build/linux-qa/desktop-fixes-{check,layout,build,sni-private,sni-real,theme}.log`。

真实 OAuth 成功、登录后额度网络请求、真实账号切换、物理休眠/唤醒、实际读屏仍未验证。没有复制/刷新真实凭据，没有安装扩展、开机启动、安装覆盖或公开发布。旧 Release 包未重建；使用 `scripts/run-linux-qa.sh` 构建并查看最新空隔离版本。状态栏可用时关闭只隐藏，必须从菜单退出或 Ctrl+Q 结束进程并清理临时目录。


## 2026-10-09 卡片内容对齐验收

用户确认 GUI 不显示 AT/RT；已同步 Spec、迁移计划、Linux 接手说明与实施任务 C01–C04。宿主 Ubuntu 26.04.1 x86_64，GTK 4.22.4 / Libadwaita 1.9.1。本轮仅 Linux 展示层及文档变更，无 Core、UniFFI、账号 schema 或依赖变动。Mac 独立重置卡信息显示问题登记为 M02，未在 Linux 修改 SwiftUI。

- 基线 `scripts/check-linux.sh`：原有 106 项测试通过。最终同命令依次 fmt → workspace/all-targets clippy → test：109 项通过，1 项 GTK 测试默认 ignored；新增 3 项卡片文案/缺字段/日期纯函数测试。
- `GDK_BACKEND=x11 xvfb-run -a scripts/test-linux-layout.sh`：1 项 GTK 聚合测试通过（13.66s），覆盖当前/推荐/stale、菜单显示与忙碌守卫、目标卡片操作反馈及完成清理、焦点保持、8 张展示夹具的宽窄布局和深浅色；不代表 8 个真实账号的接口成功或整屏同时容纳验收。
- 修正验证中发现的焦点问题：忙时保留“更多”菜单可聚焦，仅禁用内部操作。第一轮测试误用含祖先可见性的 `is_visible` 检查未展开 popover 内部菜单项，改为检查项目自身 visible 属性；未通过的轮次不计入最终证据。
- 已目视检查 `layout-wide.png`、`layout-dark.png`、`card-states-light.png`：当前账号边框/文字可辨，配额行对齐，stale 收起额度，周剩余未知仍显示独立重置卡，日期使用 Core 本地时间后转换为中文卡片格式。额外输出 `card-states-dark.png`、`card-switching.png`。均为无凭据展示夹具；自动测试不改真实桌面主题。
- `cargo build --locked -p hangar-linux` 通过；`GDK_BACKEND=wayland /usr/bin/python3 scripts/test-linux-desktop.py` 空隔离配置的启动、单实例、显式退出通过。应用本身无错误；私有会话 portal/AT-SPI 和 Xvfb DRI3 环境警告不作为完整桌面或读屏通过依据。
- 延续用户此前“使用当前机器已配置真实账号、无需隔离”的明确授权，门禁结束后通过应用 quit 动作正常退出旧预览，再启动最新 Debug 使用正常配置，供人工验收；自动回归仍全部隔离。不自动执行切换、删除、重登等人工验收操作，不以此次启动宣称真实凭据链路验收通过。

日志：`build/linux-qa/card-{baseline,check,layout,build,desktop}.log`。没有重建 Release 包、提交、推送或发布；物理键盘/读屏、真实账号成功操作与 Mac M01/M02 仍待对应环境验收。


### C05 顶部摘要移除

移除重复的“账号数量 · 当前邮箱”一行和对应 UI 字段/渲染代码。`scripts/check-linux.sh` 的 fmt、clippy、109 项测试通过；GTK 聚合图形测试单独通过，宽窄布局截图已更新，目视确认卡片区上移且当前账号仍有卡片标记。Debug 构建通过，使用应用正常退出动作替换此前已授权的真实账号预览。未新增真实账号操作或改变刷新规则。日志 `build/linux-qa/header-{check,layout,build}.log`。


## 2026-10-09 个人 deb 构建验收

按用户最终要求只提供 deb，不提供 AppImage/tar.gz。本轮未修改 Rust UI、Core、账号库或正在运行的真实账号预览；`hangar-desktop` 与 CLI 分开。方案 P01–P05 已完成配置/本机验证；P06 远端 CI 保持开放。

- 宿主 Ubuntu 26.04.1 x86_64，Rust 1.98.1，GTK 4.22.4 / Libadwaita 1.9.1。`scripts/check-linux.sh` 按 fmt → clippy → workspace test 通过，109 项测试通过，1 项 GTK 布局默认忽略（本批未改 UI，不重复布局）。
- Python 包校验回归 5 项通过，覆盖文件/manifest 篡改、缺失/重复 SHA、目录越界、大小不匹配。Python AST 与 workflow YAML 解析通过；未声称远端 Action 执行通过。
- `scripts/package-linux.py` 显式原生 target `x86_64-unknown-linux-gnu` Release 构建通过，Cargo JSON 获取真实产物；desktop、ELF、版本、动态库已回读。固定文件权限/root 包属主，无 maintainer scripts，不设置主题/显示后端或启动程序。
- `dpkg-shlibdeps` 自动依赖加 GTK/Libadwaita API 下限。初次装配缺少根 LICENSE，已按项目 Cargo 的 MIT 声明加入 Debian copyright 元数据；调整 DEBIAN 目录建立顺序后，最终构建无 shlibdeps 警告。早期失败或试包不作为最终产物。
- `scripts/verify-linux-package.py <目录> --smoke` 通过：SHA 覆盖 deb+manifest、文件白名单/权限/属主、control 版本/架构/依赖、解包版本/ldd、desktop 入口；独立临时 dpkg 数据库完成解包、重复解包和 purge，CLI/账号目录标记保持原样。本机未运行 sudo apt 安装，也没有通过忽略依赖强行安装。
- 解包 Release 在真实 Wayland 会话、私有 D-Bus 和空 HOME/CODEX_HOME 中通过启动、单实例与显式退出；未访问真实账号。私有会话仍有 portal/AT-SPI 环境警告，应用日志无错误。`apt-get --simulate install <deb>` 在本机成功，只计划新增 hangar-desktop；模拟不等于系统安装验收。
- Linux CI 固定 Ubuntu 24.04，增加包校验、Xvfb 解包 Release 冒烟、临时 runner apt 安装/purge、deb/manifest/SHA 及验证证据上传。尚未推送或远端运行，不把本机包推定为所有发行版/CPU 通用。

最终产物：`dist/linux/20261009T041645719774Z/hangar-desktop_0.7.0-1_amd64.deb`（1,402,330 bytes），SHA-256 `8d137296db451430c5479d76031b3a4f2ed43aac7311e3114f10c3d22be46b3c`。旁附 manifest schema 2 与 SHA256SUMS。构建为未签名、本地未提交源码（dirty）候选，不是公开 Release；本次未安装、提交、推送或发布。

日志：`build/linux-qa/package-{baseline,check,tests,build,verify,apt-simulate}.log`；机器可读验收记录 `build/linux-qa/package-20261009T041645719774Z.json`。正式宿主安装、远端 CI、跨发行版/arm64、真实账号成功链路仍按各自范围验收。


## 2026-10-09 本机安装与远端 CI

用户明确授权本机安装及远端 CI，包含为运行 CI 所需的提交和推送。已将此前 Linux 源码/验证脚本/打包配置一起提交为 `c01aeb7` 并推送到 main；未打标签或发布 Release。

本机 `sudo apt-get install` 安装 `hangar-desktop 0.7.0-1 amd64` 成功，之前未安装该包，无旧安装需要备份。安装文件来自上一节 SHA 为 `8d137296db451430c5479d76031b3a4f2ed43aac7311e3114f10c3d22be46b3c` 的 deb；已回读 dpkg `install ok installed`、程序版本与 desktop 校验，并比较安装后二进制和 deb 内容一致。包管理器完成 desktop、图标和 GNOME 菜单更新触发器。可用 `sudo apt remove hangar-desktop` 撤销安装，账号数据和 CLI 保留。

`/usr/bin/hangar-desktop` 在空 HOME/CODEX_HOME、私有 D-Bus、真实 Wayland 会话中通过启动/单实例/退出。随后使用真实桌面入口启动，D-Bus peer 的 `/proc/PID/exe` 确认为 `/usr/bin/hangar-desktop`，正常账号配置延续此前用户授权。此步骤不代表所有真实账号操作已验收；自动测试仍一律隔离。

推送前隔离门禁：109 项 Rust 测试及 5 项包校验测试通过。日志保存在 `build/linux-qa/local-install.log`、`installed-smoke.log`、`installed-launcher.log` 和 `prepush-*.log`。远端运行与产物回读结果续记于下方。


远端首次 Linux 运行 [37883512876](https://github.com/mocikadev/hangar/actions/runs/37883512876) 的格式/lint/单测与 GTK 布局通过，但 SNI 集成测试因 Ubuntu 24.04 runner 缺 `at-spi2-core` 而失败，随后异常清理遇到残留进程写入缓存。修复提交 `9fa4ccf` 显式安装 AT-SPI/fuse3，测试启动前验证无障碍服务，并在父进程最终清理中终止仅属于本次 QA 的独立进程组。没有修改应用 UI 或凭据逻辑。

修复后本机 Xvfb 私有 SNI 全流程与 109 项 Rust 测试通过；额外在私有 D-Bus 隐藏服务目录，验证缺 AT-SPI 时在创建应用前失败且临时目录清理成功。日志 `ci-fix-integration.log`、`ci-fix-check.log`、`no-a11y-service.log` 保存在 build/linux-qa。

同一应用源码提交 `c01aeb7` 的 [CLI/Core CI](https://github.com/mocikadev/hangar/actions/runs/37883512840) 与 [macOS CI](https://github.com/mocikadev/hangar/actions/runs/37883512858) 均通过；后者是 CI 构建证据，不替代 Mac 实机 UI 验收。


最终 Linux [运行 37884056743](https://github.com/mocikadev/hangar/actions/runs/37884056743) 全部通过，源码为 `9fa4ccf`：Ubuntu 24.04.5 x86_64、GTK 4.14.5 / Libadwaita 1.5.0 / GLib 2.80.0，格式/lint/隔离测试、GTK/SNI/主题测试、包校验、Release deb 构建、Xvfb 解包启动、runner apt 实际安装与 purge、资产上传全部成功。P06 已关闭；不扩展为真实账号或所有 Linux 桌面通过。

已下载远端产物到 `build/ci/37884056743/package/`，manifest 为 clean source、原生 GNU x64 Release。deb 为 1,407,160 bytes，SHA-256 `431755c34fcecabc2f7233b4a08abd23e5d1486b720c55ccccf0dba0d22bdfa6`。对下载产物重新执行完整包验证与本机 Ubuntu 26.04 Wayland 空配置冒烟均通过；不覆盖已安装的本机构建包。最终源码与工作流提交已推送，无 GUI tag/Release。

远端完整日志与结构化步骤保存在 `build/linux-qa/ci-linux-success.{log,json}`；产物回读日志 `remote-package-verify.log`，结构化回读证据 `package-20261009T043304530667Z.json`。本机安装的是上一节本机构建的 0.7.0-1；远端产物另行保留作可复核的 CI 构建证据。
