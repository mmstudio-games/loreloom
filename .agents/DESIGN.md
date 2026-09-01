# Loreloom 设计索引

> 状态：核心架构已接受；全部 P0 Spike 已完成，MVP 基础协议开始冻结
> 更新日期：2026-09-01
> 作用：Loreloom 的权威工程设计入口，不在本文件重复 RFC 或 Spec 的细节

本目录按成熟度区分 `rfcs/` 中的架构提案、`specs/` 中的工程契约和未来 `todos/` 中的实施
差异。根目录 `docs/` 保留给公共接口冻结后的玩家与开发者文档；当前阶段不创建稳定版指南。

## 1. 产品方向

Loreloom 是一个 Rust 开发、运行在终端中的 Agentic 大世界游戏。玩家通过自然语言与世界和
角色交互；NPC 可以由 LLM 驱动，但游戏不是纯聊天包装。影响模拟、规则判断、可见信息或未来
行为的事实必须存在于结构化的 ECS 领域状态中，并通过显式 Tool 查询和修改。

项目使用公开的 [Armillae](https://github.com/mmstudio-games/armillae) Rust 基础设施。Armillae
提供通用的 Agentic 叙事与世界模拟能力，Loreloom 只通过可公开解析的 Cargo 依赖组合它：

- Armillae 提供 Bevy ECS Simulation、一次 Provider 无关的 Model Call 和一次 ToolCall 执行；
- Loreloom 提供具体游戏领域、Agent Harness、上下文组织、Tool 策略、持久化、玩家会话和 TUI；
- 模型负责受约束的解释、选择、对话和创造性表达，不承担权威数据库职责。

Loreloom 是独立项目。其构建、测试、文档和发布必须自包含，所有依赖必须通过公开来源解析。

项目名来自 Lore 与 Loom：ECS 状态提供经纬，Agent 与玩家共同编织持续演化的叙事。

## 2. 分层与依赖方向

```text
Loreloom TUI
      │ 输入 / UiSnapshot
      ▼
Loreloom Runtime ───────────────► Persistence
      │                               ▲
      ├──► Agent Harness              │ versioned records
      │      ├──► armillae-llm        │
      │      ├──► armillae-llm-rig    │
      │      └──► armillae-tools      │
      │
      ├──► Loreloom Content
      │      └──► Loreloom Core
      │             definitions / rule plans / spawn specs
      │
      └──► Loreloom World ────────────┘
             ├──► Loreloom Content
             ├──► armillae-simulate
             ├──► armillae-simulate-bevy
             └──► compatible bevy_ecs
```

依赖与所有权约束：

- TUI 只通过 Runtime API 发送输入并读取不可变 `UiSnapshot`，不直接查询或修改 Bevy World；
- Agent Harness 只接收针对某个角色和世界版本的 `Observation`，通过受限 Tool 请求命令；
- Runtime 是应用级协调者，拥有 Agent Step、Tool Loop、Mod 加载图、世界请求、提交、取消和 UI
  发布顺序；它隔离尚未 durable commit 的 ECS candidate，提交失败或结果不确定时从 Store 重建；
- Loreloom Content 拥有版本化 Mod/Content Package、Definition/Rule Schema、依赖与跨引用验证，
  以及到领域 SpawnSpec/规则计划的纯编译，不依赖 Bevy、LLM 或存储后端，也不直接修改
  Working World；
- Loreloom World 拥有领域 Component、Resource、System、声明式规则执行和事件实例，不调用
  LLM、不渲染 UI；
- `NpcFactory` 属于 World 领域边界，消费已编译 SpawnSpec，结合当前世界校验属性预算、Scene、
  领域引用和不变量后形成 WorldCommand；Capability、数量和模型预算仍由 Runtime 校验；
- Agent Harness 把 Armillae `BridgeError` 投影为只含 correlation ID、调用阶段、归一化类别和安全
  Provider 元数据的诊断；Runtime 必须保持该诊断到模型、玩家和进程错误投影，不能把所有失败折叠
  成不可判断的 `bridge_unavailable`，也不能传播 Provider 原始错误正文；
- 二进制装配层在 World/Save 打开前预检 Narrator/NPC Provider，并把启动失败投影为包含 Provider
  槽位、稳定 setup code、安全 Provider 名称、必要时的环境变量名和本地修复提示的诊断；不得通过
  解析 Armillae 自由文本错误来猜测原因，也不得显示 Secret 值或凭证文件内容；
- Persistence 保存稳定逻辑 ID 和拥有所有权的版本化记录，不序列化 Bevy 内部身份或内存布局；
  第一阶段固定为通过 Toasty 使用嵌入式 SurrealDB + SurrealKV，SQLite 只保留为提交契约测试
  对照；durable unit 必须使用显式事务；
- Armillae 不反向依赖 Loreloom，也不承担 Loreloom 的 Agent、Memory、存档或 UI 策略。

## 3. 权威工程文档

| 文档 | 类型与状态 | 权威范围 |
|---|---|---|
| [RFC 0001：Loreloom 架构](rfcs/0001-loreloom-architecture.md) | Accepted RFC | 产品边界、ECS 权威状态、Agent/Tool 流程、持久化方向、TUI 和 crate 拆分 |
| [RFC 0002：根世界与 Mod](rfcs/0002-root-world-and-mods.md) | Accepted RFC | 唯一根世界、Mod 扩展、世界 Prompt 与 WorldLock |
| [Runtime Spec](specs/runtime.md) | Active Spec | 第一阶段工程约束与范围化实施门禁 |
| [TODO 索引](TODO.md) | Active | 路由到从 Active Spec 派生的实施清单 |

当前允许按 Runtime TODO 初始化 workspace、空 crate、版本工具与 P0 Spike；仍标记 OPEN/GATED 的
范围在独立冻结前没有产品 API 实施入口。

## 4. 当前设计结论

以下方向直接来自项目方本轮输入，视为 RFC 的已知约束，但 RFC 内的完整细节仍需整体审查：

1. 产品为 Rust TUI 大世界游戏，不是纯聊天前端；
2. 世界执行基于 Bevy ECS，并组合 Armillae 的 Simulation、LLM Bridge 与 Tool 能力；
3. 原本可能交给模型记忆的持久事实应尽量建模为 ECS Component、Resource、Relation 或
   versioned record；
4. 数据操作封装为 Tool，模型不得通过自由文本直接改世界；
5. TUI 使用左右布局：左侧展示角色和世界状态，右侧展示叙事/对话，右侧底部提供输入框；
6. Loreloom 跟随最新 stable Rust 与 Rust 2024 edition，不定义 MSRV，也不声明
   `rust-version`；
7. 物品采用“静态 Item Definition + ECS Item Instance + System 规则”模型，容器关系是背包内容
   的单一事实源，不在角色 Component 中维护重复物品列表；
8. 技能采用“静态 Skill Definition + ECS Skill Grant + System Executor”模型，主动技能通过
   类型化 Tool/WorldCommand 使用，被动与反应技能由明确的规则入口驱动；
9. Agent 上下文只接收背包摘要、当前可用技能和按需查询结果，不直接持有 ECS 集合或完整内容库；
   Runtime 为当前 ToolContext Actor 暴露有界、Stable ID 游标分页的物品/技能 Query，并只把冻结的
   Item/Skill WorldCommand 作为 Command Tool；查询不能借由任意 Actor 参数越过 Actor 所有权；
10. 主 `NarratorAgent` 负责解释玩家交互意图，并决定是否创建 NPC、创建后的 Scene/World
    生命周期、由 Narrator 叙述还是使用独立 NpcAgent，以及是否请求已有 NPC Turn；模型侧只使用
    `create_npc` 与 `request_npc_turn` 两个正交 Tool，不直接操作 Runtime 的物化状态机；Runtime
    不重新判断叙事重要性，只负责注入 Scene/Place/Revision、解析世界 GenerationPolicy 与
    AgentProfile，并校验 Capability、预算和世界不变量；
11. `NpcAgent` 是一次 NPC Turn 的临时对象，由 Agent Definition、不可变
    `CharacterContext`、`SceneContext` 和 `NpcAssignment` 构造；持久角色只保存 ECS 状态和
    Agent Binding；
12. 共享 `AgentRunner` 拥有 Bridge 与 Tool 执行能力，Agent 对象本身不持有 Provider Client、
    可变 World 或可持久化的运行服务；
13. 预设 NPC/Scene 从根世界或已启用 Mod 的版本化 Content Pack 导入，运行时 NPC 由 Narrator 提出
    受限 Generation Request；两条路径都必须转换为同一 `CharacterSpawnSpec` 并经
    `NpcFactory`/WorldCommand 创建，不能直接 Patch ECS；
14. 角色属性采用“Attribute Definition + BaseAttributes + 来源明确的 Modifier + 派生
    EffectiveAttributes”模型，派生值可缓存但不作为存档事实；
15. Health、Stamina 等当前资源独立于基础属性保存；状态效果采用“Condition Definition +
    Condition Instance”，使用 World Clock、来源与明确堆叠规则；
16. LifeState、ActionState、Posture 等互相正交的状态机不得压入一个互斥 CharacterState；人格
    描述属于 CharacterProfile，不与机械属性混用；
17. Narrator 可以在 NPC 创建时提供 Attribute/Condition Hint，但 NpcFactory 必须校验预算、
    上限和 Definition；NPC 创建后只能由 Tool/WorldCommand/System 改变权威属性或状态；
18. 主游戏是根目录 `world.toml` 描述的唯一 World，不是内置 Mod；根世界与已启用 Mod 使用同一
    Definition Registry、SpawnSpec、Factory 和持久化提交路径，不为任一来源建立旁路；
19. 第一阶段 Mod 分为纯数据 Content Mod 与声明式 Rule Mod；任意代码执行属于后续 Extension
    Mod，不直接加载 Rust 原生动态库；
20. Event Definition 的触发、可见/可选条件、选项和效果必须结构化；选择 Event Option 时携带
    Revision 并重新校验，LLM 文本不能替代实际效果；
21. 模组自定义角色参数使用版本化 `ParameterDefinition` 和受限类型值，不使用无 Schema JSON
    或动态 ECS Component 名称；
22. 特殊玩法优先表达为 `Trigger -> Predicate -> Effect -> WorldCommand`，数据 Mod 只能调用引擎
    注册的白名单效果和通用 Gameplay Tool，不能自行注册任意 Tool 或扩大 Capability；
23. 存档用 WorldLock 记录根世界身份、版本与内容哈希，并用 ModLock 单独记录已启用 Mod 的版本、
    哈希和依赖闭包；Lock 差异只表示候选内容发生变化，必须先用候选 Registry 重建并校验存档，不能
    直接等同于不兼容。Prompt-only、纯增量内容与未被引用内容的移除应允许重开并原子采用新 Lock；
    缺失或不兼容的实际依赖必须迁移或精确拒绝，不能依赖模糊加载顺序静默覆盖 Definition；
24. 第一阶段持久化后端固定为 SurrealDB + SurrealKV，SQLite 只作为测试对照；SurrealDB driver
    固定到公开 Git revision，已通过 Store Spike 验证显式事务、原生 JSON、migration tracking、
    Revision CAS、崩溃恢复和 10,000 Record 规模；确定性关闭、物理备份/恢复与存档切换受已确认的
    SurrealDB SDK 上游缺口门禁，不使用 sleep/retry 伪造完成；
25. Loreloom 的 durable commit 必须使用显式事务，不得把普通 ORM batch 当作原子事务；数据库
    Schema migration 也不能替代领域 record、ModLock 和 payload Schema 的版本迁移；
26. 第一阶段只有一个 Agent Loop 执行槽：NarratorAgent 与 NpcAgent、以及不同 NpcAgent 之间严格
    串行；`NpcTurnRequest` 可以按 Narrator 给出的顺序排队，但下一个 Agent 只能在前一个完成/
    取消/失败并释放执行槽后开始，并从届时有效的 committed Revision 重新校验和投影上下文；
27. 第一阶段的自然语言玩家输入只交给 NarratorAgent；Narrator 通过 Provider 原生 Tool Calling
    请求 NPC Turn，并以自然语言正文提供叙事；Runtime 从已接受 ToolCall 构造内部 `NarratorPlan`，
    不解析模型正文中的 JSON，也不实现叙事优先级或公平性判断；
28. NpcAgent 的自然语言响应与实际 ToolResult/WorldEvent 一起形成内部 `NpcTurnResult` 返回
    Narrator；真正的世界变化只来自已执行 ToolCall/WorldCommand，NPC 或 Narrator 的正文不能直接
    改变 ECS；
29. Narrator 不受固定 NPC 数量常量约束，可以在后续 Narrator Turn 继续调用
    `request_npc_turn`；Runtime 通过可配置的整轮与单 Turn Model Call、ToolCall、Token、输出和墙钟
    预算阻止无限循环，模型或 Mod 不能扩大配置上限；
30. 玩家输入被 Runtime command queue 接受后，TUI 立即在 thinking 状态之前显示一条本地 pending
    玩家行；该行不修改 committed Transcript，并由后续权威 Snapshot 对账或清除。等待 Provider 时
    TUI 显示由 Runtime 阶段驱动的临时 thinking 状态，并保持取消和退出响应；不向玩家转发 Provider
    的未完成正文。Runtime 在已解析 ToolCall 开始执行前和获得 ToolResult 后发布只含 call ID、名称、
    状态与脱敏错误码的临时 Tool Activity；参数与结果不进入 UI，Activity 不进入 Transcript 或存档。
    第一阶段逻辑 World Clock 不随真实墙钟时间隐式推进，世界只通过明确 WorldCommand/System 变化。
31. 第一阶段使用 Cargo virtual workspace，包含 `loreloom-core`、`loreloom-content`、
    `loreloom-world`、`loreloom-agent`、`loreloom-store`、`loreloom-runtime`、`loreloom-tui` 七个
    library crate 和 `loreloom` binary crate；
32. 每个 crate 在自己的 Manifest 中显式声明版本，禁止使用 `version.workspace`；项目不声明
    `rust-version`，使用 Semifold 的 Rust resolver 和 `.changes/` 变更集管理各 crate 版本；
    Semifold base branch 为 `main`，release branch 为非 `main` 的 `release`；
33. 仓库根目录的 `world.toml`、`content/`、`rules/` 与 `prompts/` 属于唯一主世界；`mods/` 只放
    主世界扩展，共享测试数据位于 `tests/data/`；
34. 第一阶段 WorldCommand 先在唯一世界槽中把 ECS 从 committed Revision N 修改为隔离的
    candidate N+1，再用 expected Revision N 显式提交 Store durable unit；只有 commit 成功才发布
    ToolResult/UiSnapshot，失败或不确定时丢弃 candidate 并从 Store 重建；
35. 新世界从 Revision 0 开始。ActionId 在单 Save 内幂等；ActionCommit 与 RecordOp、WorldEvent、
    Transcript 和 Save Head 同事务提交，重复相同请求返回已保存结果，不产生第二组 durable rows。
36. 运行时生成的 Stable ID 使用带三字母类型前缀的 canonical lowercase UUIDv7；ActorId 是
    ObjectId 的语义新类型，共享同一 `obj_` 身份。Mod ID 使用 reverse-DNS lowercase 标识，
    Content Definition ID 使用 `mod-id:kind/local-key`，不与运行对象 ID 混用。
37. 存档重建的唯一事实源是“最近完整 checkpoint + 其后的连续有序 RecordOp”。WorldCommand
    只作为已接受输入和幂等摘要保存，WorldEvent 只作为已发生事实、叙事 provenance 与审计记录；
    Load/Replay 不重新执行 Command、Rule、Agent 或 Provider。
38. 领域 record 使用拒绝未知控制字段的 versioned JSON envelope；当前 payload codec 拒绝未知
    字段和浮点数。首个公开版本前只有当前 v1，旧开发数据直接拒绝；发布后的旧版本只能通过逐版本、
    纯确定性的显式 migration 升级。新版本、未知 record type、迁移缺口或数据库 `NONE` 必须在物化
    World 前失败。
39. 第一阶段所有机械数值使用全局 scale 为 `1_000_000` 的 signed i64 Fixed；中间计算使用 i128，
    乘除采用 ties-to-even，任何最终越界都拒绝完整 candidate。WorldTime 是从 0 开始、只由显式
    Command 推进的逻辑秒 tick。
40. 持久领域状态使用 typed aggregate records：World、Place/Scene、Character、Item、
    Condition、SkillGrant、Relationship、KnownFact、Goal、EventInstance、ParameterSet、RuleState 与
    Transcript。派生属性、背包列表、可用技能和 UI 文本不保存；Generated provenance 的 tagged
    source 直接属于初始 payload v1，不为更早的开发期表示保留兼容分支。
41. Content 与运行时生成共享 Core 拥有的 `CharacterSpawnSpec`；Content 拥有 Definition/NpcDraft
    Schema 与纯编译器，World 拥有结合当前状态校验并执行的 NpcFactory。Content document v1 拒绝
    未知字段；首个公开版本前直接更新 v1，发布后才按 content schema version 显式迁移。
42. 纯叙事提及不调用 Tool、也不生成 Entity；Scene 与 Persistent 使用同一完整 Character
    record，`create_npc` 的 narrated mode 只是没有 AgentBinding 的 Narrator controller。Scene、其状态与
    Scene-owned entity 都是存档中的持久事实；离开 Scene 只停用，重新进入时恢复，不因离开或故事
    阶段结束而 cleanup。promotion 只表达角色脱离原 Scene、成为世界级角色的领域语义。
43. Agent 长期上下文不增加隐藏 Memory 数据库：KnownFact/Goal 是决策事实，Transcript 是有状态的
    对话归档。第一阶段只做确定性、可配置的有界投影，不调用摘要模型；Narrator/NPC 文本不会自动
    写入 KnownFact。
44. 模型侧 `create_npc` 只区分 Preset 与 Generated source；Existing 只进入
    `request_npc_turn`，Mentioned 是不产生世界事实的普通叙事而不是 Tool target。Generated NPC 的
    Draft 使用现有 Narrator Provider 的独立 generation stage，并通过该 stage 提供的原生 Tool 提交，
    不从模型正文解析 JSON；它消费同一整轮编排预算，不增加隐式第三 Provider。默认
    GenerationPolicy 由根世界锁定，Agent mode 的 AgentProfile 由 Preset Character Definition 或
    GenerationPolicy 解析，Narrator 不提交这些内部 ID。Generated provenance 明确引用触发本次生成
    的 PlayerInput Transcript 或 WorldEvent。
45. Condition Clock 在 periodic 与 expiry 同 tick 时先执行 periodic，再重新校验并执行 expiry；周期
    Effect 作用于 Condition target。诊断不写回 Condition，而使用观察者拥有、以目标 Actor 为 subject、
    Condition Definition 为 value 的 confirmed KnownFact 决定是否投影真实名称。
46. Runtime 只从已接受的 `request_npc_turn { actor_id, assignment }` ToolCall 构造引用 committed
    `ActorId` 的内部 `NarratorPlan`；Scene 与 Revision 来自 ToolContext 和当前 World，不能由模型
    提交。Observation 对当前可调度角色显式投影 `npc_turn_available`，同一 Revision 中使用该
    ActorId 的请求不得再被隐藏条件拒绝。Narrator 调用 `create_npc` 请求 Preset/Generated NPC 时，
    当前 Turn 先结束，Runtime 才能串行完成
    编译或 `npc_generation`、经统一 `CharacterSpawnSpec -> SpawnCharacter` 路径提交；随后 Runtime
    在同一次玩家输入中把物化结果和更新后的 Scene Observation 交给 Narrator。Narrator 看过完整
    角色投影后才能为其调用 `request_npc_turn`；内部物化状态、Place、GenerationPolicy、AgentProfile、
    角色重要度和可选复核不进入模型 wire。
47. `transition_scene` 是 Narrator 专用的延迟编排 Tool：它只接受已提交 Scene ObjectId 或当前
    ModLock 的 Scene Definition ID，当前 Narrator Turn 结束后才执行单个原子 `TransitionScene`
    Command。切换同时停用旧 Scene、激活或首次物化目标 Scene、移动现有玩家并产生 SceneLeft/
    SceneEntered Event；随后必须基于新 Revision 重新调用 Narrator，不能沿用切换前的 NPC 请求。
48. Runtime 对 TUI 只发布粗粒度 `RuntimePhase` 与安全的当前 Turn `ToolActivity`，不转发 Provider
    stream 或内部隐藏推理；phase 固定覆盖 PersistingInput、NarratorThinking、
    ResolvingOrchestration、NpcThinking、UpdatingWorld 与终态。Phase/Tool Activity event 不改变
    committed Snapshot；Tool Activity 在执行前为 Pending，结束后原位收敛为 Succeeded、Rejected 或
    Failed，且不携带 Tool 参数或 ToolResult。
49. Transcript 默认锚定最新内容；本地滚动状态表达“距最新内容的视觉行数”，按当前宽度折行后的
    内容高度和 viewport 约束。`PageUp`/鼠标滚轮向上查看旧内容，`PageDown`/鼠标滚轮向下返回最新；
    位于底部时新 Snapshot 自动跟随，滚动和 resize 都不触发 Runtime 请求或修改世界。
50. 每次模型调用失败生成 `err_` UUIDv7 correlation ID；Agent Harness 丢弃 Armillae/Provider 原始
    错误正文，只保留 invocation、失败阶段、归一化类别、经过约束的 Provider/Request ID、HTTP
    状态与 retry 元数据。Runtime、NpcTurnResult、TUI notice 和 headless 错误复用同一诊断事实，
    启动期 Bridge 创建失败使用同一安全投影。
51. Narrator 在请求 Scene 切换前必须通过只读 `list_scene_transitions` 查询当前 Revision 的 canonical
    目标；该 Tool 只返回 inactive 的已物化 Scene ObjectId 和尚未物化的已注册 Scene Definition ID，
    排除当前 Scene 并避免同一 Definition 的重复目标。`transition_scene` 只接受查询返回的精确 target，
    拒绝结果提供可操作的恢复方向；Narrator 不得在提交成功前叙述已经抵达，也不得原样重试已拒绝
    的请求。查询无匹配目标时表示当前内容不可达，不隐式生成新 Scene。
52. 根世界通过 `[prompts]` 分别拥有有序的 Narrator 与 NPC 基础 Prompt，启用的 Mod 可用相同结构
    按依赖拓扑和声明顺序追加两类全局 Prompt；引擎只在其前保留不可覆盖的 Tool/ECS/安全协议，
    Mod Prompt 不能扩大 Tool Capability。回复语言完全由 World/Mod Prompt 表达，Manifest、Agent
    协议和 Runtime 不拥有、检测或追加独立语言策略。Prompt hash 可以作为 WorldLock/ModLock 的内容
    provenance，但 Prompt 变化不得独自阻止读档。初始 Save Format v1 直接保存 WorldLock 与只含已
    启用扩展的 ModLock，不为开发期存档增加 Schema 兼容分支。Rainbound Inn 必须从
    Rust 硬编码迁移到根目录内容文件；生产二进制不使用硬编码 Demo Bridge 生成剧情。
53. 首个公开版本发布前，Save Format、领域 payload 和内容 Schema 的破坏性修改直接压平进各自的
    初始 v1；开发期产物直接拒绝并重建，不分配 v2、不注册兼容迁移，也不保留 legacy load 分支。
    连续 migration 基础设施只为首个公开版本之后的已发布数据契约保留。
54. Scene 是可反复进入且持久保存的叙事/生命周期容器，World 恰好有一个 active Scene；Place 是
    Scene 内 Character 实际共处和移动的节点，Character.location 只能指向 Place。Place 以持久化、
    同 Scene、双向 `edges` 表达可达关系，普通 `move_character` 只能沿连接移动，Scene 切换仍通过
    目标 `entry_place` 完成。Narrator 可以通过原生 Tool Calling 延迟请求 `create_scene` 或
    `create_place`：前者原子创建 inactive Scene 与必需 entry Place，后者在 active Scene 创建 Place
    并与玩家当前 Place 建立双向边；Runtime 分配全部 Stable ID、绑定 Scene、保存 GeneratedOrigin，
    创建后强制重规划且不自动切换或移动。NpcAgent 不拥有改变世界拓扑的 Capability；预设与运行时
    生成的 Scene/Place 都是存档事实，离开后不删除。
55. Provider 启动诊断由 Loreloom 二进制装配层拥有：它必须区分 `narrator`/`npc`，使用稳定的
    setup code 标识缺失、空或不可读取的 credential reference、endpoint policy、Provider 支持与
    Bridge 创建失败，并给出不含 Secret 的本地修复提示。环境变量名可以作为安全配置引用显示；
    Secret 值、凭证文件内容和 Armillae 原始错误正文不得进入 Display、Debug、日志或测试输出。
56. TUI 提供只读 Mods overlay，分别显示唯一主世界、当前 `ModLock` 中已启用的扩展，以及世界根
    `mods/` 下通过 Package 安全校验但未启用的目录包；内置 engine package 不作为 Mod 展示。每个
    Mod 同时显示由 Content 编译/检查路径生成的顶层 Definition 分类计数，以及 Manifest 声明的
    Prompt 文件和 Patch 数量；摘要不包含包内正文、路径、字节数或嵌套 Event/Effect 数量。`Ctrl+O`
    与 `F2` 是可靠入口，继续兼容终端可区分的 `Alt+M` 和 macOS 默认 `Option+M`；`Esc` 只关闭
    overlay，方向键、PageUp/PageDown 与滚轮滚动。安装目录和摘要只在启动时扫描，展示不启用 Mod、
    不修改存档，也不进入 ECS、Agent Context 或 Transcript。

项目方已于 2026-08-29 明确确认第 18–23 项的 Mod 子系统方向。该确认把 Content Mod、Rule Mod、
统一导入路径、类型化参数、结构化 Event Option、通用 Gameplay Tool、ModLock 和 Extension Mod
隔离作为后续 Schema 的约束，但不等同于接受整份 RFC 或授权实现。

项目方于 2026-08-30 同意把第 24–25 项记录为持久化候选上下文，并随后明确第一阶段使用
SurrealKV。Store/Armillae P0 Spike 完成后，第 24、34、35 项已同步为 Active Spec 约束；生产
Store Schema、领域迁移和 AGPL 兼容分发方式仍按各自门禁处理。

项目方同日确认第 26 项的第一阶段串行 Agent 调度边界。该次确认当时尚未冻结调度触发源、
语义顺序、数量和预算，也未决定等待 Provider 时世界是否继续推进；这些问题随后由第 27–30
项进一步解决。

项目方随后确认第 27–30 项的 Narrator 编排模型，并采用 `NpcTurnRequest`/`NpcTurnResult` 术语
取代早期的 NPC 激活请求表述。这次确认冻结玩家入口、语义调度所有权、Narrator Synthesis、配置化
资源边界和 Provider 等待期间暂停逻辑世界；精确数据 Schema、预算字段与默认值仍待 Spec 冻结。

项目方于 2026-08-31 进一步明确模型正文在所有 Provider 上都必须保持自然语言，不使用模型生成的
JSON Plan/Synthesis/NpcDraft；结构化操作统一通过 Provider 原生 Tool Calling。该决定取代上段的
模型 Synthesis wire，但保留 Narrator 编排所有权、NpcTurnRequest 语义顺序、串行执行与两级预算。
同日确认 Scene 离开后持久停用而非删除，并以 Runtime thinking phase 取代 Provider 未完成正文展示。

项目方于 2026-08-30 接受 RFC 0001 的核心架构并授权进入实施初始化，同时确认第 31–33 项的
workspace、版本管理和仓库目录约定。尚未冻结的精确协议转为 Active Spec 下的范围化实施门禁，
不得由空 crate 脚手架反向冻结公共 API。

## 5. Active 基线下仍需独立冻结的事项

- 已冻结第一阶段领域 record、Fixed、Attribute/Resource/Condition、物品/技能、正交状态、
  KnownFact/Goal/Transcript、Content Definition v1 与 CharacterSpawnSpec；后续新增类型必须走新的
  schema version/migration，不能扩展 v1 未知字段；
- Mod Package Manifest、命名空间、依赖解析、显式 Patch、内容哈希、信任来源和资源限额；
- Event/Rule/Parameter Schema、Predicate/Effect 白名单、规则执行顺序和 Gameplay Action 协议；
- Extension Mod 是否采用 WASM Component、Host API、Capability、签名与存档兼容边界；
- `create_npc`/`request_npc_turn` 的简化 Narrator Tool 面、内部 NPC 物化状态、持久 Scene 激活语义、
  Agent 化资源门禁和角色 promotion 规则已冻结；Generated Draft 复用 Narrator Provider 的独立预算
  阶段，具体预算值是 Host 配置，不进入模型/Mod wire；
- Store 各领域 payload Schema，以及最终 AGPL 兼容分发方式；record envelope、重建事实源、
  migration 顺序与未知字段策略已冻结；
- 一个玩家输入、Agent Step、ToolCall、WorldCommand 和世界提交之间的原子性；
- ToolCall 构造的内部 `NarratorPlan`、`NpcTurnRequest`、自然语言 `NpcTurnResult`，以及整轮/单
  Turn 预算字段、配置层级、默认值和最大编排轮数；模型正文不承担这些结构化协议；
- 角色私有知识、Goal、Transcript 与确定性上下文投影已冻结；摘要模型延期；
- TUI 的窄屏降级、快捷键、thinking 状态和后台任务交互细节；
- 初始世界内容、玩法循环和可发布范围。

## 6. 后续推进顺序

1. 从 Active Runtime Spec 建立并维护 Runtime TODO；
2. 初始化 Cargo workspace、Semifold 与不暴露公共领域 API 的空 crate；
3. 完成 Store、Armillae/Bevy、TUI、Agent Loop、Content/NpcFactory 和 Mod/Rule P0 Spike；
4. 对仍标记 OPEN 的协议建立后续 RFC 或明确冻结记录；
5. 只有对应实施门禁解除后，才在相关 crate 中建立公共 API 与产品实现。
