# 版本发布与自升级机制设计（update-mechanism）

> 状态：已实施并经 v0.3.0-v0.6.0 发版验证；后续 CLI 与 GUI 采用独立发布通道
> 对标：`mocika-skills-cli`（`skm`）的 CI 打包 + 自升级链路，按 hangar 约束裁剪
> 远端：`git@github.com:mocikadev/hangar.git`

## 1. 背景与目标

hangar 当前无版本发布流程、无升级能力，用户只能本地 `cargo build`。目标是建立闭环：

1. CLI 或 GUI 标签分别产出本轮 Linux/macOS 资产与各自的 `SHA256SUMS.txt`；Windows 产物延期；
2. `hangar` 启动时自动检查升级（24h 缓存节流），有新版自动升完退出；
3. TUI 命令面板 / classic 菜单各一个手动“检查更新”入口，行为对等；
4. 离线/限流/失败时永远静默放行，不阻塞正常使用。

非目标：增量更新、签名（gpg/cosign）、自动重启新进程、 nightly 渠道、Windows 提权安装。

## 2. 版本规则（单一来源）

- 二进制版本号 = `crates/cli/Cargo.toml` 的 `version`，代码内一律 `env!("CARGO_PKG_VERSION")` 获取，不手写版本号字符串。
- `hangar-core` 版本保持内部库版本，不参与发布比较。
- Release tag 格式 `v{cli版本}`（如 `v0.3.0`），CI 的 `verify` job 强制 `tag == v$(cli Cargo.toml version)`，不一致直接失败。
- CLI 发版步骤：改 `crates/cli/Cargo.toml` → commit → `git tag vX.Y.Z` → push（含 tag）→ CLI Release 自动生成并成为 GitHub `latest`。
- GUI 独立使用 `gui-v{gui版本}`，同时校验 GUI Cargo 与 Tauri bundle 版本；GUI Release 显式 `make_latest: false`，不得影响 CLI 安装和自升级。

## 3. CI / Release（照抄 skm，改名适配）

工作流按现有 crate 与发布边界拆分：

- `.github/workflows/ci.yml`：CLI/Core 路径触发，Linux/macOS/Windows 执行定向 clippy/test。
- `.github/workflows/gui-ci.yml`：GUI/Core 路径触发，Linux/macOS/Windows 执行 GUI 定向 clippy/test；不再追加重复 build。
- `.github/workflows/release.yml`：仅 `v*` tag 触发 → 校验 CLI 版本 → 本轮四个 target 构建：
  - `x86_64-unknown-linux-musl`（cross）→ `hangar-linux-amd64`
  - `aarch64-unknown-linux-musl`（cross）→ `hangar-linux-arm64`
  - `x86_64-apple-darwin` → `hangar-macos-amd64`
  - `aarch64-apple-darwin` → `hangar-macos-arm64`
  → `SHA256SUMS.txt` → `softprops/action-gh-release` 发 CLI Release 并标为 latest。
- `.github/workflows/gui-release.yml`：仅 `gui-v*` tag 触发 → 校验 GUI/Tauri 版本 → Linux amd64/arm64 与 macOS amd64/arm64 安装包 → 独立校验文件 → 非 latest GUI Release。

`install.sh` 继续服务 Linux/macOS。`install.ps1` 与 Windows target 映射保留供后续恢复发布，但 v0.5.0 的 latest Release 不包含 Windows 资产，README 不再引导用 latest 安装 Windows 版本。

## 4. `core::updater` 模块设计（零新依赖）

只用 workspace 已有依赖：`ureq`（blocking http）+ `sha2`（SHA256）+ `serde_json`。不引入 `reqwest` / `self_update` / `self-replace`。

```rust
pub struct ReleaseInfo { pub tag: String, pub version: String, pub binary_url: String, pub checksum_url: String }
pub fn current_target() -> Option<&'static str>  // 5 平台映射，不支持返回 None
pub fn check_update(force: bool) -> Result<Option<ReleaseInfo>, String>
pub fn apply_update(info: &ReleaseInfo) -> Result<String, String>  // 成功返回新版本号
pub fn last_check_secs() -> Option<u64>  // 读缓存，供 doctor 展示
```

常量：`REPO = "mocikadev/hangar"`，`GITHUB_API = "https://api.github.com"`，
检查请求 `User-Agent: hangar/{版本}`，连接超时 10s、总超时 15s；下载总超时 300s；
支持 `GITHUB_TOKEN` 环境变量（与 skm 一致，缓解未认证 60/h 限流）。

### 4.1 `check_update` 语义

- `force=false`：先读 `~/.hangar/update-check.json`（`{last_check: u64秒, last_version: string}`）；
  `now - last_check < 86400` 直接返回 `Ok(None)`（不联网）；否则联网查 `releases/latest`。
- `force=true`：跳过缓存直接联网。
- 版本比较：`x.y.z` 三段数值比较（`v` 前缀剥离），解析失败视为“无更新”（不报错）。
- 以下情况一律 `Ok(None)`（静默放行，不进 TUI 前报错）：网络错、超时、非 2xx（含 403/429 限流）、JSON 解析失败、缺对应平台产物、缺 SHA256 条目、当前平台不支持。
- 联网检查完成后（无论有无新版）最佳努力写回缓存 `last_check=now`；写失败忽略。

### 4.2 `apply_update` 语义

