# GUI 版本设计（egui 第三前端）

> 状态：已实施并进入维护模式；v0.5.0 继续发布 Linux/macOS 包，Windows 安装包与真机验收延期
> 决策：界面层 egui（`eframe`）；打包 `tauri-bundler` 独立使用；后端复用 `hangar-core`

## 1. 目标与非目标

目标：给不懂终端的用户一个双击即用的桌面版，首版能力与 TUI 完全对等
（切换/添加/复活/删除/配额/自检/检查更新），安装包分发（dmg/nsis/deb）。

非目标：界面美学竞赛、开机自启、通知中心、多语言（中文单语先行）。系统托盘已纳入正式范围。

## 2. 框架选型结论

egui（eframe）：单二进制 5～10MB，无 webview，Linux 免装 webkit；
全 Rust 单语言，业务简单（列表+详情+按钮+进度条）几百行搞定；
TUI 的后台线程+mpsc 模型原样复用。代价（界面朴素、无官方安装包）
分别用“目标用户要可用而非好看”与 tauri-bundler 覆盖。
备选 Tauri 仅当用户抱怨界面质感时重启评估，后端 core 不动。

## 3. 架构

新增 `crates/gui`（cargo 内 bin 名 `hangar-gui`，见 §8 命名说明），与 `crates/cli` 平级，只依赖
`hangar-core` 公开 API。UI 线程只渲染，不直接做网络与切换。

```
crates/gui (bin hangar-gui, eframe)
  app.rs    App 状态机 + 帧渲染（左列表/右详情配额/底状态行/工具栏/弹窗）
  worker.rs 后台线程池（切换/配额/登录/升级）+ mpsc 事件回 UI
  hooks.rs  LoginHooks 的 GUI 实现（弹窗 URL + 粘贴框）
  tray.rs   Linux SNI / Windows 与 macOS 原生托盘
      │ 仅依赖 core 公开 API
crates/hangar-core（唯一改动：LoginHooks，见 §7）
```

## 4. 主窗口布局

- 左列账号列表：email + ●使用中 / ⚠失效徽标；点击选中。
- 右上详情：令牌有效期、刷新令牌状态、计划、账号状态。
- 右下配额：窗口剩余进度条 + 重置时间 + 重置卡行（对标 TUI）。
- 底部状态行：一句话状态（就绪/忙/成功/失败原因），替代日志栏。
- 顶部工具栏按钮：添加 / 复活 / 删除 / 刷新配额 / 自检 / 检查更新 / 关于。
- 弹窗四个：添加（含浏览器提示+URL 复制框+回调粘贴框）、删除二次确认、
  复活（同添加流程换标题）、关于（含版本+检查更新按钮）。

## 5. 线程与数据流

沿用 TUI 模式：事件枚举（SwitchDone/QuotaOne/QuotaDone/UpdateDone/LoginDone），
UI 线程每帧 `try_recv`；忙时相关按钮置灰 + 进度转圈。
启动先 `harvest()`；启动后台静默刷一次全量配额；60s 被动收敛（egui 定时重绘触发）。

## 6. 特殊流程

- **OAuth 添加/复活**：后台跑登录流程；需手动粘贴时弹窗给出 URL（复制按钮）+
  粘贴输入框 + 确认/取消；添加成功仅入库不切换，复活成功原位更新并切换，失败状态行显示原因。
- **自升级**：GUI 走安装包分发，应用内不自动替换二进制。复用 `core::updater`
  的版本检查（`newer_version_available`，24h 缓存、静默失败逻辑不变），
  发现新版弹“去下载页下载安装包”指引（含链接复制），用户手动装新包；
  CLI 的自动替换链路不受影响（两边按二进制名隔离产物）。
- **Codex 运行中**：切换/添加成功后若 `codex_process_running()`，弹模态提示
  “请重启 Codex 生效”，替代终端文字提示。
