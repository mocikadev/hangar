# GUI（egui 第三前端）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `crates/gui`（egui），能力与 TUI 对等，对外打包名为 hangar。

**Architecture:** UI 线程只渲染 `App` 状态；切换/配额/登录/升级全跑后台线程，经 `mpsc::Receiver<Ev>` 每帧 `try_recv` 收敛；OAuth stdin 改 `LoginHooks` 注入；打包用 tauri-bundler 独立产出安装包。

**Tech Stack:** Rust 2021，`eframe = "0.36"`（egui），`hangar-core` 路径依赖；tauri-cli v2 仅打包用。

**Spec:** `docs/design/gui.md`

## Global Constraints

- 不碰 S11 写入顺序、OAuth 白名单常量、`Account` 结构体、锁过期语义；Task 1 的 trait 抽取保持默认行为与原来逐行一致。
- GUI 层禁止直接读写 `~/.hangar` 与 `auth.json`，一律走 core 公开 API。
- cargo 内 bin 名 `hangar-gui`，用户可见处一律 hangar（§Task 6 负责）。
- 错误展示只说人话状态行，不打 token/body；升级失败旧版继续可用。
- 验证顺序：`cargo fmt` → `cargo clippy -- -D warnings` → `cargo test`；GUI 无快照测试，以 reducer 单测 + 三平台手工冒烟为准；写路径验证隔离 HOME/CODEX_HOME；不编造 token。
- CI 无 GPU：GUI 只做编译门禁，不跑界面。

## Review Focus

1. OAuth 手动粘贴串了 state → 弹窗报错且后台监听不死，App 停在添加对话框（Task 4 测）。
2. 后台线程掉线/返回 Err → UI 不卡死，状态行一句话报错（Task 3 测）。
3. 配额字段缺失 → 渲染"未知"分支，不 panic（Task 3 测）。
4. 下载完 SHA 不匹配 → 不替换、tmp 清理、旧二进制可用（Task 5 以 reducer+_helpers 测替代真下载，见 Task 内说明）。
5. 点删除使用中账号 → GUI 层直接拦截，不调 core（Task 4 测）。

---

### Task 1: core `LoginHooks` 抽取

**Files:**
- Modify: `crates/core/src/oauth.rs`（`prompt_manual_callback` 改经 hooks；`login_codex` 拆 `login_codex_with`）
- Modify: `crates/core/src/lib.rs`（`pub use oauth::{login_codex, login_codex_with, LoginHooks}`）
- Test: `crates/core/src/oauth.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Consumes: 无
- Produces: `pub trait LoginHooks { fn show_auth_url(&self, url: &str); fn prompt_callback(&self, state: &str) -> Option<String>; }`；`pub fn login_codex_with(hooks: &dyn LoginHooks) -> Result<Account, String>`；`pub fn login_codex()`（默认 stdin 行为，老调用方不动）

- [x] **Step 1: 写失败单测**（默认 hooks 与旧逻辑一致：拒绝非本地回调、state 不匹配）

```rust
struct StdinHooks;
impl LoginHooks for StdinHooks {
    fn show_auth_url(&self, url: &str) { println!("{}", url); }
    fn prompt_callback(&self, _state: &str) -> Option<String> { None }
}

