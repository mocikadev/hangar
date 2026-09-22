# CLI/GUI CI 与发布解耦计划

> 状态：已完成（2026-09-22）

## 目标

1. CLI/TUI 或文档变更不再无条件编译 GUI。
2. `hangar-core` 变化仍在 Linux、macOS、Windows 上同时回归 CLI 和 GUI。
3. CLI 与 GUI 使用独立发布入口和版本节奏；Windows 只保留源码兼容门禁。
4. CLI Release 始终是 GitHub `latest`，保证安装脚本与 CLI 自升级契约不变。

## 边界与触发规则

| 变化范围 | CLI/Core CI | GUI CI |
|----------|--------------|--------|
| `crates/cli/**` | 是 | 否 |
| `crates/gui/**` | 否 | 是 |
| `crates/core/**` | 是 | 是 |
| `Cargo.toml` / `Cargo.lock` | 是 | 是 |
| 纯文档 | 否 | 否 |

- CLI 标签：`vX.Y.Z`，只产生四个平台 CLI 二进制和校验文件，并设为 GitHub `latest`。
- GUI 标签：`gui-vX.Y.Z`，只产生 Linux/macOS 安装包和校验文件，显式设置 `make_latest: false`。
- Windows GUI 继续参与三平台 CI，但两个发布工作流均不产生 Windows 资产。
- GUI 依赖 core，因此 core 变化必须同时触发两条 CI；CLI 与 GUI 不直接互相依赖。

## 任务清单

### Phase 1：工作流拆分

- [x] C01 将日常 CI 收敛为 CLI/Core 定向 clippy 与 test
- [x] C02 新增按路径触发的三平台 GUI CI
- [x] C03 删除日常 CI 中重复的 `cargo build -p hangar-gui`
- [x] C04 保证 core 与依赖锁变化同时触发两条 CI

### Phase 2：发布解耦

- [x] C05 将 `v*` Release 改为只校验 CLI 版本、只构建 CLI
- [x] C06 新增 `gui-v*` GUI 安装包 Release
- [x] C07 GUI Release 不抢占 `latest`，Windows 不进入任何发布矩阵
- [x] C08 两条 Release 分别生成只覆盖自身资产的 `SHA256SUMS.txt`

### Phase 3：验证与交付

- [x] C09 静态校验工作流语法、路径矩阵、标签与资产契约
- [x] C10 按序通过 `cargo fmt`、`cargo clippy -- -D warnings`、`cargo test`
- [x] C11 推送后确认 CLI/Core CI 与 GUI CI 三平台全绿
- [x] C12 确认不创建测试标签、不触发正式 Release，并记录验证证据

## 验收标准

- CLI-only 提交只触发 CLI/Core CI；GUI-only 提交只触发 GUI CI；core/锁文件提交同时触发两者。
- CLI CI 不编译 `hangar-gui`；GUI CI 不编译 CLI 前端，且没有额外的重复 GUI build 步骤。
- `v*` 发布产物只有 `hangar-linux-*`、`hangar-macos-*` 与 `SHA256SUMS.txt`。
- `gui-v*` 发布产物只有 Linux/macOS GUI 安装包与 `SHA256SUMS.txt`。
- GUI Release 的 `make_latest` 为 false，`releases/latest` 继续指向 CLI Release。
- Windows 仅在 CI 验证 GUI 源码，不发布 `.exe`/`.msi`。

## 非目标与已知限制

- 本任务不删除 GUI、不改变 GUI/Core 运行时代码，也不修改账号或凭据文件。
- 本任务不推送测试 tag；正式 GUI 发布仍需独立候选验收和用户确认。
- GUI 当前仍采用安装包手动更新模型；独立 GUI 版本发现机制不在本次工作流解耦范围内。

## 验收记录

### 2026-09-22

- Ruby YAML parser 与 `actionlint v1.7.12` 对四份工作流检查通过，未发现语法或 GitHub Actions 表达式错误。
- 静态契约断言通过：CLI CI 不引用 GUI；GUI CI 包含 core 触发且无额外 `cargo build`；两条 Release 各含 4 个 Linux/macOS target；CLI/GUI 分别设置 `make_latest: true/false`；GUI 发布无 Windows 资产。
- 按序执行 `cargo fmt`、`cargo clippy -- -D warnings`、`cargo test` 全绿，共 76 项测试（CLI 单元 17、CLI 集成 11、core 35、GUI 13）。
- 本轮不创建 `v*` 或 `gui-v*` 测试标签，避免产生正式 Release；发布工作流只做静态契约验证。
- 提交 `56d95bd` 推送后，[CLI/Core CI run `35699117823`](https://github.com/mocikadev/hangar/actions/runs/35699117823) 与 [GUI CI run `35699117827`](https://github.com/mocikadev/hangar/actions/runs/35699117827) 均在 Ubuntu、macOS、Windows 全绿；未触发 `Release · CLI` 或 `Release · GUI`。
