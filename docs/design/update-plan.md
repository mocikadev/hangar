# 自升级机制 Implementation Plan

> 状态：历史实施计划，功能已随 v0.3.0/v0.3.1 发布。下方未勾选框保留原始计划记录，不再作为当前进度；当前任务见 `v0.4.0-closure-plan.md`。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 `docs/design/update-mechanism.md`：CI+Release、自升级、双前端入口。

**Architecture:** 新增 `core::updater`（纯函数可测 + `ureq` 网络边车），`main.rs` 启动编排，TUI/classic/doctor 三处接入；Windows 用 `.old` 移开交换，不引新依赖。

**Tech Stack:** Rust 2021，workspace 已有 `ureq` / `sha2` / `serde_json`；GitHub Actions（`dtolnay/rust-toolchain`、`cross`、`softprops/action-gh-release`）。

**Spec:** `docs/design/update-mechanism.md`

## Global Constraints

- 不碰 S11 写入顺序（先库后官方）、OAuth 白名单常量、`Account` 结构体、锁过期语义。
- 业务代码输出一律 `crate::emit::emit/emit_err`，`main.rs` 启动层可用 `eprintln`。
- 新增用户可见能力必须同时进 TUI 与 classic（本计划 Task 4 覆盖 doctor 展示）。
- 验证顺序：`cargo fmt` → `cargo clippy -- -D warnings` → `cargo test`。
- 写路径验证一律隔离 `HOME`/`CODEX_HOME` 沙箱；不编造 token/expires_at 宣称通过。
- 提交信息格式 `<英文类型>: <中文描述>`。

## Review Focus

1. GitHub API 返回未知 JSON 形状 → `check_update` 必须 `Ok(None)` 而非报错阻断启动。
2. `SHA256SUMS.txt` 为 CRLF 换行（Windows 生成）→ 解析必须按行 trim，否则 Windows 自升级永远失败。
3. `update-check.json` 损坏 → 视为过期走联网，不报错。
4. `current_exe` 路径含空格/中文 → tmp/`old` 路径拼接必须用 `PathBuf` 操作，不做字符串拼接。
5. 已有残留 `.old` 且再次升级 → 先删旧 `.old` 再移，防止 `rename` 目标已存在失败。

---

### Task 1: git 收尾 + CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: 无（仅 git 操作）

**Interfaces:**
- Consumes: 无
- Produces: CI 基线（后续 Task 的验证前置）

- [ ] **Step 1: 写 ci.yml**（照抄 skm 精简：fmt+clippy+test job，5 target 编译矩阵 job，runner 与 release.yml 一致）

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
env:
  CARGO_TERM_COLOR: always
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test
  build:
    needs: check
    strategy:
      fail-fast: false
      matrix:
        include:
          - { target: x86_64-unknown-linux-musl, runner: ubuntu-latest }
          - { target: aarch64-unknown-linux-musl, runner: ubuntu-latest }
          - { target: x86_64-apple-darwin, runner: macos-latest }
          - { target: aarch64-apple-darwin, runner: macos-latest }
          - { target: x86_64-pc-windows-msvc, runner: windows-latest }
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - run: cargo build --release --target ${{ matrix.target }}
```

- [ ] **Step 2: 本地校验 yaml 可解析**

Run: `python3 -c "import yaml,sys; [yaml.safe_load(open(f)) for f in ['.github/workflows/ci.yml']]; print('YAML_OK')"`
Expected: `YAML_OK`（若缺 `pyyaml` 改用 `ruby -ryaml -e`，仍缺则跳过并记录）

- [ ] **Step 3: 全量 test 基线**

Run: `cargo test`
Expected: 20/20 通过（core 15 + cli 5）

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: 新增 fmt+clippy+test 与多平台编译矩阵"
```

---

### Task 2: `core::updater` 模块

**Files:**
- Create: `crates/core/src/updater.rs`
- Modify: `crates/core/src/lib.rs`（加 `pub mod updater;` + 按需 re-export）
- Test: `crates/core/src/updater.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Consumes: `crate::account::write_file_private`（不存在则本 Task 内新建 helper；以 `account.rs` 现状为准：若不可见则在 `updater.rs` 内自带同款 600 直写函数）
- Produces: `updater::check_update(force: bool) -> Result<Option<ReleaseInfo>, String>`，`updater::apply_update(&ReleaseInfo) -> Result<String, String>`，`updater::current_target() -> Option<&'static str>`，`updater::last_check_secs() -> Option<u64>`