#[test]
fn hooks_reject_wrong_state() {
    let hooks = StdinHooks;
    assert!(parse_code_from_callback_url(
        "http://localhost:1455/auth/callback?code=abc&state=s1", "other"
    ).is_err());
    let _ = &hooks as &dyn LoginHooks;
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p hangar-core hooks_reject_wrong_state`
Expected: FAIL（`LoginHooks` 不存在）

- [x] **Step 3: 写最小实现**（`oauth.rs` 内加 trait + 默认 `StdinHooks`（私有）；`prompt_manual_callback(expected_state)` 改为 `prompt_manual_callback_with(hooks, expected_state)`，原函数保留为薄 wrapper 调默认实现；`login_codex` 改调 `login_codex_with(&StdinHooks)`；`println!` 授权 URL 改经 `hooks.show_auth_url`；回调线程/state 校验逻辑逐行不动）

- [x] **Step 4: 运行确认通过**

Run: `cargo test`
Expected: 全绿（core 20 + cli 7，既有 OAuth 单测全部通过即默认行为未变）

- [x] **Step 5: Commit**

```bash
git add crates/core/src/oauth.rs crates/core/src/lib.rs
git commit -m "refactor: OAuth 登录抽取 LoginHooks，老行为默认实现"
```

---

### Task 2: `crates/gui` 骨架（空窗口可启动）

**Files:**
- Create: `crates/gui/Cargo.toml`，`crates/gui/src/main.rs`，`crates/gui/src/app.rs`
- Modify: 根 `Cargo.toml`（members 加 `"crates/gui"`），`.github/workflows/ci.yml`（check 后加 `cargo build -p hangar-gui` 冒烟行）
- Test: 无单测，以启动为准

**Interfaces:**
- Consumes: Task 1（仅编译依赖，骨架暂不用 hooks）
- Produces: `cargo run -p hangar-gui` 弹出空窗口；后续 Task 的文件落点（`app.rs` 的 `App`、`worker.rs`、`hooks.rs`）

- [x] **Step 1: 写 Cargo.toml 与最小入口**

```toml
[package]
name = "hangar-gui"
version = "0.3.0"
edition = "2021"

[[bin]]
name = "hangar-gui"
path = "src/main.rs"

[dependencies]
hangar-core = { path = "../core" }
eframe = "0.36"
```

```rust
// src/main.rs
mod app;
fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions::default();
    eframe::run_native("hangar", opts, Box::new(|_cc| Ok(Box::new(app::App::default()))))
}
```

```rust
// src/app.rs
#[derive(Default)]
pub struct App {
    status: String,
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("hangar");
            ui.label(self.status.is_empty().then_some("就绪").unwrap_or(&self.status));
        });
    }
}
```

- [x] **Step 2: 接入 workspace 并验证启动**

Run: 根 `Cargo.toml` members 加 `"crates/gui"` 后执行 `cargo run -p hangar-gui`
Expected: 空窗口标题 hangar，中央“hangar/就绪”；沙箱禁止事项：不读写 HOME（空窗口无业务调用）

- [x] **Step 3: CI 冒烟行**

```yaml
      - run: cargo build -p hangar-gui
```

加到 ci.yml `cargo build` 之前；本地跑 `cargo build -p hangar-gui` 通过。

- [x] **Step 4: Commit**

```bash
git add crates/gui Cargo.toml .github/workflows/ci.yml
git commit -m "feat: GUI 空窗口骨架与 CI 编译门禁"
```

---

### Task 3: 只读三件套（列表/详情/配额）+ worker 管道

**Files:**
- Create: `crates/gui/src/worker.rs`
- Modify: `crates/gui/src/app.rs`
- Test: `crates/gui/src/app.rs` 内 `#[cfg(test)]`（reducer 纯函数）

**Interfaces:**
- Consumes: Task 2 的 `App` 落点；core 的 `load_accounts/harvest/fetch_quota_for_account/freshou`（只读：`load_accounts`、`harvest`、`fetch_quota_for_account`）
- Produces: `enum Ev { HarvestDone, QuotaOne{id,res}, QuotaDone }`；`fn reduce(&mut App, ev: Ev)` 纯收敛；`fn spawn_quota(tx, ids)`；Task 4 复用该管道加变体

- [x] **Step 1: 写失败单测**（Review Focus #2/#3）

```rust
#[test]
fn reducer_quota_error_never_panics() {
    let mut app = App::default();
    app.reduce(Ev::QuotaOne { id: "x".into(), res: Err("断网".into()) });
    assert!(app.status.contains("断网"));
    assert!(app.quotas.get("x").is_none());
}

#[test]
fn reducer_empty_windows_renders_unknown() {
    let mut app = App::default();
    app.reduce(Ev::QuotaOne {
        id: "x".into(),
        res: Ok(hangar_core::quota::Quota::default()),
    });
    assert_eq!(quota_summary(&app, "x"), "未知");
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p hangar-gui reducer_quota_error_never_panics`
Expected: FAIL（`Ev`/`reduce`/`quota_summary` 不存在）

- [x] **Step 3: 写最小实现**（`worker.rs`：`Ev` 枚举、`spawn_harvest(tx)`（调 `harvest()` 后发 `HarvestDone`）、`spawn_quota(tx, ids)`（逐个 `fetch_quota_for_account` 发 `QuotaOne`，完发 `QuotaDone`；线程内无 UI 调用）；`app.rs`：`App{accounts, current, selected, quotas, status, busy, tx, rx}`、`reduce()` 纯函数处理三变体（Err 只写状态行）、`quota_summary()` 空窗口返回"未知"、渲染左列表（email+●/⚠）/右详情/配额条（`egui::ProgressBar`）、启动 `harvest`+后台全量配额（TUI 同款）；`main.rs` 保持不变）

- [x] **Step 4: 运行确认通过**

Run: `cargo test -p hangar-gui`
Expected: 2/2 通过；`cargo test` 全绿；本机 `cargo run -p hangar-gui` 沙箱账号可见列表（只读冒烟，截图或文字回报）

- [x] **Step 5: Commit**

```bash
git add crates/gui/src/worker.rs crates/gui/src/app.rs
git commit -m "feat: GUI 账号列表详情配额只读展示"
```

---

### Task 4: 写操作（切换/添加/复活/删除）+ 弹窗

