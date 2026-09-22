# v0.5.0 CLI/TUI Execution Contract

## Status

- State: implemented and locally verified; awaiting commit
- Approved by: user
- Last updated: 2026-09-22

## Intent Lock

- Goal: 为 hangar 增加可脚本化、可验证、不会泄露凭据的一次性 CLI，并改善 TUI 配额交互。
- Non-goals:
  - 不新增 GUI 功能或 Windows GUI 发布工作。
  - 不改变账号持久化模型、S11 顺序、OAuth 常量或明文安全模型。
  - 不升级版本、打 tag 或发布。
- Success criteria:
  - 命令、JSON、selector、退出码和非交互删除契约可由自动化测试验证。
  - TUI 不再因启动全量配额查询而锁住主要交互。
  - 项目全量门禁与终端冒烟通过。

## Approved Behavior

- 无子命令保持默认 TUI/非 TTY classic；显式子命令直接执行后退出。
- 支持 list/current/switch/login/reauth/remove/quota/doctor/harvest/update/tui/classic。
- JSON 显式脱敏；selector 只接受完整 ID 或唯一邮箱；删除要求 `--yes`。
- GUI 冻结为维护模式，但共享 core 仍接受 GUI 编译/测试约束。

## Design Constraints

- Architecture: 参数、selector、输出和命令编排留在 cli crate；core 不依赖 CLI 协议。
- API/Data: 不序列化原始 `Account`；不改 accounts.json schema。
- UX/Flow: 一次性命令不隐式自升级；human stdout 与诊断 stderr 分离。
- Risk controls: 写路径只在隔离 HOME/CODEX_HOME 验证；守卫失败前不触碰官方文件。

## Task Batches

| Batch | Scope | Done When | Verification |
|-------|-------|-----------|--------------|
| 1 | 文档与边界 | Spec、计划、契约和导航同步 | diff review |
| 2 | 一次性 CLI | 全部子命令与兼容 flags 可用 | unit + isolated integration |
| 3 | TUI 配额交互 | 启动/当前/全部刷新与非阻塞交互完成 | TestBackend + terminal smoke |
| 4 | 收口 | 文档、门禁和 release build 完成 | fmt → clippy → test → build |

## Test Obligations

- 参数解析、selector 歧义、JSON 脱敏和退出码必须有测试。
- 写命令集成测试必须隔离 HOME/CODEX_HOME，禁止读取或修改真实账号。
- TUI 变化必须有 TestBackend/状态测试及终端冒烟。
- 最终运行 workspace 全量门禁，GUI 作为共享 core 回归继续参与。

## Review Gates

- [x] Batch review: spec 合规
- [x] Batch review: 代码质量
- [x] Final review: 全量 diff 与契约一致
- [x] Final verification: 验证证据已记录

## Change Handling

- 需求变化先更新 Spec、计划和本契约。
- 若实现必须改变持久化模型、OAuth 或发布范围，停止并重新确认。