- [ ] **Step 1: 写失败单测**（纯函数部分，4 个 test）

```rust
#[test]
fn newer_comparison() {
    assert!(is_newer("0.3.0", "0.2.0"));
    assert!(!is_newer("0.2.0", "0.2.0"));
    assert!(!is_newer("bad", "0.2.0"));
    assert!(!is_newer("0.2.0", "bad"));
}
#[test]
fn checksum_line_parses_crlf() {
    let body = "abc123  hangar-linux-amd64\r\ndef456  hangar-macos-arm64\r\n";
    assert_eq!(find_checksum(body, "hangar-linux-amd64").as_deref(), Some("abc123"));
}
#[test]
fn cache_ttl_boundary() {
    assert!(cache_expired(1_000_000, 1_000_000 - 86_399)); // 未过期
    assert!(!cache_expired(1_000_000, 1_000_000 - 86_401)); // 已过期
}
#[test]
fn corrupt_cache_means_expired() {
    assert!(!cache_expired(1_000_000, 0)); // last=0 视为过期走联网
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p hangar-core updater`
Expected: FAIL（`updater` 模块不存在）

- [ ] **Step 3: 写最小实现**（`updater.rs` 全量：`ReleaseInfo`、`current_target` 5 平台映射、`parse_version/is_newer`、`find_checksum` 按行 trim、`cache_expired(now,last)`、`check_update`（ureq 短超时 10s/15s，任何失败→`Ok(None)`；成功写缓存）、`apply_update`（下载→验 SHA→Unix rename / Windows `.old` 交换）、`last_check_secs`（解析失败→None）；缓存路径 `switcher_dir().join("update-check.json")` 需 `switcher_dir` 可见，否则经 `accounts_file_path` 的 parent 推导——以实际可见性为准，规则：只用 `pub` API，必要时在 `account.rs` 加 `pub(crate)` 暴露）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p hangar-core updater`
Expected: 4/4 通过；再跑 `cargo test` 全绿 24/24（含新增 4 个）

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/updater.rs crates/core/src/lib.rs crates/core/src/account.rs
git commit -m "feat: 新增自升级检查与应用模块"
```

---

### Task 3: `main.rs` 启动流程 + flags

**Files:**
- Modify: `crates/cli/src/main.rs`
- Test: `crates/cli/src/main.rs` 内 `#[cfg(test)]`（纯函数 `parse_flags`）

**Interfaces:**
- Consumes: Task 2 的 `updater::check_update/apply_update`
- Produces: 无（进程入口行为）

- [ ] **Step 1: 写失败单测**

```rust
#[test]
fn flags_parse() {
    assert!(parse_flags(&["hangar".into(), "--version".into()]).show_version);
    assert!(parse_flags(&["hangar".into(), "--no-update".into()]).no_update);
    assert!(parse_flags(&["hangar".into(), "--check-update".into()]).check_update);
    assert!(parse_flags(&["hangar".into(), "--classic".into()]).classic);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p hangar flags_parse`
Expected: FAIL（`parse_flags` 不存在）

- [ ] **Step 3: 写最小实现**（抽 `parse_flags(args) -> Flags{classic,show_version,no_update,check_update}` 纯函数供测试；`main` 按 spec §5 编排：`--version` 打印 `env!("CARGO_PKG_VERSION")` 退出；`HANGAR_NO_UPDATE=1`/`--no-update` 跳过；`--check-update` 走 force 流程并按结果 `exit(0)/exit(1)`；默认 `check_update(false)`→有新版 `apply_update`→成功打印并 `exit(0)`，失败 `eprintln` 后继续；启动顺手清 `hangar.old`（exe 同目录，best-effort）；原有 TTY/`--classic` 分发保留）

- [ ] **Step 4: 沙箱冒烟**