**Files:**
- Create: `crates/gui/src/hooks.rs`
- Modify: `crates/gui/src/app.rs`，`crates/gui/src/worker.rs`
- Test: `crates/gui/src/app.rs` 内 `#[cfg(test)]` 追加

**Interfaces:**
- Consumes: Task 1 的 `LoginHooks/login_codex_with`；Task 3 的 `Ev/reduce/spawn_*` 管道
- Produces: 完整 `App` 交互；Task 5 在其上加 Doctor/Update 动作

- [x] **Step 1: 写失败单测**（Review Focus #1/#5）

```rust
#[test]
fn delete_current_account_is_blocked_in_gui() {
    let mut app = App::default();
    app.current = Some("id-1".into());
    assert!(!app.can_delete("id-1"));
    assert!(app.can_delete("id-2"));
}

#[test]
fn login_dialog_survives_state_mismatch() {
    // state 不匹配只记错，不杀对话框：reducer 收到 LoginFailed 仍保持 dialog=Add
    let mut app = App::default();
    app.dialog = Some(Dialog::Add { url: "http://x".into(), error: String::new() });
    app.reduce(Ev::LoginFailed("state 不匹配".into()));
    assert!(matches!(app.dialog, Some(Dialog::Add { .. })));
    assert!(app.status.contains("state 不匹配"));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p hangar-gui delete_current_account_is_blocked_in_gui`
Expected: FAIL（`can_delete`/`Dialog` 不存在）

- [x] **Step 3: 写最小实现**（`hooks.rs`：`struct GuiHooks { url_slot: Mutex<Option<String>> }` 实现 `LoginHooks`（`show_auth_url` 存槽位由 UI 线程轮询取走展示，`prompt_callback` 返回预填槽——GUI 不阻塞等 stdin，粘贴框内容由 worker 经 oneshot 送入；为保持简单：`prompt_callback` 从 `Mutex<Option<String>>` 取，若空则返回 `None`（对应取消），对话框“确认”按钮把输入写入槽并重发一次登录）；`worker.rs` 加 `Ev::SwitchDone{res}/LoginDone{res: Result<String,String>}/LoginFailed(String)` 与 `spawn_switch/spawn_login`；`app.rs` 加 `Dialog{Add{url,error}, ConfirmDelete{email}, Notice(String)}`、`can_delete()`（使用中直接 false）、工具栏按钮（添加/复活/删除/刷新配额）、忙时按钮置灰；切换成功后 `codex_process_running()` 则弹 `Notice("检测到 Codex 正在运行，请重启生效")`）

- [x] **Step 4: 运行确认通过**

Run: `cargo test -p hangar-gui`
Expected: 4/4 通过；`cargo test` 全绿；本机冒烟：切换/添加（用测试号或取消路径）/删除拦截（使用中账号点删除只提示不弹窗）

- [x] **Step 5: Commit**

```bash
git add crates/gui/src/hooks.rs crates/gui/src/app.rs crates/gui/src/worker.rs
git commit -m "feat: GUI 切换添加复活删除与弹窗"
```

---

### Task 5: 自检/关于/检查更新

**Files:**
- Modify: `crates/gui/src/app.rs`，`crates/gui/src/worker.rs`
- Test: `crates/gui/src/app.rs` 内 `#[cfg(test)]` 追加

**Interfaces:**
- Consumes: Task 4 的 `Dialog/Ev` 管道；core 的 `doctor_lines(&str)`（传 cli 同款版本串常量，见下）、`updater::{check_update, apply_update}`
- Produces: 无（终端行为）；版本串：`crates/gui/Cargo.toml version` 与 cli 保持一致（本 Task 若为 0.3.0 则写 0.3.0，发版一起 bump）

- [x] **Step 1: 写失败单测**（Review Focus #4 的可测部分：SHA 不匹配绝不替换，由调用前置检查保证；测“已是最新不弹窗”）

```rust
#[test]
fn no_dialog_when_already_latest() {
    let mut app = App::default();
    app.reduce(Ev::UpdateDone { res: Ok(None) });
    assert!(app.dialog.is_none());
    assert_eq!(app.status, "已是最新版本");
}

#[test]
fn update_failure_keeps_old_version_usable() {
    let mut app = App::default();
    app.reduce(Ev::UpdateDone { res: Err("断网".into()) });
    assert!(app.dialog.is_none());
    assert!(app.status.contains("旧版继续可用"));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p hangar-gui no_dialog_when_already_latest`
Expected: FAIL（`UpdateDone` 变体不存在）

