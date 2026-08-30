# Loreloom 设计索引

> 状态：Discovery
> 更新日期：2026-08-30
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
  发布顺序；
- Loreloom Content 拥有版本化 Mod/Content Package、Definition/Rule Schema、依赖与跨引用验证，
  以及到领域 SpawnSpec/规则计划的纯编译，不依赖 Bevy、LLM 或存储后端，也不直接修改
  Working World；
- Loreloom World 拥有领域 Component、Resource、System、声明式规则执行和事件实例，不调用
  LLM、不渲染 UI；
- `NpcFactory` 属于 World 领域边界，消费已编译 SpawnSpec，结合当前世界校验属性预算、Scene、
  领域引用和不变量后形成 WorldCommand；Capability、数量和模型预算仍由 Runtime 校验；
- Persistence 保存稳定逻辑 ID 和拥有所有权的版本化记录，不序列化 Bevy 内部身份或内存布局；
  当前首选候选为通过 Toasty 使用嵌入式 SurrealDB + SurrealKV，SQLite 保留为 P0 Store Spike 的
  对照后端；该候选定位不等同于冻结最终 Store；
- Armillae 不反向依赖 Loreloom，也不承担 Loreloom 的 Agent、Memory、存档或 UI 策略。

## 3. 权威工程文档

| 文档 | 类型与状态 | 权威范围 |
|---|---|---|
| [RFC 0001：Loreloom 架构](rfcs/0001-loreloom-architecture.md) | Draft RFC | 产品边界、ECS 权威状态、Agent/Tool 流程、持久化方向、TUI 和 crate 候选拆分 |
| [Runtime Spec](specs/runtime.md) | Proposed Spec | 可审查的第一阶段规范候选；RFC 接受并转为 Active 前不授权实现 |
| [TODO 索引](TODO.md) | Blocked | 只说明当前没有可实施清单 |

当前没有 Active Spec 或 Accepted RFC，因此没有产品代码实施入口。

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
10. 主 `NarratorAgent` 负责解释玩家交互意图，并决定 NPC 是仅叙事提及、实体化、由 Narrator
    代理、规则控制还是请求独立 NPC Turn，以及建议其 Scene/World 生命周期；Runtime 不重新判断
    叙事重要性，只负责 Schema、Revision、Capability、预算和世界不变量；
11. `NpcAgent` 是一次 NPC Turn 的临时对象，由 Agent Definition、不可变
    `CharacterContext`、`SceneContext` 和 `NpcAssignment` 构造；持久角色只保存 ECS 状态和
    Agent Binding；
12. 共享 `AgentRunner` 拥有 Bridge 与 Tool 执行能力，Agent 对象本身不持有 Provider Client、
    可变 World 或可持久化的运行服务；
13. 预设 NPC/Scene 从版本化 Mod Package 中的 Content Pack 导入，运行时 NPC 由 Narrator 提出
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
18. 内置内容与外部 Mod 使用同一版本化 Mod Package、Definition Registry、SpawnSpec、Factory
    和持久化路径，不为外部内容建立旁路；
19. 第一阶段 Mod 分为纯数据 Content Mod 与声明式 Rule Mod；任意代码执行属于后续 Extension
    Mod，不直接加载 Rust 原生动态库；
20. Event Definition 的触发、可见/可选条件、选项和效果必须结构化；选择 Event Option 时携带
    Revision 并重新校验，LLM 文本不能替代实际效果；
21. 模组自定义角色参数使用版本化 `ParameterDefinition` 和受限类型值，不使用无 Schema JSON
    或动态 ECS Component 名称；
22. 特殊玩法优先表达为 `Trigger -> Predicate -> Effect -> WorldCommand`，数据 Mod 只能调用引擎
    注册的白名单效果和通用 Gameplay Tool，不能自行注册任意 Tool 或扩大 Capability；
23. 存档固定 Mod ID、版本、内容哈希和依赖闭包；缺失或不兼容内容必须迁移或拒绝加载，不能
    依赖模糊加载顺序静默覆盖 Definition；
24. 持久化后端以 SurrealDB + SurrealKV 作为首选候选，SQLite 作为对照；候选 driver 已具备显式
    顶层事务、原生 JSON 列和迁移跟踪/安全范围自动生成，但 Loreloom 只有在 Store Spike 验证
    原子提交、Revision CAS、崩溃恢复、备份和性能后才能冻结选择；
25. Loreloom 的 durable commit 必须使用显式事务，不得把普通 ORM batch 当作原子事务；数据库
    Schema migration 也不能替代领域 record、ModLock 和 payload Schema 的版本迁移；
26. 第一阶段只有一个 Agent Loop 执行槽：NarratorAgent 与 NpcAgent、以及不同 NpcAgent 之间严格
    串行；`NpcTurnRequest` 可以按 Narrator 给出的顺序排队，但下一个 Agent 只能在前一个完成/
    取消/失败并释放执行槽后开始，并从届时有效的 committed Revision 重新校验和投影上下文；