Run: `HOME=/tmp/opencode/hangar-upd-home CODEX_HOME=/tmp/opencode/hangar-upd-codex ./target/debug/hangar --version`
Expected: 输出 `0.2.0`（当前 cli 版本），exit 0，不触碰真实 HOME

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "feat: 启动自升级检查与版本参数"
```

---

### Task 4: TUI / classic / doctor 接入

**Files:**
- Modify: `crates/cli/src/tui.rs`，`crates/cli/src/classic.rs`，`crates/core/src/doctor.rs`
- Test: `crates/cli/src/tui.rs` 内既有 palette 单测旁新增

**Interfaces:**
- Consumes: Task 2 的 updater API，Task 3 的退出语义（本 Task 只做手动入口，不复用退出逻辑）
- Produces: 无（UI 行为）

- [ ] **Step 1: 写失败单测**（TUI palette 含升级项）

```rust
#[test]
fn palette_lists_update_command() {
    let mut app = fake_app();
    app.palette_open = true;
    let t = screen_text(&mut app);
    assert!(t.contains('升'), "palette missing update");
    app.palette_input = Some("升级".to_string());
    let vis = palette_visible(&app);
    assert_eq!(vis.len(), 1);
    assert_eq!(PALETTE[vis[0]].action, Action::Update);
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p hangar palette_lists_update_command`
Expected: FAIL（无 `Update` 变体；注意 CJK 断言只用单字，沿用既有惯例）

- [ ] **Step 3: 写最小实现**（`Action::Update` + `PALETTE` 条目“检查更新/检查并升级到最新版”；`Ev::UpdateDone{res: Result<String,String>}`；`spawn_update` 后台线程内 `set_quiet(true)` 后 `check_update(true)`→有则 `apply_update`；成功日志“✅ 已升级到 x.y.z，请重启 hangar 生效”，无更新“已是最新”，失败“✗ 更新失败：原因（旧版继续可用）”；`classic.rs` 加 `update` 全字命令同语义；`doctor.rs` 加版本行与上次检查行（经 `fmt_ts_local`，无缓存显示“尚未检查”））

- [ ] **Step 4: 全量测试**

Run: `cargo test`
Expected: 全绿（含新增 1 个，总数 25）

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/tui.rs crates/cli/src/classic.rs crates/core/src/doctor.rs
git commit -m "feat: 双前端升级入口与自检版本展示"
```

---

### Task 5: release.yml + 安装脚本 + 版本号

**Files:**
- Create: `.github/workflows/release.yml`，`install.sh`，`install.ps1`
- Modify: `crates/cli/Cargo.toml`（`0.2.0` → `0.3.0`，首个自升级版本演练）
- Test: 无单测，以静态校验为准

- [ ] **Step 1: 写 release.yml**（照抄 skm：`v*` tag→verify cli 版本一致→5 构建→SHA256→gh-release；产物名 `hangar-*`，Windows 带 `.exe`）

- [ ] **Step 2: 写 install.sh / install.ps1**（照抄 skm，`REPO=mocikadev/hangar`，`BINARY=hangar`，产物名对应；sh 保留中英双语与 SHA256 校验）

- [ ] **Step 3: 版本号 bump**

```bash
# crates/cli/Cargo.toml: version = "0.2.0" → "0.3.0"
cargo test  # 确认 --version 输出 0.3.0
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml install.sh install.ps1 crates/cli/Cargo.toml Cargo.lock
git commit -m "build: 发布流水线与安装脚本，版本升至 0.3.0"
```

---

### Task 6: 首推 + 发版演练（需用户配合）

**Files:** 无（git 操作 + 真机回报）

- [ ] **Step 1: 推送 main 全量**

```bash
git add -A && git status --short  # 确认无 token/凭据文件（重点查 ~/.hangar 残留物未进仓库）
git commit -m "chore: 首版全量代码"  # 若有剩余未提交文件
git push -u origin main
```

- [ ] **Step 2: 打 tag 触发 Release**

```bash
git tag v0.3.0 && git push origin v0.3.0
# CI verify 通过后检查 Release 页面出现 5 产物 + SHA256SUMS.txt
```

- [ ] **Step 3: 真机 `--check-update` 回报**（用户在联网机器执行旧版 `hangar --check-update`，回报输出；离线场景确认秒进 TUI）

---

## Self-Review

- Spec 覆盖：§2 版本规则→Task 5；§3 CI/Release→Task 1/5；§4 updater→Task 2；§5 启动→Task 3；§6 双前端→Task 4；§7 安全→Task 2/3 错误路径；§8 测试→各 Task；§9 顺序→Task 1-6；§10 验收→Task 6。
- 无占位符：每步含真实代码/命令；`account.rs` 可见性差异已在 Task 2 Step 3 注明按实际为准的规则。
- 类型一致：`ReleaseInfo{tag,version,binary_url,checksum_url}`、`check_update(force: bool)`、`apply_update(&ReleaseInfo)->Result<String,String>` 全 Task 统一。
- Review Focus 5 条：CRLF→Task 2 单测；损坏缓存→Task 2 单测；其余 3 条（未知 JSON 形状→静默、`PathBuf` 拼接、`.old` 预清理）已分别落入 Task 2 Step 3 / Task 3 Step 3 实现要求。