- [x] **Step 3: 写最小实现**（`worker.rs` 加 `spawn_update(tx)`（`check_update(true, GUI_VERSION)`→有则 `apply_update`→`Ok(Some(v))/Ok(None)/Err`；`Ev::UpdateDone{res: Result<Option<String>,String>}`）；`app.rs`：工具栏“检查更新”/关于对话框（含版本+检查更新按钮）、自检对话框（`doctor_lines(GUI_VERSION)` 文本滚动区）、升级成功弹确认框“已升级到 x.y.z，点重启生效”→确认才 `std::process::exit(0)`；`GUI_VERSION = env!("CARGO_PKG_VERSION")`（gui 包版本，发版与 cli 一起 bump 的规则写进 release 流程备注）

- [x] **Step 4: 运行确认通过**

Run: `cargo test`
Expected: 全绿（gui 6 + 既有 27）；本机冒烟：关于/自检/检查更新各点一遍（无 Release 时应为“已是最新”或如实报错）

- [x] **Step 5: Commit**

```bash
git add crates/gui/src/app.rs crates/gui/src/worker.rs crates/gui/Cargo.toml
git commit -m "feat: GUI 自检关于与检查更新"
```

---

### Task 6: 打包（dmg/nsis/deb）与发版接入

**Files:**
- Create: `crates/gui/bundle/tauri.conf.json`，`crates/gui/assets/icon.svg`（转全套图标源）
- Modify: `.github/workflows/release.yml`（gui 5 产物 + gui 版本校验），`README.md`（GUI 下载入口三行）
- Test: 无单测，以产物为准

**Interfaces:**
- Consumes: Task 2-5 的 `hangar-gui` 二进制
- Produces: 安装包产物

- [x] **Step 1: 装 tauri-cli 并写最小 bundle 配置**（`cargo install tauri-cli --version "^2" --locked`；`tauri.conf.json` 含 `productName: "hangar"`、`identifier`（如 `dev.mocika.hangar`，无域名则用该占位并在提交信息注明）、`version` 占位由打包脚本按 `crates/gui/Cargo.toml` 覆写、bundle targets `["dmg","nsis","deb","appimage"]`、`icon` 指向生成目录；执行器以 `tauri bundle --help` 输出与构建日志为准修正字段名——这是构建迭代不是占位）

- [x] **Step 2: 图标源与生成**（`assets/icon.svg`：圆角矩形底 + 白色挂钩折线，256 视图盒，内容自包含如下；跑 `tauri icon assets/icon.svg` 生成 `icons/` 全套）

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <rect x="8" y="8" width="240" height="240" rx="56" fill="#0ea5e9"/>
  <path d="M128 52 v96 m0 0 c-34 0 -52 -18 -52 -44 m52 44 c34 0 52 -18 52 -44 m-52 132 v-40"
        stroke="#fff" stroke-width="20" fill="none" stroke-linecap="round"/>
</svg>
```

- [x] **Step 3: release.yml 接入**（verify job 加 gui 版本一致校验（同 cli 规则）；build 矩阵产物名加 `hangar-gui-{linux-amd64,linux-arm64,macos-amd64,macos-arm64,windows-x86_64.exe}`；Unix 用对应 target 的 `hangar-gui` 二进制改名 stage，Windows 同理 `.exe`）

- [x] **Step 4: 本机验证**（Linux 本机 `tauri bundle --bundles deb` 产出 `.deb`；`cargo fmt --check`/`clippy`/`test` 全绿；README 加 GUI 下载三行：Release 页下对应包、双击安装、首次打开从应用列表启动 hangar）

- [x] **Step 5: Commit**

```bash
git add crates/gui/bundle crates/gui/assets .github/workflows/release.yml README.md
git commit -m "build: GUI 安装包打包与发版接入"
```

---

## Self-Review

- Spec 覆盖：§3 架构→Task 2；§4 布局→Task 3/4；§5 线程→Task 3；§6 OAuth→Task 1+4、自升级→Task 5、Codex 提示→Task 4；§7 core 改动→Task 1；§8 打包命名→Task 6（cargo 名 hangar-gui、productName hangar）；§9 测试→各 Task；§10 顺序→Task 1-6。
- 无占位符：每步含真实代码/命令；Task 6 的 tauri 字段名以 CLI 输出为准属构建迭代，已明示。
- 类型一致：`Ev::{HarvestDone,QuotaOne{id,res},QuotaDone,SwitchDone,LoginDone,LoginFailed,UpdateDone}` 全 Task 统一；`check_update(force: bool, current_version: &str)`、`apply_update(&ReleaseInfo)->Result<String,String>`、`doctor_lines(&str)` 签名统一；`Dialog::{Add,ConfirmDelete,Notice,About,Doctor}` Task 4 定义、Task 5 扩展 About/Doctor。
- Review Focus：5 条各有归属 Task 与单测；第 4 条真下载不可离线测，以“SHA 前置检查+不替换”语义的 reducer 测试覆盖。
