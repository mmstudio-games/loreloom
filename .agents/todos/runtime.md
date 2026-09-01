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
  resize、窄屏降级和终端恢复；产品阶段改用 Runtime thinking 状态，不展示 Provider 未完成正文；
- [x] [Agent Loop Spike](../spikes/0004-agent-loop.md)：验证严格串行、ToolCall 构造的内部 Plan、
  NpcTurnRequest/Result、两级预算、取消和 stale Revision；产品模型正文协议已改为自然语言；
- [x] [Content/NpcFactory Spike](../spikes/0005-content-npc-factory.md)：验证并冻结预设 Definition
  与运行时 Draft 的统一 SpawnSpec/Factory 路径、两阶段跨引用、失败回滚和 GeneratedOrigin 恢复；
- [x] [Mod/Rule Spike](../spikes/0006-mod-rule.md)：验证并冻结目录包格式、依赖/Patch/哈希锁定、
  Parameter、Event Option、Gameplay Action、Rule 预算、ModLock 和资源限制。

## 后续协议与实现

- [x] 冻结 Stable ID 与 Command/Event/RecordOp 重建权威关系；
- [x] 冻结领域 record payload 初始 v1 Schema；record envelope、未知字段、重建、提交、后端和物理
  恢复顺序均已冻结；首个公开版本前直接压平破坏性修改并拒绝旧开发数据；
- [x] 删除开发期领域 payload v1 -> v2 与 Save Format v2 兼容路径；当前 Save、领域 payload 与内容
  Schema 均直接使用初始 v1，连续 migration 基础设施只为首个公开版本后的契约保留；
- [x] 冻结角色、物品、技能、属性、资源、Condition、正交状态、KnownFact/Goal/Transcript Schema；
- [x] 冻结 Character/Scene/Item/Skill 等 Content Definition v1 字段与迁移版本；
- [x] 冻结 Mod Manifest/ModLock、Parameter、Event、Gameplay Action 与声明式 Rule 协议边界；
- [x] 把模型正文协议收敛为自然语言，并由原生 ToolCall 构造内部 NarratorPlan/NpcTurnRequest；
- [x] 实现 Bevy Working World 的 Stable ID 映射、typed record 投影/重建与第一阶段领域 Command；
- [x] 实现 typed ModLock/SaveManifest、SurrealKV 显式事务、Revision CAS、ActionId 幂等、checksum、
  checkpoint + RecordOp 重建和 Transcript/Event/Action durable rows；
- [x] 实现严格 Agent wire、Armillae 单次调用/Tool continuation、顺序关联、可唤醒取消与两级预算；
- [x] 实现单一 World owner 的 Runtime WorldService、candidate commit/recovery、可信 Transcript 提交与
  Content-resolved Character/Scene/UiSnapshot 投影；
- [x] 实现 Player -> Narrator planning -> ordered NPC turns -> Narrator synthesis 纵向编排，并覆盖
  stale Request、committed Event provenance、伪造 Event 拒绝、SurrealKV durable replay 与冲突恢复；
- [ ] **UPSTREAM-GATED**：SurrealDB SDK 暴露可等待、幂等的 embedded shutdown 后接入，移除物理
  关闭/备份对 sleep/retry 的依赖；上游已确认当前 SDK 无法等待 local router/datastore 退出；
- [ ] **UPSTREAM-GATED**：实现关闭后物理备份、恢复与存档切换的产品 API；
- [ ] **RELEASE-GATED**：在实际分发前确认 Loreloom 采用与 `toasty-driver-surreal`
  `AGPL-3.0-only` 兼容的分发许可，或为该依赖取得兼容的重新许可；该选择不改变当前 Runtime、
  Store Schema 或存档格式；
- [x] 实现 Runtime Client/Event loop、确定性 TUI 产品层与目录根世界装配，完成最小可玩 Runtime、
  World、Agent、Store、TUI 纵向切片；
- [x] 实现正式目录 Mod package discovery、依赖/Patch/hash lock、统一内置/外部 Registry 加载与
  精确 ModLock 重开门禁；
- [x] 把当前 WorldLock/ModLock 精确相等门禁替换为候选内容协调；Prompt-only、纯增量
  Definition/Mod 与未被引用内容的移除通过内存重建和领域校验后可原子更新 SaveManifest Lock，缺失
  实际依赖时保持原存档不变并报告稳定 Mod/Definition ID；暂不实现通用 Definition migration；
- [x] 把根世界/外部 Mod 的初始 Scene 统一编译为 spawn plan，并通过共享 NpcFactory/初始化提交物化，
  不在应用代码中手工构造初始 Character/Item records；
- [x] 实现 Event Option、Gameplay Action 与声明式 Rule 的产品 executor、可信 capability 门禁、
  Save/Session Parameter 语义和通用 Tool；
- [x] 实现外部 Provider、严格非敏感 TOML、Environment/File Secret source、endpoint allowlist 与
  Runtime/Rule/TUI budget；生产二进制要求 Provider 配置，Mock Bridge 只存在于测试；
- [x] 提高单 Agent Turn 与完整 PlayerInput 编排的默认 Model/Tool/Token/时间预算，保证默认档支持
  多次 Tool continuation 和多轮 Narrator/NPC 调度，并让 Rust 默认值与示例配置保持一致；
- [x] 实现 Condition periodic/expiry scheduler、同 tick periodic-first、target Effect 与原子 Rule
  cascade；