1. 下载二进制字节 + 下载 `SHA256SUMS.txt`；
2. 按 `<hex>  <文件名>`（双空格）解析出本平台条目，大小写不敏感比对；不匹配→删 tmp 并 `Err`；
3. `std::env::current_exe().canonicalize()` 定位自身，同目录写 `.hangar-update-tmp`（Unix 经 `mode(0o600)` 写，复用 `account::write_file_private` 思想）；
4. Unix：`chmod 755` → `rename(tmp → exe)` 原子替换；
   Windows：`rename(exe → hangar.old)`（运行中 exe 允许改名移开）→ `rename(tmp → exe)`；
   任一步失败→清 tmp、保留旧二进制，返回 `Err`（调用方继续进 TUI，不阻断使用）；
5. 成功返回 `info.version`。Windows 下残留 `hangar.old` 由下次启动顺手删除（见 §5）。

### 4.3 与 skm 的差异（论证后偏离）

- skm 全平台一把 `rename(tmp → exe)`，Windows 下替换运行中 exe 会 sharing violation 失败；
  本设计 Windows 用 `.old` 移开交换，全自动，无需用户手动替换或重跑安装脚本。
- http 客户端用 `ureq` 而非 `reqwest`：workspace 已有，不增依赖。
- 不引入 `self-replace` crate：Windows 逻辑仅约 30 行且两端清理都是自家代码，可控；helper 子进程方案反而扩大审计面。

## 5. 启动流程（`crates/cli/src/main.rs`）

```
解析 args（沿用现有手写风格新增 --version/--no-update/--check-update，不引入 clap）
  → --version：打印版本退出
  → --no-update 或 HANGAR_NO_UPDATE=1：跳过检查
  → --check-update：force 检查，有新版则升级后打印版本号 exit(0)，无新版打印已是最新 exit(0)，失败打印原因 exit(1)
  → 默认：check_update(false)；有新版 → apply_update → 成功打印新版本号并 exit(0)（升级后不进 TUI，用户自行重进）；
     失败打印原因后继续；无新版直接进
  → 启动时顺手清 Windows 残留 hangar.old（存在且 hangar.exe 存在时删除，失败忽略）
  → 原有 TTY 检测 → tui / classic
```

约束：检查与下载都不持账号锁；错误输出走纯文本 `eprintln`（main 层允许，业务层仍走 `emit`）；
升级流程与 harvest/switch 无交叉，S11 顺序不受影响。

## 6. 三前端对等

- TUI（`tui.rs`）：`Action` 新增 `Update`（命令面板“检查更新”，hint“检查并升级到最新版”）；
  后台线程执行 `check_update(true)+apply_update`，经 `Ev::UpdateDone{res}` 回主线程；
  `busy` spinner 复用现有；成功日志“✅ 已升级到 x.y.z，请重启 hangar 生效”，失败“✗ 更新失败：原因（旧版继续可用）”；不自动退出。
- classic（`classic.rs`）：新增 `update` 命令同语义（`u` 已被配额占用，用全字 `update`）。
- doctor（`doctor.rs`）：加两行——当前版本（`env!`）、上次检查时间（`last_check_secs` 经 `fmt_ts_local`，无缓存显示“尚未检查”）。
- GUI：只检查版本并引导下载对应安装包，不在运行中直接替换已安装应用。

## 7. 安全声明

- 校验失败绝不替换；tmp 文件残留必清理；旧二进制在替换成功前不碰。
- 错误输出只记 `HTTP status + body_len`（沿用脱敏规范），不打 URL 以外的敏感信息、不打 token。
- 缓存文件经 600 写（无敏感内容，为一致性）。
- 升级不读写账号库与 `auth.json`，不触碰凭据。

## 8. 测试策略

- 单测（离线，`#[cfg(test)]`）：`parse_version/is_newer` 三段比较（含 `v` 前缀/非法输入）、
  `current_target` 编译期映射断言（本机目标非 None）、SHA256SUMS 行解析、缓存 TTL 边界（(now-86399 跳过，now-86401 联网——以可注入时间的纯函数测）。
- 不做真实网络单测；`--check-update` 真机由用户回报输出。
- 验证命令沿用项目清单：`cargo fmt` → `cargo clippy -- -D warnings` → `cargo test`，
  另加沙箱冒烟（隔离 HOME/CODEX_HOME 跑 `--version` 与 `doctor`，不断言真实凭据）。

## 9. 落地顺序（实施计划将细化）

1. git 初始化 + 远端 + 首次推送 + `.github/ci.yml`；
2. `core::updater` + 单测；
3. `main.rs` 启动流程 + flags；
4. TUI/classic/doctor 三处接入；
5. `release.yml` + `install.sh/ps1`；
6. 本地 tag 演练（`v0.3.0` 发版）+ 真机 `--check-update` 回报。

## 10. 验收标准

- `v*` tag 推送后 4 个 CLI 产物 + SHA256 自动出现在 latest GitHub Release；
- `gui-v*` tag 推送后只出现 Linux/macOS GUI 安装包 + SHA256，且不改变 latest；
- 新版发布后 24h 内，老版本启动一次即自动升级并退出（沙箱可复现）；
- 无网络时启动延迟增加 ≤ 总超时且必进 TUI；
- SHA256 篡改演练中拒绝替换、旧二进制可用；
- Windows 真机（或至少 cross 编译通过 + 逻辑走读）确认 `.old` 交换路径；
- `fmt/clippy/test` 全绿。