- **离线**：任何网络失败静默记状态行，不弹窗打断（手动检查更新除外，如实报错）。
- **系统托盘**：账号菜单复用 GUI 守卫；Linux 已验证。macOS 红色关闭事件必须在 eframe 的 `logic()` 阶段取消（窗口不可见时可能不执行 `ui()`），随后切换为 `Accessory`、隐藏窗口与 Dock；从托盘恢复时切回 `Regular` 并重显 Dock 与窗口。Windows 保留托盘恢复入口。

## 7. core 改动（唯一）

`login_codex()` 内手动粘贴回调走 stdin，GUI 无 stdin。抽取：

```rust
pub trait LoginHooks {
    fn show_auth_url(&self, url: &str);          // 默认：println
    fn prompt_callback(&self, state: &str) -> Option<String>; // 默认：stdin 读一行
}
pub fn login_codex_with(hooks: &dyn LoginHooks) -> Result<Account, String>;
pub fn login_codex() -> Result<Account, String>; // 默认 hooks，老行为不变
```

TUI/classic 走 `login_codex()` 零改动；GUI 实现 `LoginHooks`（弹窗版）。
`Account` 结构、`emit`、锁语义、S11 顺序均不碰。

## 8. 打包分发

### 命名（对外统一叫 hangar）

cargo workspace 内不允许两个 bin 同名（CLI 已占 `hangar`，产物会打架），
故 cargo 层 bin 名保留 `hangar-gui`，**用户可见处一律叫 hangar**：

- bundler `productName = "hangar"`：macOS 得 `Hangar.app`（Dock/启动台显示 Hangar），
  Windows NSIS 安装程序与开始菜单项为 hangar、装完 exe 为 `hangar.exe`，
  Linux `.desktop` 名称 Hangar、可执行仍指向包内二进制。
- CLI 的 `hangar` 与 GUI 的 `hangar` 安装位置不同（前者在 `~/.local/bin`，
  后者在系统应用目录），互不覆盖；自升级各自替换各自的 `current_exe`，互不干扰。

### 产物与流水线

- `tauri-bundler` 独立使用（不引 Tauri runtime）：macOS `.dmg`、Linux `.deb` / `.rpm` / `.AppImage`，Linux 附 `.desktop` 启动器；Windows `nsis .exe` 配置保留但不进入 v0.5.0 Release。
- 版本号与 cli 同源（发版一起 bump，tag 校验覆盖 gui 包名）。
- v0.5.0 Release 资产覆盖 `linux-amd64`、`linux-arm64`、`macos-amd64`、`macos-arm64`；Windows 资产延期；
  `install.sh` 不动（CLI 用户）；GUI 用户从 Release 页下载安装包。
- CI 在 Linux、macOS、Windows 执行 GUI 构建，release 矩阵负责各平台正式产物。

## 9. 测试策略

- egui 无 `TestBackend` 式快照：首版以后台 worker 状态机单测（事件收敛逻辑纯函数化）
  + core 既有单测 + 三平台手工冒烟（启动/切换/添加/升级/离线）为准。
- 门禁：`cargo fmt` → `cargo clippy -- -D warnings` → `cargo test` 全绿；
  真机网络链路由用户回报，不编造。

## 10. 落地顺序

1. core `LoginHooks` 抽取 + 老前端回归绿；
2. `crates/gui` 骨架（空窗口可启动）+ CI 接入；
3. 列表/详情/配额只读三件套；
4. 切换/添加（含弹窗）/复活/删除；
5. 自检展示/检查更新/关于；
6. bundler 打包 + 三平台冒烟 + 发版。

## 11. 验收标准

- Linux/macOS 安装包可安装、启动无终端、无需任何命令；Windows 安装包与真机验收延期，不阻塞 v0.5.0；
- 与 TUI 逐项对等操作一遍，结果一致（切换生效、配额数字一致）；
- 无网络启动 ≤ 超时后必进主窗口；SHA 篡改演练拒绝替换；
- `fmt/clippy/test` 全绿。
