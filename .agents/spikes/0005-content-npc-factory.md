# P0 Spike 0005：Content Registry 与 NpcFactory

> 状态：Completed
> 开始日期：2026-08-30
> 完成日期：2026-08-30
> 规范来源：[Runtime Active Spec §10.2](../specs/runtime.md#102-modcontent-packagespawnspec-与运行时生成)

## 目标

验证外部预设 Character/Scene Definition 与运行时模型生成的 NpcDraft 可以在不直接反序列化 ECS
Component 的前提下，纯编译为同一 `CharacterSpawnSpec`，再经同一 NpcFactory/World transaction
完成校验、Stable ID 分配、跨对象引用解析、提交和持久恢复。

## 候选边界

- Content Pack 先收集所有 Definition ID，再解析引用；完整 Pack 只有全量通过后才能发布新
  immutable Registry；
- Character Definition、Scene Definition 和运行时 Draft 都不能携带 Bevy Entity、Component
  patch、Provider Client、Tool Handler 或任意脚本；
- 预设输入产生 `ContentOrigin`，运行时输入产生 `GeneratedOrigin`，其余领域字段汇入同一
  `CharacterSpawnSpec`；
- NpcFactory 在 candidate world 中先为全部 spawn entry 分配 Stable ObjectId，再解析 local key 与
  existing ObjectId 引用；任一失败丢弃整个 candidate，包括 ID allocator 推进；
- Generated NPC 成功后保存完整领域状态、GeneratedOrigin 与当前内容锁；Load 只读存档，不再次
  调用 Generator/LLM；
- 本 Spike 使用测试专用拥有所有权数据模型，不冻结 Stable ID 编码、Fixed 数值或最终文件格式。

## 验收

- [x] Pack 两阶段加载支持前向引用，并拒绝重复 ID、缺失/错误种类引用；
- [x] 失败 Pack 不改变已发布 Registry，成功 Registry 的规范顺序不依赖输入文件顺序；
- [x] Character Definition 与 NpcDraft 纯编译为同一 CharacterSpawnSpec 字段与 validation path；
- [x] Factory 同时验证 Agent Profile、Scene、Attribute budget/range、Resource、Condition、Item、
  Skill、Goal 与来源；
- [x] Scene spawn plan 先分配所有 ObjectId，再解析角色间关系等跨对象引用；
- [x] 任一 spawn/ref/invariant 错误使 candidate world、事件和 ID allocator 全部回滚；
- [x] 成功的 preset/generated spawn 使用相同 World event 和领域实例形状，但保留不同 Origin；
- [x] GeneratedOrigin 和完整 NPC 状态 JSON round-trip 后可恢复，Generator 调用计数不增加；
- [x] 内容版本或 hash 与 save lock 不匹配时迁移或拒绝，不静默换用当前 Definition；
- [x] 测试不访问网络、真实 Provider、Store 后端或 Bevy World；
- [x] 记录统一 Schema、失败边界、仍由 Mod/Rule Spike 决定的文件格式与最终结论。

## 禁止提前冻结

Spike 类型只能位于测试。通过后可以把逻辑字段、事务阶段和 Origin/恢复语义同步回 Active Spec，
但不得在 Stable ID、Fixed、内容文件格式和领域子 Schema 各自解除门禁前建立公共 Rust API 或长期
持久化格式。

## 自动化证据

测试文件：`crates/loreloom-content/tests/content_npc_factory_spike.rs`。

| 测试 | 证据 |
|---|---|
| `pack_load_is_two_phase_deterministic_and_atomic_on_reference_failure` | 先收集全部 ID 后解析 Character/Scene 前向引用；输入顺序变化得到同 Registry/plan；缺失 Skill、Item 被当作 Skill 的错误 kind、跨 kind 重复 ID 均拒绝且 published Registry 不变 |
| `preset_and_generated_inputs_share_spawn_spec_and_factory_validation` | Content Definition 与 NpcDraft 机械字段汇入同一 SpawnSpec；同 Factory 生成相同事件/实例形状；Generated attribute budget 越界使整个 candidate 回滚 |
| `factory_allocates_all_ids_before_relationship_resolution_and_rolls_back` | local key 规范排序后先分配两个 ID，再解析 Mira -> Tomas；缺失 local target 发生在分配后仍回滚 Character、Event、关系和 ID allocator |
| `existing_object_relationship_is_checked_inside_the_same_candidate` | existing Stable ObjectId 可以解析，缺失 ObjectId 使本次 candidate 全部回滚 |
| `generated_origin_and_complete_state_restore_without_generator_call` | GeneratedOrigin、属性、Resource、Condition、Inventory、Skill、Goal 与 World JSON round-trip；load API 不接收 Generator，计数保持 1 |
| `save_content_lock_mismatch_is_rejected_instead_of_using_new_definitions` | 相同存档对不同 content hash 返回 ContentLockMismatch，对原锁精确恢复 |

验证命令：

```sh
cargo test -p loreloom-content --test content_npc_factory_spike
cargo clippy -p loreloom-content --all-targets -- -D warnings
```

结果：6 passed，Clippy 无警告。

## 冻结结论与剩余边界

- Registry 发布、Scene materialization 与 World spawn 是三个清晰边界；前两者纯编译，最后一个在
  candidate world transaction 中执行。失败不得泄漏 ID 或部分对象；
- Definition 与 Draft 不是两个 Factory API；二者先归一为 CharacterSpawnSpec。Origin 不改变领域
  校验，只改变 provenance 与恢复检查；
- GenerationPolicy 是 Runtime 可信输入，NpcDraft 不能扩大预算或 capability；
- Generated NPC 持久化完整状态，load 不调用 Generator。它仍受引用内容的 ModLock 约束；
- 本 Spike 不选择 TOML/JSON 等物理文件格式，不冻结 Stable ID 编码或 Fixed；这些继续由对应门禁
  和 Mod/Rule Spike 决定。

## 最终结论

Spike 通过。统一 SpawnSpec/Factory、Pack Registry 原子发布、两阶段 ObjectId/reference、完整回滚、
Content/Generated Origin 和无模型 load 已同步回 Active Spec。测试专用类型不升级为公共 API。
