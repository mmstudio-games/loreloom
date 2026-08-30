# Loreloom Runtime 实施清单

> 状态：Active
> 来源：[Runtime Active Spec](../specs/runtime.md)
> 建立日期：2026-08-30

本清单只记录 Active Spec 与实现之间的差异。标记为 **GATED** 的范围必须先解决对应 Spec
OPEN 项、后续 RFC 或 P0 Spike；空 crate 不代表相关协议已经实现或冻结。

## P0：Workspace 初始化

- [x] 接受 RFC 0001，并把 Runtime Spec 转为 Active；
- [x] 创建根级 Cargo virtual workspace；
- [x] 使用 Cargo CLI 创建七个 library crate 和一个 `loreloom` binary crate；
- [x] 每个 crate 显式声明 `version = "0.1.0"`，且所有 Manifest 不含 `version.workspace`；
- [x] 使用 Rust 2024 edition、stable channel，且不声明 `rust-version`；
- [x] 初始化 Semifold Rust resolver、`.changes/` 和八个 workspace package；
- [x] 配置与 base branch `main` 不同的 Semifold `release` branch；
- [x] 创建 `mods/` 与共享测试数据目录 `tests/data/`；
- [x] 通过 fmt、Cargo metadata/check/test、Clippy 和 Semifold 状态检查。

## P0：架构 Spikes

- [x] [Store commit Spike](../spikes/0002-store-commit.md)：SurrealDB + SurrealKV 与 SQLite 对照，覆盖显式事务、Revision CAS、
  ActionId 幂等、故障注入、崩溃恢复、备份、性能、公开依赖和许可证；
- [x] [Armillae/Bevy Spike](../spikes/0001-armillae-bevy.md)：验证 Component/System、WorldGateway、Observation、容器、技能、
  Attribute、Condition/Clock 与 Revision conflict；
- [x] [TUI Spike](../spikes/0003-tui.md)：验证 Ratatui/Crossterm 双栏、多行 Unicode 输入、
  streaming、resize、窄屏降级和终端恢复，并冻结第一阶段交互边界；
- [x] [Agent Loop Spike](../spikes/0004-agent-loop.md)：验证并冻结 NarratorPlan、
  NpcTurnRequest/Result、NarratorSynthesis、严格串行、两级预算、取消和 stale Revision；
- [x] [Content/NpcFactory Spike](../spikes/0005-content-npc-factory.md)：验证并冻结预设 Definition
  与运行时 Draft 的统一 SpawnSpec/Factory 路径、两阶段跨引用、失败回滚和 GeneratedOrigin 恢复；
- [ ] **GATED** Mod/Rule：验证依赖/Patch/哈希锁定、Parameter、Event Option、Rule 预算和资源限制。

## 后续协议与实现

- [ ] 冻结 Stable ID 与 Command/Event 重建权威关系；
- [ ] 冻结领域 record Schema 与领域迁移协议；提交、后端和物理恢复顺序已由 Store Spike 冻结；
- [ ] 冻结角色、物品、技能、属性、资源、Condition 和正交状态 Schema；
- [ ] 冻结 Content/Mod/Event/Rule/Parameter/Gameplay Action Schema；
- [x] 冻结 NarratorPlan/NpcTurnRequest/NpcTurnResult/NarratorSynthesis 与预算配置；
- [ ] 实现最小可玩 Runtime、World、Agent、Store、TUI 纵向切片；
- [ ] 满足 Active Spec 第 17 节全部验收条件。
