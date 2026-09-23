# 测试策略

## 现状

- workspace 自动化测试由 `cargo test` 单命令运行；数量以本次测试输出为准，不在此固化过时计数
- GitHub Actions 已在 Linux/macOS/Windows 运行 CLI/Core 定向 clippy/test，Linux 额外执行 fmt 门禁。macOS 原生 CI 配置另有 Core/UniFFI、Swift 排序测试及 Debug/Release App 构建；提交后尚需以实际 Actions 结果确认
- CLI 已有隔离沙箱集成测试；端到端依赖 TUI 真实 PTY/tmux 冒烟。原生 GUI 测试在对应平台工程创建后接入

## 分层测试策略

| 层 | 方式 | 位置 |
|----|------|------|
| 纯函数（JWT/email/时间/配额解析/推荐/路径） | 表驱动单测，基准值注明生成方式 | core/cli 各模块 `mod tests` |
| 文件 I/O（原子写/备份/权限/锁） | 临时目录真实读写断言（如 `atomic_write_keeps_backup_private`） | core account.rs |
| TUI 渲染 | `ratatui::backend::TestBackend` 快照断言 | cli tui.rs |
| 前端分发/守卫逻辑 | 构造 `App` 假数据直接断言（如 `palette_visible` 过滤） | cli tui.rs |
| 一次性 CLI 协议 | 参数解析、选择器、退出码、human/JSON 脱敏断言 | cli args/selector/output/commands |
| CLI 文件集成 | 隔离 HOME/CODEX_HOME 后调用二进制，断言 stdout/stderr/退出码及文件副作用 | cli integration tests |
| 端到端 | 真实 PTY 或 tmux 冒烟：验证启动、按键、命令面板与退出 | 人工/会话内 |
| 自升级 | 用已安装旧版在隔离 HOME/CODEX_HOME 执行正式 Release 升级，核对线上 SHA、原地替换、重复执行与真实配置不变 | 人工/发布后 |
| 真实凭据链路 | 用户真机操作回报输出（登录/刷新/配额成功路径不可离线模拟） | 用户协同 |
| macOS 原生 GUI | macOS 主机构建/启动，Swift→UniFFI→Core 调用，SwiftUI、菜单栏与 Dock 交互验收 | macOS 真机 |
| Linux 原生 GUI | Linux 主机构建/启动，GTK→Core 调用，真实桌面会话、Wayland/X11 与状态图标验收 | Linux 真机 |

## 已知约束（写测试时必读）

1. **CJK 断言**：TestBackend buffer 中宽字符占两格、续格为空格（"详情"存为"详 情"），中文 `contains` 断言只用**单字**；ASCII 可整串
2. **时区无关**：时间格式化断言只验格式（长度/分隔符），不硬编码日期；需要具体值的用固定偏移环境
3. **隔离沙箱**：任何触盘测试用临时 `HOME`/`CODEX_HOME`；二进制集成测试额外设置仅 debug 构建生效的 `HANGAR_TEST_HOME`，避免 Windows Known Folder 忽略环境变量；禁止在用户真实目录验证写路径

### macOS 原生应用

```bash
scripts/generate-swift-bindings.sh
xcodebuild -project apps/macos/HangarMac.xcodeproj \
  -scheme HangarMac -configuration Debug \
  -derivedDataPath build/xcode CODE_SIGNING_ALLOWED=NO build
```

- Swift 冒烟程序必须真实链接 release 静态库并调用 `bridgeInfo()`，不能只检查生成文件存在。
- 真实账号运行必须复制账号库到隔离 `HOME`/`CODEX_HOME`；不得让开发版 GUI 对用户真实配置执行刷新或写入。
- UI 验收需确认 7～8 个卡片、串行加载状态、周剩余、推荐标记与 stale 提示；同时检查应用图标、关闭窗口后 Dock 隐藏、菜单栏恢复窗口与真正退出。仅构建成功不能替代可视验收。
- 只读 UI 验收（例如排序、键盘焦点）不得点击会切换账号、删除账号或启动 OAuth 的控件来探测焦点；自动化操作前先获取当前无障碍树并核对控件完整标签，窗口/菜单变化后重新取元素编号。即使使用隔离副本，也遵守“不要切换”等当次操作约束。
- 配额缓存验收需核对重启立即显示旧值、30 分钟内不重复自动请求、失败保留旧值并退出推荐、手动强制刷新；菜单栏每账号周剩余与主窗口快照一致。没有真实账号回报时仅可标记构建/单测/隔离空配置冒烟通过，不可声称真实配额链路通过。
- Xcode Debug 配置必须链接 Rust debug 静态库，使仅 debug 生效的 `HANGAR_TEST_HOME` 能真正隔离账号库；Release 配置仍链接 release 静态库，不提供测试路径覆盖。
- 本机可运行 `scripts/run-macos-qa.sh`：脚本复制真实账号库到临时测试目录，通过 `open --env` 和 LaunchServices 从完整 `.app` 启动 Debug 应用，确保数据隔离且 AppIcon/Asset Catalog 正常加载，同时不污染用户级 launchd 环境；退出后自动清理。
4. **无网络依赖**：单测不打真实接口；wham 解析用内置 JSON 样本。仅 debug/test 构建可用 `HANGAR_TEST_TOKEN_ENDPOINT` 和 `HANGAR_TEST_USAGE_ENDPOINT` 指向本地不可用端口，分别阻断 RT 刷新和模拟配额失败；Release 不读取这些覆盖值
5. **一次性读取命令仍可能收敛**：`list`/`current` 会先执行 `harvest()`，不能作为真实 HOME 的纯只读验证；真实数据兼容冒烟必须把账号库与官方配置复制到权限受限的隔离沙箱后运行
6. **宿主平台门禁**：只在当前宿主实现对应原生 GUI；macOS 不创建 Linux UI，Linux 不创建 macOS UI。交叉编译、CI 编译和无头测试只能记录其实际覆盖，不能替代对应桌面真机交互

## 验收清单（变更合入前）

- [x] `cargo fmt` → `cargo clippy -- -D warnings` → `cargo test` 全绿
- [x] 新行为有对应断言（不是只跑旧测试）
- [x] 核心能力在一次性 CLI 与 TUI 可达；classic 保持基础兼容，shared core 回归通过
- [x] JSON 输出不含 token，未知参数/选择歧义/守卫失败具有非零退出码
- [x] 真机冒烟已做并声明覆盖范围（见 v0.5.0 CLI/TUI 计划验收记录）
- [x] Linux/macOS/Windows 的 CLI/Core `cfg` 分支至少在对应 runner 完成定向 clippy/test