27. 第一阶段的自然语言玩家输入只交给 NarratorAgent；Narrator 负责结合 Scene/Context 生成
    `NarratorPlan`，决定是否以及按什么顺序请求 NPC Turn，Runtime 不实现叙事优先级或公平性判断；
28. NpcAgent 的发言、意图和动作描述先形成有界 `NpcTurnResult` 返回 Narrator；真正的世界变化只
    来自已执行 ToolCall/WorldCommand，Narrator 必须以实际 ToolResult/WorldEvent 为准形成
    `NarratorSynthesis`，不能把 NPC 声称的行动当作已发生事实；
29. Narrator 不受固定 NPC 数量常量约束，可以在 Synthesis 后提出下一轮有序 `NpcTurnRequest`；
    Runtime 通过可配置的整轮与单 Turn Model Call、ToolCall、Token、输出和墙钟预算阻止无限循环，
    模型或 Mod 不能扩大配置上限；
30. 等待 Provider 时 TUI 流式显示和取消保持响应，但第一阶段逻辑 World Clock 不随真实墙钟时间
    隐式推进；世界只通过明确 WorldCommand/System 变化。

项目方已于 2026-08-29 明确确认第 18–23 项的 Mod 子系统方向。该确认把 Content Mod、Rule Mod、
统一导入路径、类型化参数、结构化 Event Option、通用 Gameplay Tool、ModLock 和 Extension Mod
隔离作为后续 Schema 的约束，但不等同于接受整份 RFC 或授权实现。

项目方于 2026-08-30 同意把第 24–25 项记录为持久化候选上下文。这只确认候选排序、显式事务
边界和验证门槛；最终后端、Store Schema、ECS/Store 提交顺序与故障恢复协议仍保持 OPEN。

项目方同日确认第 26 项的第一阶段串行 Agent 调度边界。该次确认当时尚未冻结调度触发源、
语义顺序、数量和预算，也未决定等待 Provider 时世界是否继续推进；这些问题随后由第 27–30
项进一步解决。

项目方随后确认第 27–30 项的 Narrator 编排模型，并采用 `NpcTurnRequest`/`NpcTurnResult` 术语
取代早期的 NPC 激活请求表述。这次确认冻结玩家入口、语义调度所有权、Narrator Synthesis、配置化
资源边界和 Provider 等待期间暂停逻辑世界；精确数据 Schema、预算字段与默认值仍待 Spec 冻结。

## 5. 仍待接受或独立冻结的事项

- 第一阶段全部领域类型的精确 Schema，包括 Attribute ID/Fixed 数值、Resource 最大值变化、
  Modifier 聚合、Condition Stack/Duration 和正交状态机；
- Item/Skill Definition 的内容包版本、堆叠等价规则和 Skill Executor 数据驱动边界；
- Content Pack 文件格式、Definition ID、CharacterSpawnSpec、导入事务和 Generated Origin；
- Mod Package Manifest、命名空间、依赖解析、显式 Patch、内容哈希、信任来源和资源限额；
- Event/Rule/Parameter Schema、Predicate/Effect 白名单、规则执行顺序和 Gameplay Action 协议；
- Extension Mod 是否采用 WASM Component、Host API、Capability、签名与存档兼容边界；
- NarratorNpcDecision 的精确 Schema、SceneScoped 清理条件、Agent 化预算和持久引用升级规则；
- SurrealDB + SurrealKV 与 SQLite 对照 Spike 的最终结论，以及 Store Schema、备份、存档切换、
  性能、依赖发布和许可证结论；
- 领域 record/ModLock/payload Schema 的版本迁移、迁移校验与未知字段策略；
- ECS 执行与显式 Store 事务之间的提交顺序、提交失败、结果不确定和崩溃恢复语义；
- 一个玩家输入、Agent Step、ToolCall、WorldCommand 和世界提交之间的原子性；
- `NarratorPlan`、`NpcTurnRequest`、`NpcTurnResult`、`NarratorSynthesis` 的精确 Schema，整轮/单
  Turn 预算字段、配置层级、默认值和最大编排轮数；
- 角色私有知识、长期记忆、对话归档和上下文摘要的结构与规模上限；
- TUI 的窄屏降级、快捷键、流式输出和后台任务交互细节；
- 初始世界内容、玩法循环和可发布范围。

## 6. 文档推进顺序

1. 审查 RFC 0001 的核心边界和待决问题；
2. 用户确认后把 RFC 状态改为 Accepted；
3. 根据确认结果修改 Proposed Runtime Spec 并转为 Active；
4. 建立 Runtime TODO，把门禁拆成 Spike、协议、测试和实现任务；
5. 使用 Cargo CLI 创建 workspace/crates，再开始代码实现。

RFC 接受之前，允许继续修改设计文档和进行只读技术调查，不允许以候选接口创建实现。