- [x] 实现 confirmed KnownFact 驱动的 Condition 诊断投影，未诊断视图不得包含真实名称；
- [x] 实现 Existing/Preset/Generated/Mentioned NpcTarget、Narrator generation stage、统一物化路径，
  并在 Preset/Generated 提交后于同一玩家输入内使用完整角色投影重新规划；
- [x] 用 `create_npc { source, lifetime, mode }` 取代模型侧 NarratorNpcDecision 交叉组合，保持内部
  Preset/Generated 两阶段物化与重规划；
- [x] 把默认 GenerationPolicy 与生成用 AgentProfile 移入根世界锁定内容，由 Runtime 注入当前
  Scene/Place，移除产品 Provider 配置中的世界生成策略；
- [x] 收敛 `npc_generation` 为首次成功 `submit_npc_draft` 即结束的单用途 Tool stage，移除 Draft
  wire 和模型 payload 中的 AgentProfile/GenerationPolicyId，允许省略空集合，并以不回显参数的
  字段类别诊断替代笼统 `invalid_input`；
- [ ] 把成功的 `create_npc` 收敛为 Narrator Turn barrier，跳过无用自然语言 continuation 与同一
  响应内后续 ToolCall，释放执行槽后立即物化并基于 committed Observation 重规划；
- [x] 把 `request_npc_turn` 压平为 ActorId + assignment，在 Observation 投影
  `npc_turn_available`，并让 AgentRunner/UiSnapshot/TUI 保留脱敏 Tool 拒绝码；
- [x] 暴露有界 Stable ID 游标分页的 `list_inventory`/`inspect_item` 与
  `list_available_skills`/`inspect_skill` Query Tool，并把 `transfer_item`、`equip_item`、
  `split_stack`、`use_skill` 接到已冻结的 WorldCommand；
- [x] 实现 Scene 停用/重新激活与切换产品入口；Scene、Scene-owned entity 和状态不因离开而删除，
  `promote_npc` 只改变角色的领域归属；
- [x] 为 Narrator 提供当前 Revision 的 canonical Scene 切换目标查询，拒绝猜测或过期目标，并让
  重复的同目标请求幂等收敛；
- [x] 把 Place Definition edge 物化为同 Scene 双向 ObjectId 连接并约束普通移动；实现 Narrator-only
  延迟 `create_scene`/`create_place`、GeneratedOrigin、原子 Command/Event/RecordOp 与创建后重规划；
- [x] 实现根级 `world.toml`、外部 Content/Prompt、WorldLock 与只含已启用扩展的 ModLock；
- [x] 统一根世界与 Mod 的 `[prompts]` Narrator/NPC 全局上下文声明，按根世界、依赖拓扑和列表顺序
  注入 Agent，并覆盖哈希、分流与未声明资源不注入测试；
- [x] 删除 Manifest/Agent/Runtime 的独立响应语言配置与 System Message，把固定或跟随语言完全交给
  World/Mod Narrator/NPC Prompt；
- [x] 把目录 Mod 根级 `.gitignore` 作为不参与 payload/hash 的本地作者元数据跳过，保持其它未知文件
  与隐藏路径的严格拒绝策略；
- [x] 把 Rainbound Inn 从 `demo.rs` 迁移到根目录世界文件，并移除生产 Demo Bridge/英文剧情硬编码；
- [x] 把 Spec 8.2 的 Character/Scene/Transcript 数量与字节上限接入 Narrator/NpcAgent 产品上下文，
  并在裁剪时投影 `truncated` metadata；
- [x] 用 Runtime phase/status event 驱动 TUI thinking 展示，并移除 Provider 文本 streaming 产品协议；
- [x] 在 Tool 执行前后发布安全的当前 Turn Activity，实时更新 TUI pending/终态状态并保持玩家输入、
  Tool Activity、Narrator 正文的显示顺序；
- [x] 完成面向叙事阅读的 TUI 视觉重构；
- [x] 修复 Transcript 底部锚定、按折行 viewport 约束的 PageUp/PageDown 与鼠标滚轮滚动；
- [x] 在玩家输入被 Runtime command queue 接受后立即显示本地 pending 玩家行，并由最终 Snapshot
  确定性替换或清除；
- [x] 保留 Armillae 模型失败的脱敏 category、阶段、安全 Provider 元数据与 `err_` correlation ID，
  贯通启动、AgentRunner、NpcTurnResult、Runtime、TUI notice 和 headless 错误；
- [x] 为二进制 Provider 装配增加启动前预检与安全诊断，区分 Narrator/NPC、稳定 setup code、环境
  变量引用和本地修复提示，且不保留 Secret 或 Armillae 原始错误正文；
- [x] 扫描世界根 `mods/` 的有效 installed package，把主世界、enabled/installed Mod 与 unavailable
  汇总投影到 `UiSnapshot`，并实现支持独立滚动、以 `Ctrl+O`/`F2` 为可靠入口且兼容 macOS
  `Option+M` 的 Mods overlay；
- [x] 从启用编译候选和未启用目录检查生成非持久化 `PackageContentView`，在 Mods overlay 展示每个
  Mod 的顶层 Definition 总数/分类以及 Prompt/Patch 数量，并覆盖 enabled 优先合并和滚动边界；
- [x] 完成 Active Spec 第 17 节实施审计：除显式 `UPSTREAM-GATED` 的第 47 条和上述分发许可选择外，
  其余验收均有自动化或可复现证据；当前门禁为 174 tests passed，目录根世界的创建、锁分离、
  Prompt 注入与存档内容锁重开拒绝均有确定性测试。macOS arm64 release 基线仍为 81.2 MiB。
