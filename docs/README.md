# hangar 技术文档总览

> 面向开发者与 AI 协作。用户使用说明请看根目录 [README.md](../README.md)。

## 阅读顺序（新人）

1. [project-spec.md](requirements/project-spec.md) — 做什么、不做什么、验收标准
2. [architecture.md](design/architecture.md) — 分层架构与关键机制
3. [scenarios.md](design/scenarios.md) — S1-S12 场景风险矩阵
4. [test-strategy.md](quality/test-strategy.md) — 测试策略与验收清单
5. [execution-protocol.md](ai/execution-protocol.md) — AI 协作执行协议（人类可跳过）

## 按目录

### 需求（requirements）

| 文档 | 内容 |
|------|------|
| [project-spec.md](requirements/project-spec.md) | 目标、范围、非目标、验收标准 |

### 设计（design）

| 文档 | 内容 |
|------|------|
| [architecture.md](design/architecture.md) | 分层架构、模块边界、harvest/切换/原子写/锁/OAuth 等关键机制 |
| [scenarios.md](design/scenarios.md) | S1-S12 令牌一致性场景、风险与方案、安全声明 |
| [update-mechanism.md](design/update-mechanism.md) | 版本发布与自升级机制设计（CI/升级流程/双前端接入） |
| [update-plan.md](design/update-plan.md) | 自升级实施计划（已执行完毕，留档备查） |

### 质量（quality）

| 文档 | 内容 |
|------|------|
| [test-strategy.md](quality/test-strategy.md) | 测试策略、单测布局、验收清单 |

### AI 协作（ai）

| 文档 | 内容 |
|------|------|
| [execution-protocol.md](ai/execution-protocol.md) | 任务分级、实现硬约束、验证要求、风险操作清单 |

## 从源码构建（仅开发者）

```bash
cargo build --release  # 产物 target/release/hangar（约 2.3MB）
cargo test              # 单测（core + cli）
```

提交前按顺序跑：`cargo fmt` → `cargo clippy -- -D warnings` → `cargo test`。
