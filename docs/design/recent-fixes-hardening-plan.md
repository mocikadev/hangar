# 近期修复收口任务

> 2026-09-25；范围仅为 `afec5ba`、`d982a6b`、`9651cf3`、`149d904` 后续审计发现的回归风险。不升级版本、不打 tag、不发布或推送。测试使用隔离 `HOME`/`CODEX_HOME`；macOS GUI 只在当前 macOS 宿主构建。

## 决策与顺序

1. HTTP 解析器返回 IPv4 地址在前、IPv6 地址在后，保留同族内原有顺序。`ureq` 2.12.1 在 TCP 连接失败时会尝试下一个地址；本次不更改超时、代理、OAuth 或凭据写入规则。
2. Xcode 与绑定生成脚本使用同一个 Cargo 产物目录。默认仍为仓库 `target/`，允许通过 Xcode 构建设置 `CARGO_TARGET_DIR` 显式覆盖；不从固定路径静默链接旧 `.a`。
3. `resources/shared/hangar-icon.svg` 继续作为完整图标的规范源，Icon Composer 前景 SVG 从其中的白色标志生成。保留旧 AppIcon asset catalog 供低版本兼容核查；未有 macOS 14 真机证据前不删除。

## 任务清单

- [x] H01 双栈网络回退：混合、仅 IPv4、仅 IPv6、同族顺序测试先红后绿；本机 IPv4 失败→IPv6 回环 HTTP 请求通过，暂时恢复旧过滤行为后测试失败，再恢复通过。解析器只排序，不丢弃有效地址。
- [x] H02 静态库路径一致：Xcode 的 `CARGO_TARGET_DIR` 同时控制绑定生成、Debug/Release 链接路径；默认 Debug 与全新自定义目录 Release 均构建成功，两个 App 的 `otool -L` 均无 `libhangar_uniffi.dylib`。macOS CI 已增加自定义目录构建和依赖回读，远端运行待推送后确认。
- [x] H03 图标来源一致：生成脚本从完整 SVG 同步 Icon Composer 前景，`--check-source` 在人为制造漂移时失败、恢复后通过；重新生成没有资产字节差异。旧 AppIcon asset catalog 继续保留，macOS 14/x64 真机未验证。
- [x] H04 本机总体验收：隔离 `HOME`/`CODEX_HOME` 依次执行 `cargo fmt`、`cargo clippy --locked -- -D warnings`、`cargo test --locked`，93 项测试通过；Xcode 工程 `plutil`、CI YAML 解析、图标来源检查通过。未启动应用、未读写真实账号、未创建 DMG；远端 CI 和 macOS 14/x64 仍未验证。

## 非目标

- 不改变账号库、官方 `auth.json`、Token 刷新、额度缓存或推荐规则。
- 不实现 Linux/Windows GUI，不从本机构建结果推断其可用。
- 不生成新的对外 DMG，也不宣称 macOS 14/x64 已通过运行验收。
