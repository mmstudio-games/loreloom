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
- [x] [Mod/Rule Spike](../spikes/0006-mod-rule.md)：验证并冻结目录包格式、依赖/Patch/哈希锁定、
  Parameter、Event Option、Gameplay Action、Rule 预算、ModLock 和资源限制。

## 后续协议与实现

- [x] 冻结 Stable ID 与 Command/Event/RecordOp 重建权威关系；
- [x] 冻结领域 record payload v1 Schema；record envelope、未知字段、迁移、重建、提交、后端和物理
  恢复顺序均已冻结；
- [ ] 为 Generated provenance 实现领域 payload v1 -> v2 连续纯 migration，并保持其它 record
  canonical 等价；
- [x] 冻结角色、物品、技能、属性、资源、Condition、正交状态、KnownFact/Goal/Transcript Schema；
- [x] 冻结 Character/Scene/Item/Skill 等 Content Definition v1 字段与迁移版本；
- [x] 冻结 Mod Manifest/ModLock、Parameter、Event、Gameplay Action 与声明式 Rule 协议边界；
- [x] 冻结 NarratorPlan/NpcTurnRequest/NpcTurnResult/NarratorSynthesis 与预算配置；
- [x] 实现 Bevy Working World 的 Stable ID 映射、typed record 投影/重建与第一阶段领域 Command；
- [x] 实现 typed ModLock/SaveManifest、SurrealKV 显式事务、Revision CAS、ActionId 幂等、checksum、
  checkpoint + RecordOp 重建和 Transcript/Event/Action durable rows；
- [x] 实现严格 Agent wire、Armillae 单次调用/Tool continuation、顺序关联、可唤醒取消与两级预算；
- [x] 实现单一 World owner 的 Runtime WorldService、candidate commit/recovery、可信 Transcript 提交与
  Content-resolved Character/Scene/UiSnapshot 投影；
- [x] 实现 Player -> Narrator planning -> ordered NPC turns -> Narrator synthesis 纵向编排，并覆盖
  stale Request、committed Event provenance、伪造 Event 拒绝、SurrealKV durable replay 与冲突恢复；
- [ ] 取得并接入可等待、幂等的 SurrealKV shutdown API，移除物理关闭/备份对 sleep/retry 的依赖；
- [ ] 实现关闭后物理备份、恢复与存档切换的产品 API；
- [x] 实现 Runtime Client/Event loop、确定性 TUI 产品层与可运行的内置 demo 装配，完成最小可玩
  Runtime、World、Agent、Store、TUI 纵向切片；
- [x] 实现正式目录 Mod package discovery、依赖/Patch/hash lock、统一内置/外部 Registry 加载与
  精确 ModLock 重开门禁；
- [x] 把内置/外部初始 Scene 统一编译为 spawn plan，并通过共享 NpcFactory/初始化提交物化，不再由
  demo 手工构造初始 Character/Item records；
- [x] 实现 Event Option、Gameplay Action 与声明式 Rule 的产品 executor、可信 capability 门禁、
  Save/Session Parameter 语义和通用 Tool；
- [x] 实现外部 Provider、严格非敏感 TOML、Environment/File Secret source、endpoint allowlist、
  Runtime/Rule/TUI budget 与无 Secret demo 回退的二进制装配；
- [x] 实现 Condition periodic/expiry scheduler、同 tick periodic-first、target Effect 与原子 Rule
  cascade；
- [x] 实现 confirmed KnownFact 驱动的 Condition 诊断投影，未诊断视图不得包含真实名称；
- [ ] 实现 Existing/Preset/Generated/Mentioned NpcTarget、Narrator generation stage 与统一物化路径；
- [ ] 满足 Active Spec 第 17 节全部验收条件。
