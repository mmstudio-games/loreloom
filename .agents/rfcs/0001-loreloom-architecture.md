# RFC 0001：Loreloom 持久化 Agentic 世界与 TUI 架构

> 状态：Accepted
> 接受日期：2026-08-30
> 更新日期：2026-08-31
> 设计入口：[Loreloom 设计索引](../DESIGN.md)
> 规范：[Loreloom Runtime Active Spec](../specs/runtime.md)
> 基础设施依赖：[Armillae](https://github.com/mmstudio-games/armillae)

本 RFC 提议 Loreloom 第一阶段的整体架构：以 Bevy ECS 工作世界承载结构化游戏事实，以
Armillae 提供 Simulation、单次 LLM 调用和单次 Tool 执行能力，由 Loreloom Runtime 组合玩家
输入、Agent Step、世界命令、持久化与 TUI。

本文的核心架构已经项目方接受，MUST 级行为由 Active Runtime Spec 约束。第 14 节尚未冻结的
精确协议是范围化实施门禁：它们不阻止 workspace 与空 crate 初始化，但在独立冻结前不得据此
创建相应公共 API、持久化格式或产品行为。

## 1. 背景与问题

酒馆式 AI 角色应用擅长开放对话和角色扮演，但常把人物属性、关系、物品、地点、已知事实、
目标、任务状态和世界时间都塞进 Prompt 或聊天历史。这样会产生一组系统性问题：

- 上下文窗口不是数据库，旧事实会被裁剪、摘要、遗忘或互相冲突；
- 模型可以用自然语言声称状态已经改变，却没有可验证的规则执行；
- 每轮重复注入完整设定会提高延迟和 Token 成本；
- 角色知道什么、世界真实发生了什么、玩家看见什么容易混为一谈；
- 存档、回放、调试、测试和迁移都依赖不可重复的模型输出；
- Tool 只作为提示词装饰时，模型仍可能绕过权限和业务不变量。

Loreloom 的目标不是消除 LLM，而是把模型放在它擅长的位置：理解自然语言、生成对话、在受限
候选中选择行动、形成计划和提供创造性表达。结构化事实、规则、权限、提交和恢复则由正常的
游戏系统负责。

## 2. 目标

1. 建立一个可以长期运行、保存、加载和演化的 Agentic 游戏世界。
2. 使用 Bevy ECS Component、Resource、关系实体和 System 表达游戏事实与规则。
3. 让影响未来模拟或决策的状态脱离 Prompt，成为可查询、验证、版本化的领域数据。
4. 用显式 Tool 把模型请求转换为查询或类型化世界命令，而不是直接暴露可变 ECS。
5. 组合 Armillae 的单次 Model Call、单次 Tool 执行和 Bevy Simulation，不改变其底层边界。
6. 支持一个负责叙事/调度的 NarratorAgent、多个按需调用的 NPC Agent、玩家控制角色和纯规则
   控制实体共存。
7. 提供类似 Codex 的 TUI：左侧状态，右侧叙事/对话，右下输入。
8. 为确定性测试、存档重建、失败恢复和审计提供清晰边界。
9. 允许从外部 Mod Package 导入 NPC、Scene、事件选项、类型化角色参数和受限特殊玩法。
10. 跟随最新 stable Rust，不维护 MSRV。

## 3. 非目标

- 复制某个现有酒馆产品的 Prompt、角色卡格式或 UI；
- 把所有叙事创作都改成预写脚本或确定性规则；
- 让 LLM 自由执行文件、Shell、网络、付费 API 或其它外部副作用；
- 第一阶段支持实时多人、服务器权威同步或分布式世界；
- 第一阶段支持图形渲染、物理、音频或完整 Bevy App；
- 把向量数据库、Embedding、RAG 或“无限记忆”设为核心前置条件；
- 序列化原始 Bevy `World`、`Entity`、Schedule 或 Rust 内存布局；
- 让 Armillae Bridge 变成自动 Agent、自动 Tool Loop 或 Memory 框架；
- 第一阶段加载 Rust 原生动态库、执行任意 Mod 脚本或开放不受限的文件/网络能力；
- 在本 RFC 中冻结具体存储引擎、世界内容或完整玩法数值。

## 4. 术语

| 术语 | 含义 |
|---|---|
| World Fact | 影响规则、可见性、未来决策或叙事一致性的结构化事实 |
| Working World | 当前由 Bevy ECS 承载、供 System 查询和修改的活动世界 |
| Stable ID | 跨进程、存档和 ECS 重建仍保持一致的领域身份，不等于 Bevy `Entity` |
| Observation | 针对一个 Actor、一个世界版本生成的只读上下文投影 |
| Actor | 能发起游戏行动的玩家角色、Agent 角色或规则驱动实体 |
| Agent Step | 一次 Observation 到模型响应或 ToolCall 的受预算执行步骤 |
| NarratorAgent | 解释玩家输入、调用受控 Tool，并以自然语言正文向玩家叙事的主 Agent |
| NarratorPlan | Runtime 从 Narrator 已接受 ToolCall 构造的内部有序 NpcTurnRequest 队列 |
| NpcAgent | 由不可变角色/场景上下文按 Turn 创建的一次性 NPC 执行对象 |
| NpcTurnRequest | NarratorPlan 中请求一个指定 NPC 在指定 Scene 执行一次 Turn 的有序记录 |
| NpcTurnResult | 一次 NPC Turn 的自然语言响应及其实际 ToolResult/WorldEvent 关联 |
| AgentRunner | 共享 Bridge、ToolExecutor、预算与取消状态机的 Agent 执行服务 |
| Tool | 向模型暴露的带 Schema 能力；分为 Query、Command、Orchestration 和受控 Service |
| WorldCommand | 经过身份、权限、参数和前置条件校验的类型化修改请求 |
| WorldEvent | 世界已经接受并提交的结构化事实 |
| Revision | 标识一个已提交世界版本的单调逻辑版本 |
| Mod Package | 外部或内置内容的版本化分发、依赖、完整性与能力申请单位 |
| Content Pack | Mod Package 中的静态 Definition 与实例模板集合 |
| Rule Bundle | Mod Package 中声明式 Trigger/Predicate/Effect 与 Gameplay Action 的集合 |
| Definition Registry | 通过全包校验后供 World 只读消费的版本化 Definition 集合 |
| Transcript | 面向玩家的对话/叙事记录；不是世界事实的替代品 |
| UiSnapshot | 从已提交世界版本生成、只供 TUI 渲染的不可变 View Model |

## 5. 提议的核心决策

### 5.1 ECS 是活动事实源，不是模型缓存

运行中的游戏事实以 Working World 为准。任何满足以下条件的数据都不能只留在 Prompt、
Transcript、模型隐藏推理或临时 Agent 对象中：

- 改变 System 的规则判断或数值结果；
- 决定 Actor 能否看到、知道、拥有或操作某个对象；
- 会被后续 Agent、玩家或存档加载继续引用；
- 需要参与冲突检测、权限、任务进度或关系演化；
- 需要测试、迁移、回放或调试。

第一阶段候选领域面包括：

| 领域 | ECS 表达方向 |
|---|---|
| 身份 | `PersistentId`、`EntityKind`、`DisplayName` |
| 空间 | `Location`、`ContainedBy`、连接和可达关系 |
| 角色 | `CharacterProfile`、属性、需求、状态效果、能力 |
| 物品 | Item 实体、所有权/容器关系、数量与装备状态 |
| 社交 | 关系实体、Faction Membership、承诺与声望 |
| 认知 | Actor 私有的 Known Fact、Belief、Source 与置信信息 |
| 意图 | Goal、Plan、待执行 Intent 与控制器状态 |
| 叙事 | Quest/Scene 状态、结构化 Flag、已提交 WorldEvent |
| 时间 | 世界 Clock、日程、持续时间与到期条件 |

这些名称是领域候选，不在 Draft RFC 中冻结 Rust 类型。重点是不把“模型提到过”误当成“世界已
记录”。

Transcript 可以持久化，并可参与近期上下文；但若一段对话产生了会影响未来行为的事实，例如
承诺、任务、物品转移或角色新获知的信息，相应事实必须由 Tool 转换为结构化状态。摘要也只是
投影；只有被明确接纳并带来源的数据才能成为 World Fact。

#### 5.1.1 物品、容器与装备

物品使用“静态定义 + ECS 实例 + System 规则”三层模型：

- Item Definition 属于版本化内容，描述名称、标签、堆叠上限、基础重量和默认能力；
- 每件或每组实际存在的物品是具有 Stable ID 的 ECS Entity，通过 `ItemInstance` 引用定义；
- 数量、耐久、自定义名称和其它会变化的数据属于实例 Component；
- Container 本身也是具有 Stable ID 的实体，可以属于角色、地点或另一件物品；
- 物品通过唯一的 `ContainedBy` 关系表达物理位置，角色上的 `InventoryOwner` 只指向根容器；
- `OwnedBy`、`ContainedBy` 和 `Equipped` 分别表达所有权、物理位置和使用状态，不得互相替代；
- 背包列表通过 Query 从 `ContainedBy` 推导，不在 Character Component 中维护第二份
  `Vec<ItemId>`。

嵌套容器必须拒绝循环。堆叠拆分会产生新的 Stable ID；只有定义和所有影响行为的实例状态都
等价时才能合并。总重量、剩余容量、装备加成等派生结果由 System 计算，不作为独立权威事实
重复保存。

Agent 默认只取得有界 Inventory Summary；检查、转移、装备和使用物品通过 Query/Command Tool
完成。模型文本或自造 Item ID 不能改变物品关系。

#### 5.1.2 技能、授予关系与执行

技能同样分为三层：

- Skill Definition 属于版本化内容，描述技能类型、消耗、目标规则、冷却和稳定 Executor ID；
- 角色学会、装备授予或临时获得技能时，创建具有 Stable ID 的 Skill Grant 实体；
- 等级、熟练度、来源、启用状态和独立冷却属于 Skill Grant 或其关联 Component；
- Executor ID 在启动时映射到已注册的规则 System/handler，持久化数据不保存函数指针或
  Provider 类型。

技能至少区分 Active、Passive 和 Reaction：

- Active 由玩家或 Agent 通过 `UseSkillCommand` 显式请求；
- Passive 由确定性的领域 System 参与计算，不需要 Agent ToolCall；
- Reaction 只能由明确的事件窗口和规则入口触发，不能因为角色拥有技能就启动隐藏 Agent Loop。

`use_skill` Tool 只接收 Skill Grant 和目标等领域参数；Actor、Action、Revision 和 Capability
来自可信 ToolContext。World 在执行前验证归属、启用状态、资源、冷却、目标、距离和角色状态，
成功后扣除资源、更新基于 World Clock 的冷却并产生结构化 WorldEvent。模型不能仅凭名称使用
未被授予的技能。

Observation 只投影当前可用技能的有界视图；完整说明通过 `list_available_skills`、
`inspect_skill` 等 Query Tool 按需取得。

#### 5.1.3 属性、资源、Condition 与角色状态

角色机械状态必须拆为四层：

1. Attribute Definition 与持久化 `BaseAttributes` 表达相对稳定的基础能力；
2. Health、Stamina、Mana 等 `ResourcePool` 保存频繁变化的 current/base maximum；
3. 装备、技能、Condition 和永久调整提供来源明确的 Modifier；
4. `EffectiveAttributes`、最终最大资源、可用行动和 UI 文本属于派生结果，可缓存但不作为存档
   事实。

Attribute ID 必须来自版本化注册表，值使用可确定性重放的数值表示，不能把任意 JSON 当作属性。
Modifier 使用规范化顺序：Base -> Flat Addition -> Multiplication -> Override -> Clamp；同阶段
通过 priority 和稳定 source ID 排序。装备或状态变化后重新求值，不能让多个 System 分别写入
互相覆盖的“最终属性”。

Resource 的 current 是持久事实；最终 maximum 可以由 base maximum 与 Modifier 推导。Maximum
下降时 current 的 clamp、按比例缩放或临时超限行为由 Resource Definition 明确，不能由 UI 或
Agent 猜测。

状态效果使用“Condition Definition + 具有 Stable ID 的 Condition Instance”：

- Definition 定义 tags、Stack Policy、Duration Policy、Modifier 和稳定 Executor ID；
- Instance 保存 target、source、stacks/intensity、applied_at 和 expires_at；
- Duration 基于 World Clock 或领域 Clock，不使用进程墙钟；
- Stack Policy 至少明确 Unique、Refresh、IncreaseStacks 或 IndependentInstances；
- Condition 的周期效果和过期由显式 System/Clock 入口处理，不启动隐藏 Agent Loop。

Alive/Downed/Dead、Idle/Acting/Waiting 和 Standing/Sitting/Prone 等互相独立的状态分别使用
`LifeState`、`ActionState`、`Posture` 等正交状态机，不合并成一个无法表达并存状态的
`CharacterState`。CharacterProfile 保存背景、价值观和说话风格；只有确实参与规则的特征才
通过 Trait/Modifier 影响机械属性。

Narrator 可以在创建 NPC 时提供 Attribute 和初始 Condition Hint，但 NpcFactory 必须校验
Archetype、属性预算、上下限、资源范围和允许的 Condition。NPC 已进入世界后，Narrator 文本
不能直接 set attribute/health/condition；变化必须由受控 Tool、WorldCommand 和规则 System
产生。

真实 Condition 与角色认知必须分离。例如 ECS 可以存在 Poisoned，而尚未诊断的 NPC Observation
只投影“头晕、体力下降”等症状；确认中毒后再写入 Known Fact。

### 5.2 全局真相、角色认知与展示文本分离

Loreloom 必须至少区分三层：

1. 世界真相：实际位置、所有权、角色属性、事件结果等；
2. Actor 认知：某个角色知道、相信、误解或尚未发现的内容；
3. 展示投影：本轮允许放入 Observation 或 UiSnapshot 的内容。

模型不能因为系统 Prompt 中存在某项全局数据就自动“知道”它。Observation Builder 必须根据
Actor、位置、感知、关系、知识来源、权限和当前 Revision 投影可见子集。TUI 同样不得因方便
调试而默认泄漏 NPC 私有认知；调试视图需要显式模式。

### 5.3 Loreloom Runtime 拥有应用级推进

Armillae 不拥有 Loreloom 的游戏主循环。Runtime 决定：

- 接受哪一条玩家输入或世界触发；
- 执行哪个 Narrator/NPC Turn；
- 在哪个 Revision 上构建 Observation；
- 是否发起一次 Model Call；
- 按什么策略暴露 Tool；
- ToolCall 的顺序、预算、取消和错误反馈；
- 何时请求 Simulation 执行；
- 何时形成持久化提交并发布新的 UiSnapshot。

World 只执行已经明确请求的入口或 Command，不会因 Component 改变自行调用 LLM。等待 Provider
响应时世界也不会隐藏推进；如果未来允许后台世界继续运行，必须通过显式 Scheduler 和
Revision 冲突策略设计。

#### 5.3.1 NarratorAgent、NpcAgent 与 AgentRunner

多 Agent 第一阶段采用明确的 Narrator 编排边界：

- 自然语言 `PlayerInput` 只进入主 `NarratorAgent`。Narrator 观察当前 Scene、已提交
  WorldEvent 和玩家输入，通过 Provider 原生 Tool Calling 请求结构化编排或世界操作，并以自然
  语言正文提供最终叙事；Runtime 不解析模型正文中的 JSON 或其它控制协议；
- Runtime 按已接受 `request_npc_turn { actor_id, assignment }` ToolCall 的顺序构造内部
  `NarratorPlan`。Scene、Revision 与 request identity 由 Runtime 注入；它决定 NPC 是否
  需要独立 Turn、需要哪些 NPC 以及语义执行顺序；Runtime 不再实现叙事优先级、公平性或“重要
  NPC”判断；
- `NarratorAgent` 不在 Tool Handler 内同步创建或递归执行另一个模型。Runtime 依次校验请求中的
  NPC、Scene、Revision、Capability 和资源上限，再按 Narrator 给出的顺序放入显式临时队列；
- 每个请求真正开始时，Runtime 从届时有效的 committed Revision 重新生成不可变
  `CharacterContext`，从结构化 `SceneState`/`DirectorState` 生成 Actor 可见的
  `SceneContext`，再组合 Narrator 给出的 `NpcAssignment`；
- `NpcAgent` 是一次 NPC Turn 的临时对象，通过
  `NpcAgent::new(agent_definition, character_context, scene_context, assignment)` 构造；它不
  持有 ECS 引用，也不作为角色存档；
- 每个 NPC Turn 返回内部 `NpcTurnResult`，包含一段自然语言响应、结束状态，以及实际
  ToolResult/WorldEvent 的关联；NPC 声称要做或已经做的动作本身不是 World Fact；
- 全部已请求 NPC Turn 结束后，Runtime 把结果和已提交事实交给下一个 Narrator Turn。Narrator 只能
  把成功 ToolCall/WorldCommand 产生的 ToolResult/WorldEvent 叙述为已经发生；它可以返回最终玩家
  可见的自然语言叙事，也可以在总预算允许时继续调用编排 Tool；
- 长期角色只保存 ECS 状态和 `AgentBinding`；Plan、未开始请求、Turn Result、一次调用的
  canonical history、取消、重试和 Future 属于 Runtime 临时状态；
- 共享 `AgentRunner` 拥有 LlmBridge、ToolExecutor 和执行状态机，Narrator 与所有 NPC 可以复用
  同一 Runner；Agent 对象本身不持有 Provider Client 或可变 World。

Narrator 可以通过结构化 Directive 更新 Scene Context，但其隐藏模型历史不能成为 NPC 的场景
事实源。Runtime 只把已提交 `SceneState`、允许披露的事件和本次 `NpcAssignment` 投影给 NPC。

角色上下文、场景上下文和 Assignment 都必须携带或绑定同一个 Revision。NpcAgent 返回后，
Tool 仍经 Runtime 提交 WorldCommand；Agent 对象不能把自己的 Character Snapshot 写回 ECS。
Narrator 不受固定 NPC 数量常量约束，但整个 PlayerInput 编排和每个 Narrator/NPC Turn 都受
Runtime 配置的 Model Call、ToolCall、Token、输出大小、墙钟时间和最大编排轮数约束；模型与
Mod 不能扩大这些上限。

#### 5.3.2 预设导入、运行时生成与 Narrator 分级

NPC/Scene 创建与 NpcAgent Turn 是两个阶段。导入或生成先创建持久化/Scene-scoped ECS 角色；
需要模型决策时才从当前 Revision 的世界投影构造一次性 NpcAgent。

预设内容来自版本化 Mod Package 中的 Content Pack；Package Manifest 声明包身份、兼容与依赖，
Content Pack 至少包含 Character/Scene Definition 以及它们引用的 Agent Profile、Item、Skill
和其它领域 Definition。导入管线必须：

1. 由 Content 层解析完整 Package，验证 Manifest、Schema、版本、依赖、内容哈希、Definition ID
   和所有跨文件引用；
2. 在不访问 Working World 的情况下生成不可变 Definition Registry，并把 Character/Scene
   Definition 纯编译为 `CharacterSpawnSpec`/Scene spawn plan；
3. 由 Runtime 建立一个初始化提交边界，把验证后的 Registry 与 spawn plan 交给 World；
4. 由 NpcFactory/World 先创建 Stable Object ID/Entity，再解析关系、位置、物品、技能和知识；
5. 校验当前世界引用与领域不变量后统一发布，任一步失败都不能留下半个 Pack 或 Scene。

Content Definition ID 与运行 ObjectId 必须分离。预设实例记录
`ContentOrigin { pack_id, definition_id, version }`，存档固定或迁移相应 content version。

运行时生成由 Narrator 通过 `create_npc` 提交受限角色意图，Runtime 从根世界解析默认
GenerationPolicy、当前 Scene/Place 和生成用 AgentProfile，再构造内部 `NpcGenerationRequest`；
生成器只产生 `NpcDraft`。Draft 不是 World Fact；Content 层必须先校验 Definition 引用和静态约束，Runtime 校验 Capability、数量与
模型预算，NpcFactory 再结合当前世界校验属性预算、初始物品/技能/Condition、Scene 和领域
不变量，最后转换为与预设内容相同的 `CharacterSpawnSpec -> SpawnNpcCommand`。生成成功后保存
完整领域状态和 `GeneratedOrigin`，加载时不得重新调用模型生成。

NarratorAgent 拥有 NPC 的叙事分级与调度决定，但模型侧不直接操作物化状态机。Narrator 只使用：

- `create_npc`：选择 Preset/Generated source、Scene/Persistent lifetime 与 narrated/agent mode；
- `request_npc_turn`：对 Observation 中标记为可调度的 committed ActorId 提供自然语言 assignment。

纯叙事提及不调用 Tool。`create_npc` 不接受 SceneId、PlaceId、GenerationPolicyId、AgentProfileId、
Revision 或 assignment；`request_npc_turn` 不接受 SceneId 或 Revision。上述字段由 Runtime 根据当前
World、ToolContext、Character Definition 与根世界默认 GenerationPolicy 注入。

Runtime 不用硬编码自然语言规则重新判断 NPC 是否“重要”，也不因为玩家出现“交互”关键词就
强制 Agent 化。玩家明确与某个角色交互时，Narrator 可以根据所需独立人格、记忆和决策深度
选择 NarratorProxy 或独立 NpcAgent。

Runtime 只承担可信执行：

- 在 Scene Observation 中明确投影 `npc_turn_available`；同一 Revision 中，使用该投影 ActorId 的
  `request_npc_turn` 不得再因未向模型披露的条件被拒绝；
- 验证 Schema、目标、Scene membership、Revision、Capability、数量和模型预算；
- 限制 Narrator 不能选择任意 Provider、原始 Component 或越权 Agent Profile；
- 在角色获得物品、建立关系/知识/目标、成为持久 Event 引用或被安排复现前，强制先完成实体化
  或 World-scoped promotion；
- Scene-scoped 实体在当前 Scene 存续时进入存档，Scene 结束且没有持久引用后才允许回收；
- 完整 NPC 可以 Dormant 或禁用 AgentBinding，但不得因暂时不调用模型而丢弃权威角色数据。

因此 Narrator 决定叙事语义，Runtime 只拒绝无效或越权计划，并返回结构化原因供 Narrator
调整，不擅自把请求改写成另一段剧情。

#### 5.3.3 Mod Package、事件规则与特殊玩法

Loreloom 把内置内容与外部 Mod 视为同一种受版本约束的输入，不为第三方内容建立旁路。
`ModPackage` 是分发与兼容单位，包含 Manifest、一个 Content Pack、可选 Rule Bundle、文本资源
和受约束的包内资源；内置世界也应通过同一校验、Registry、SpawnSpec、Factory 和持久化路径
加载。

第一阶段支持两层 Mod：

| 层级 | 可声明内容 | 不允许 |
|---|---|---|
| Content Mod | NPC、Scene、Agent Profile、Item、Skill、Attribute、Resource、Condition 等 Definition | 直接访问 World、执行代码或注册 Tool |
| Rule Mod | Event、Option、Trigger、Predicate、Effect、Parameter Schema 与 Gameplay Action | 任意脚本、任意 Component Patch 或未注册副作用 |

第三层 Extension Mod 用于将来无法用声明式规则表达的复杂玩法。它必须通过独立 RFC 冻结沙箱
Host API、Capability、资源配额、版本和存档边界；候选方向是 WASM Component。第一阶段不得
直接加载 Rust/C 原生动态库，也不得把本机 ABI 当作 Mod API。

Manifest 至少声明 Mod ID、版本、Loreloom Schema/Engine 兼容范围、依赖、内容哈希和所需
Capability。Definition ID 必须带 Mod 命名空间；重复 ID 默认拒绝，不能通过模糊加载顺序静默
覆盖。需要修改其它 Mod 时必须使用显式 Patch，声明目标 Mod、Definition 和兼容版本；具体
Manifest、命名空间与 Patch 格式为 **OPEN**。

Event Definition 必须结构化表达 Trigger、进入条件、节点、选项、可见/可用 Predicate、Effect
和后继节点；活动事件使用带 Stable ID 的 Event Instance 保存当前节点和生命周期。玩家或 Agent
选择选项时必须提交 Event Instance ID、Option ID 与 expected Revision，World 在执行时重新验证
可见性、可用性和状态，成功后产生 WorldCommand/WorldEvent。LLM 可以叙述选项与结果，但不能以
文本替代 Effect。一个 Option 的 Effect plan 与节点推进是一个复合 WorldCommand 的
all-or-reject 领域结果；具体 durable commit 机制仍由持久化提交协议冻结。

模组自定义角色参数不得伪装成动态 Rust Component 或无约束 JSON。Content 层通过版本化
Parameter Definition 声明 Bool、Fixed、Counter、Enum、TagSet、ObjectRef 等有限类型、默认值、
范围、可见性与持久化策略；运行实例只保存经 Schema 校验的 Parameter Set。

Rule Mod 的执行链固定为：

```text
Trigger
  -> bounded Predicate evaluation
  -> Effect plan
  -> whitelisted WorldCommand / registered rule entry
  -> WorldEvent
```

Trigger 只能来自已提交 WorldEvent、World Clock、Scene 转换或显式 Gameplay Action；Predicate
只能读取授权的结构化事实；Effect 只能使用引擎注册的白名单效果。数据 Mod 不能动态注册任意
Tool，而是通过稳定的通用 Tool/Command（例如选择事件选项或执行已注册 Gameplay Action）进入
规则系统。Runtime/World 必须为规则执行提供可信 initiator 与 Mod/Rule/source Event provenance，
数据 Mod 不能伪造 Actor 或系统身份。Tool Capability 不得由 Mod 文本、Prompt 或 Definition
自行扩大。

包内叙事文本和 Agent Profile 都视为不可信内容，不能覆盖系统安全策略。资源路径必须限制在包
根目录并经过大小、数量、递归深度和解压上限校验；Content/Rule Mod 默认没有文件、Shell、网络
或 Secret 能力。

#### 5.3.4 Scene、Place 与运行时世界拓扑

Scene 是可反复进入的持久叙事与生命周期容器；Place 是 Scene 内 Character 实际共处、可见性与
移动的具体节点。World 恰好一个 Scene 为 active，Character 的 location 指向 Place；普通 Place
移动与 Scene 激活是不同命令，不能用跨 Scene `move_character` 代替 `transition_scene`。

Place 的可达关系属于存档事实，以同 Scene 的双向连接表达。Content 编译必须把 Place Definition
edge 解析为运行 ObjectId，并拒绝跨 Scene、缺失或非对称连接；World 加载与每次提交都重新验证。
`move_character` 只能从角色当前 Place 沿一条连接到达目标 Place。

NarratorAgent 可以通过 Provider 原生 Tool Calling 请求运行时拓扑扩展，但不能提供运行 Stable ID、
Scene 归属或 edge：

- `create_scene { display_name, framing, entry_place_name, entry_place_description }` 原子创建一个
  inactive Scene 和必需 entry Place，不自动激活或移动玩家；
- `create_place { display_name, description }` 在当前 active Scene 创建 Place，并与玩家当前 Place
  建立双向连接，不自动移动玩家；
- Runtime 在 Narrator Turn 结束后执行单个创建命令，分配 ID、绑定 active Scene/当前 Place、保存
  引用 PlayerInput 或 WorldEvent 的 GeneratedOrigin，随后基于新 committed Revision 重新调用
  Narrator；
- 创建与 `transition_scene`、`create_npc`、`request_npc_turn` 在同一 Narrator Turn 互斥，避免用
  切换前的场景投影继续调度；NpcAgent 不获得创建 Scene/Place 或直接编辑 edge 的 Capability。

预设 Scene/Place 继续来自已验证 Content Definition；运行时生成的 Scene/Place 使用同一
SceneRecord/PlaceRecord、WorldCommand、RecordOp 和持久化重建路径。两者都不因离开 Scene、Prompt
改变或故事阶段结束而删除。Definition 专属的 Scene trigger 只匹配具有 ContentOrigin 的 Scene；
Generated Scene 仍产生通用 SceneLeft/SceneEntered WorldEvent，但不伪造 Definition ID。

### 5.4 LLM 只能经 Tool 影响世界

模型返回的自然语言可以直接成为候选 Transcript 内容，但不能直接修改 Working World。世界
变化必须经过以下路径：

```text
LLM ToolCall
   -> Tool Schema 与 Actor Capability 校验
   -> Loreloom Tool Handler
   -> typed WorldCommand
   -> Runtime 冲突与前置条件校验
   -> Armillae Simulation / Bevy Systems
   -> WorldEvent 与结果
   -> persistence boundary
```

Tool 分为：

- Query Tool：读取当前 Actor 可见的 Observation/Query Snapshot，不产生世界修改；
- Command Tool：提交类型化 WorldCommand，由世界规则决定是否成功；
- Orchestration Tool：只构造当前 Agent 编排的有界 Runtime 请求，例如向 NarratorPlan 追加
  NpcTurnRequest；不得直接读取或修改 ECS；
- Service Tool：访问 ECS 之外的外部系统，第一阶段默认不存在或禁用，后续需单独能力授权。

Tool Handler 不向模型暴露任意 `&mut World`、任意 Component 名称或脚本求值。Tool Schema 是
产品 API，应版本化、测试并返回结构化结果。权限和规则校验必须位于可信代码中，不能只写进
System Prompt。Mod 只能引用 Runtime 已注册的通用 Gameplay Tool/Action，不得从数据文件注入
新的 Tool Handler、Schema 权限或外部 Service Tool。

### 5.5 Agent Loop 是有预算、可取消的显式状态机

Armillae `LlmBridge` 每次只执行一次 Model Call，`ToolExecutor` 每次只执行一次 ToolCall。
Loreloom Runtime 在这两个边界之上实现有限状态机：

```text
trigger
  -> capture Observation at Revision N
  -> assemble canonical request
  -> one Model Call
  -> text and/or ToolCalls
  -> validate and execute allowed ToolCalls
  -> append ToolResults
  -> optionally start another Model Call
  -> complete, cancel, or fail
```

每个 Agent Turn 必须有模型调用次数、Tool 调用次数、Token、墙钟时限和输出大小预算；完整的
PlayerInput 编排还必须有总预算和最大 Narrator 编排轮数。到达预算时返回结构化
`budget_exhausted` 结果，并保持已经提交的世界变化可解释。Runtime 不允许模型通过循环
ToolCall 或反复生成下一轮 Plan 无限占用世界。

第一阶段修改类 ToolCall 按模型响应中的稳定顺序串行处理，不并发写世界。多个 ToolCall 是否
属于同一个原子提交仍是待决问题，不能由实现自行猜测。

第一阶段同时只允许一个 Agent Turn 占用 Runtime 的 Agent Loop 执行槽。NarratorAgent、
NpcAgent 和不同 NpcAgent 之间不得重叠运行 Model Call/Tool Loop；`NpcTurnRequest` 可以排队，但
下一个 Turn 只能在前一个 Completed/Cancelled/Failed 后开始。开始时必须针对当前 committed
Revision 重新校验请求并捕获 Observation，不能沿用排队时的可变世界引用或陈旧快照。

这条串行边界不禁止 Provider 自身的流式 I/O。等待 Provider 时 TUI 必须继续处理流式显示、取消
和退出，但第一阶段逻辑 World Clock 不随真实墙钟时间隐式推进；世界只通过显式
WorldCommand/System 变化。

### 5.6 ECS 只有一个逻辑写入所有者

Runtime 必须保证活动 Working World 在应用级只有一个逻辑写入所有者。TUI、Provider 任务和
上下文组装只获得拥有所有权的不可变投影；外部网络等待期间不得持有 Bevy 可变借用或世界锁。

这一决定不禁止 Bevy 在一次 Schedule 内根据访问集合并行运行 Systems，也不强制使用某个
Tokio task 或 OS thread。它只禁止多个应用级流程无序地直接写同一个 World。

来自 Revision N 的 Command 在执行前必须验证 Revision 或显式声明可重基。过期 Agent 响应
不能静默覆盖新世界；Runtime 应拒绝、重新投影或按未来明确策略重试。

### 5.7 持久化使用稳定领域记录重建 ECS

Loreloom 需要真正的存档，但长期格式不能是 Bevy 内部状态。持久化边界必须满足：

- 每个需要跨存档引用的对象具有 Stable ID；
- 被保存的 Component/Resource 有显式 Schema 名称和版本；
- 存档只包含拥有所有权的领域值，不包含 ECS 指针、借用、`Entity` 或 Schedule 状态；
- 加载时先验证版本与引用，再把记录物化为新的 Working World；
- Stable ID 到 `Entity` 的映射只在当前 Working World 内有效；
- 接受的模型决定以 WorldCommand、WorldEvent 或等价领域记录保存，回放不重新调用模型；
- 存档固定完整 Mod 依赖闭包的 ID、版本、内容哈希、显式 Patch 和必要迁移 provenance；
- Event Instance、Parameter Set 和影响未来规则的 Rule State 必须作为版本化领域记录保存；
- Provider 请求、完整隐藏推理和 Provider Secret 不是世界存档的一部分；
- 未知类型、版本迁移、缺失 Mod、内容哈希不匹配、缺失引用和部分写入必须有明确失败语义。

加载时不能静默替换为同名的其它 Mod 版本，也不能重新执行导入时的 LLM 生成。

概念模型采用“版本化 Snapshot + 有序领域变更记录”。候选 `RecordOp` 与 Snapshot 是重建 ECS 的
权威输入；WorldEvent 负责已经提交事实的叙事、审计和规则触发语义，但不能在没有明确映射契约时
独自承担任意 Component 的重建。Transcript 同样不是世界事实的替代品。

第一阶段后端的**首选候选**是通过 Toasty 使用嵌入式 SurrealDB + SurrealKV，SQLite 保留为 P0
Store Spike 的对照。选择依据是候选 SurrealDB driver 已验证：

- Toasty 公共 API 的显式顶层 Start/Commit/Rollback，并把写冲突分类为 serialization failure；
- `#[column(type = json)]` 到 SurrealDB Object/Array/标量的原生 JSON 往返，区分数据库空值与
  JSON literal null；
- migration ID tracking、事务化 apply，以及安全 Schema diff 的自动 SurrealQL generation；
- SurrealKV 文件引擎的事务、重开持久化与 migration tracking。

这些能力只消除了 driver 级已知缺口，并未证明 Loreloom 的存档协议。普通 Toasty batch 对非 SQL
driver 不自动获得原子性，因此每次 durable commit 必须显式开启事务。数据库 migration 只维护
物理表/索引和其 tracking；领域 record、ModLock、JSON payload 的 Schema/version migration 仍由
Loreloom 拥有。

候选 durable unit 是把一次成功 WorldCommand 对应的 `RecordOp`、WorldEvent、持久 Transcript
变化和 Save Head/current Revision 在同一显式事务内提交，并用 `expected_revision` 对 Save Head
执行 CAS。该记录集合已经是 Store Spike 的验证目标，但 ECS 在事务前后如何执行、失败时如何
恢复，以及多个修改 ToolCall 是否共享一个 durable unit 仍保持 OPEN。

最终冻结前，P0 Store Spike 必须与 SQLite 对照验证原子写入、双连接 Revision CAS、中途错误无
部分提交、提交前/中/后杀进程后的 N 或 N+1 恢复、备份/恢复、确定性关闭与存档切换、版本化 JSON
兼容、至少一万条 Record 的加载性能、提交延迟、构建体积和许可证兼容性。以下任一项不能可靠
满足时，应拒绝 SurrealDB 候选或降级为可选后端：原子/CAS/崩溃语义不成立、无法得到一致备份、
依赖不能从公开 release 或固定 public revision 解析、许可证与发布方式不兼容，或体积/性能明显
不适合单机 TUI。

### 5.8 TUI 使用状态投影而不是直接访问 World

默认宽屏布局：

```text
┌──────────────────────┬──────────────────────────────────────────┐
│ 角色 / 世界状态      │ 叙事、对话、Tool 结果与系统消息          │
│                      │                                          │
│ 身份与控制器         │ 可滚动历史                               │
│ 属性、状态与需求     │                                          │
│ 位置、时间与任务     ├──────────────────────────────────────────┤
│ 关系与上下文摘要     │ 多行输入框                         状态栏 │
└──────────────────────┴──────────────────────────────────────────┘
```

- 左侧只展示当前玩家可见或显式调试授权的数据；
- 右侧上部是可滚动 Transcript 与执行状态；
- 右侧底部是输入框，提交后显示排队、推理、Tool、提交或失败阶段；
- 渲染只读取 UiSnapshot，输入转换为 Runtime Command；
- 窄屏必须有可用降级模式，候选方案为状态/对话 Tab，而不是把输入框挤出视口；
- Provider 流式文本可以进入暂存显示，但只有完成或明确保留的内容才能进入持久 Transcript。

### 5.9 Rust 跟随 stable，不设置 MSRV

Loreloom 使用 Rust 2024 edition，构建与 CI 跟随执行时的最新 stable toolchain：

- Cargo package 不填写 `rust-version`；
- 不建立“仍支持 Rust X.Y”的测试矩阵；
- 不因旧编译器限制拒绝稳定语言或标准库能力；
- 应用提交 `Cargo.lock` 以保证依赖可复现，但 toolchain channel 不钉死补丁版本；
- 依赖自身的编译器要求仍必须满足；
- Bevy 版本跟随 `armillae-simulate-bevy` 兼容线，Rust 最新不等于擅自升级 Bevy。

## 6. Armillae 集成边界

[Armillae](https://github.com/mmstudio-games/armillae) 是独立发布的开源 Rust 基础设施项目，
不是 Loreloom 仓库的一部分。

| Armillae 能力 | Loreloom 用法 | Loreloom 不得假设 |
|---|---|---|
| `armillae-simulate` | 显式 Execute/Advance 与 Simulation 生命周期 | 自动游戏循环、Agent 或持久化 |
| `armillae-simulate-bevy` | Bevy Working World、Native Systems 与 Context | Bevy `Entity` 是稳定身份 |
| `armillae-llm` | Provider 无关的一次 Model Call | 自动 Tool、Memory 或重试策略 |
| `armillae-llm-rig` | 具体 Provider Adapter | Rig 类型可进入领域或存档 |
| `armillae-tools` | Tool Schema、注册与单次执行 | Tool Executor 持有 World 或继续模型调用 |
| `armillae-tools-macros` | 声明 Loreloom Tool | 宏负责权限、提交或事务 |

Loreloom 提交的 manifest 必须通过 registry 版本，或公开 Git URL 与明确 tag/revision 解析
Armillae crates，使干净 checkout 能独立构建。项目规范、示例和测试不得依赖未提交的外部
状态。

Loreloom 不修改 Armillae 公共边界来隐藏自身 Harness 复杂度；如果真实下游证据表明 Armillae
缺能力，应作为独立上游项目问题提出，Loreloom 文档只记录自身依赖的公开契约。

## 7. 候选 workspace 边界

本文提议第一阶段按责任拆分，最终 crate 创建仍需 Spec Active 后执行：

| crate | 责任 |
|---|---|
| `loreloom-core` | 稳定 ID、领域值、Command/Event、Observation/UiSnapshot 公共协议 |
| `loreloom-content` | Mod/Content Package、Definition/Rule Schema、依赖/Patch 验证与纯编译 |
| `loreloom-world` | Bevy Component/Resource、Factory、事件/规则 System 和 Armillae Simulation 集成 |
| `loreloom-agent` | 上下文组装、Agent Step、Tool 暴露策略和 Armillae LLM/Tool 适配 |
| `loreloom-store` | 版本化记录、Snapshot/Journal、加载、迁移和恢复 |
| `loreloom-runtime` | 应用级调度、世界所有权、Agent Loop、提交和取消 |
| `loreloom-tui` | 终端布局、输入编辑、渲染和 UiSnapshot 交互 |
| `loreloom` | 二进制装配、配置和进程生命周期 |

候选依赖方向：

```text
loreloom (binary)
  ├── loreloom-tui ───────────► loreloom-core
  └── loreloom-runtime
        ├── loreloom-content ─► loreloom-core
        ├── loreloom-world ───► loreloom-content ─► loreloom-core
        ├── loreloom-agent ───► loreloom-core
        └── loreloom-store ───► loreloom-core
```

`loreloom-content` 只负责拥有所有权的 Mod/Content 数据、静态 Definition Registry、依赖、
Patch、引用和 Rule Schema 验证，以及把 Character/Scene/Event/Rule Definition 编译为公共领域
SpawnSpec/规则计划；它不依赖 Bevy、LLM、Store 或 Provider，也不直接产生 ECS 副作用。
`loreloom-world` 消费已验证 Definition/SpawnSpec/规则计划，由 Factory 和规则 System 结合当前
世界完成最终校验与 WorldCommand 构造。

Runtime 可以依赖所有应用子系统；Content、World、Agent、Store 和 TUI 不形成循环。具体
Provider Adapter 由二进制装配，`rig-core` 类型不得进入 `loreloom-core`。NpcDraft、
CharacterSpawnSpec 和各 Definition 输入类型最终放在 Core 还是 Content，由 Schema 冻结时决定，
但依赖方向不得反转。

物理布局采用根级 Cargo virtual workspace，八个成员统一位于 `crates/`。每个 crate 必须在自己的
Manifest 中显式写版本，禁止 `version.workspace`，也不得声明 `rust-version`。Semifold 使用 Rust
workspace resolver 与 `.changes/` 变更集维护成员版本。内置 Mod 位于 `mods/`，共享测试数据位于
`tests/data/`；这两个目录的位置已经冻结，内部数据格式仍由后续 Schema 冻结。

## 8. 典型执行流程

### 8.1 玩家自然语言行动

```text
玩家提交输入
  -> TUI 发送 PlayerInput
  -> Runtime 只把自然语言输入交给 NarratorAgent
  -> NarratorAgent 基于 SceneObservation 与 Revision 调用结构化 Tool
  -> Runtime 从已接受 ToolCall 构造内部 NarratorPlan
  -> Runtime 执行 Plan 中允许的 Narrator Tool 和有序 NPC Turn
  -> 后续 Narrator Turn 汇总 NpcTurnResult 与已提交 WorldEvent
  -> Narrator 返回最终玩家可见的自然语言叙事
  -> 持久化世界事实/Transcript 并发布新 UiSnapshot
```

### 8.2 Narrator 编排与 NPC Turn

```text
PlayerInput
  -> Runtime 为 NarratorAgent 构建 Scene Observation
  -> NarratorAgent 以原生 ToolCall 提交结构化决定
  -> 判断 Mention / Materialize / NarratorProxy / NpcTurn 与 lifetime/controller
  -> 简单交互可由 NarratorProxy 完成
  -> 需要实体时提交 MaterializeNpcRequest
  -> Runtime 校验并通过 NpcFactory / WorldCommand 创建或提升 NPC
  -> 需要独立 Agent 时调用 request_npc_turn
  -> Runtime 按已接受 ToolCall 的语义顺序构造 NarratorPlan/NpcTurnRequest
  -> Runtime 依次校验并排队，不在 Narrator Tool 内递归调用模型
  -> 对每个请求从当前 committed Revision 生成 CharacterContext 与 SceneContext
  -> 组合 NpcAssignment，创建一次性 NpcAgent
  -> AgentRunner 按能力过滤 ToolDefinitions，执行受预算 NPC Turn
  -> NpcAgent 可以产生发言/意图/动作描述，并经允许的 ToolCall 请求世界动作
  -> Runtime 串行执行允许的 Command，记录实际 ToolResult/WorldEvent
  -> 每个 NPC 返回 NpcTurnResult
  -> 所有请求结束后再次调用 Narrator
  -> 根据 NpcTurnResult 与已提交事实生成最终自然语言叙事
  -> 若仍需 NPC 且总预算允许，继续调用 request_npc_turn；否则结束本轮
```

没有经过 Runtime 接受的 `NpcTurnRequest` 时，NPC 不会仅因“是 Agent”而后台运行。Narrator
决定请求顺序和数量，Runtime 不添加叙事优先级或公平性规则；第一阶段所有 Agent Turn 与 NPC
世界修改严格串行。请求数量没有固定常量，但受整轮/单 Turn 预算和最大编排轮数约束。

### 8.3 预设导入与运行时生成

```text
ModPackage ContentPack Character/Scene Definition ─┐
                                                   ├─> CharacterSpawnSpec
Narrator NpcGenerationRequest -> Draft ────────────┘
                                                │
                                                ▼
                                  NpcFactory 验证与补全
                                                │
                                                ▼
                                      SpawnNpcCommand
                                                │
                                                ▼
                                ECS Character + Origin + AgentBinding
```

两条来源路径必须共享 Factory、WorldCommand、不变量、持久化和 Agent Turn 逻辑，不得为生成 NPC
建立第二套“只存在 Prompt 中”的角色系统。

### 8.4 Event Option 与声明式玩法

```text
WorldEvent / Clock / Scene / Gameplay Action
  -> 匹配已编译 Trigger
  -> 创建或推进 Event Instance
  -> UiSnapshot / Observation 投影可见 Option
  -> 玩家或 Agent 提交 Event Instance + Option + expected Revision
  -> 重新计算 Predicate
  -> 执行白名单 Effect plan
  -> WorldCommand / WorldEvent / 新 Revision
```

过期或不再满足条件的 Option 必须返回结构化拒绝，不能按旧 UI 或旧 Observation 强制执行。

### 8.5 存档加载

```text
读取存档元数据和 Schema 版本
  -> 校验/迁移拥有所有权的记录
  -> 创建空 Working World
  -> 物化 Stable ID 与实体
  -> 安装 Component/Resource 与索引
  -> 校验引用和不变量
  -> 恢复 Revision
  -> 生成首个 UiSnapshot
```

加载过程不得调用真实 LLM，也不得依赖上次进程中的 Bevy Entity 位值。

## 9. 错误、取消与安全

- 玩家输入、Content/Generation/Rule、Tool 参数、世界冲突、Provider、存档和渲染错误必须
  分类，不能压成一个字符串；
- Content 错误必须能区分 Pack/Schema/版本/依赖/引用/编译失败；Generation 错误必须能区分
  Request 无效、Draft 不满足约束、预算拒绝和无法形成合法 SpawnSpec；
- Rule 错误必须能区分未知 Trigger/Predicate/Effect、参数类型错误、求值预算耗尽、循环/递归
  拒绝和失效 Event Option；
- Agent 等待期间取消不会产生未执行的世界变化；
- 已经提交的 Tool 副作用不会因后续 Model Call 失败而伪装回滚；
- 在原子性策略冻结前，不得实现多个修改 Tool 的“看起来像事务”语义；
- 模型不能选择自己的 Actor ID、扩大观察范围或注册新 Tool；
- Tool 必须绑定 Runtime 提供的 Actor/Session/Revision Context；
- Provider Secret 只来自运行时 Secret 配置，不进入 Prompt、ECS、存档或日志；
- 默认日志记录稳定 ID、阶段、预算、耗时、错误分类和 Revision，不记录完整内容；
- 所有未来外部 Service Tool 使用独立 capability 与审批策略。

## 10. 确定性与回放边界

LLM 输出本身不被宣称为确定性。Loreloom 的可回放承诺应是：

- 对已经记录的 WorldCommand/WorldEvent，在相同 Schema、规则版本和初始 Snapshot 下可以
  重建等价领域状态；
- 回放不会重新请求 Provider；
- 影响结果的随机数必须来自显式 Seed/Stream 并记录必要事实；
- Mod 依赖闭包、内容哈希、Rule 编译版本和业务相关执行顺序必须固定或记录；
- Bevy 内部并行不能让具有业务意义的结果依赖未定义执行顺序；
- Transcript 流式分片时序、Provider latency 和纯展示动画不属于领域确定性；
- 规则或 Schema 版本变化必须迁移或拒绝加载，不能静默产生不同世界。

具体确定性等级和哈希策略在持久化方案确认时冻结。

## 11. 主要取舍

### 11.1 收益

- Prompt 变为世界投影，不再承担数据库职责；
- 角色认知与全局真相可以明确分离；
- Tool 使权限、规则和失败可验证、可测试；
- 世界可以在没有 LLM 或使用 Mock LLM 时运行；
- 接受的模型决定能够保存、恢复和回放；
- TUI、Agent 和 Persistence 通过稳定投影隔离 Bevy 借用与内部身份；
- 内置与第三方内容共享协议，NPC、事件和玩法扩展仍可校验、迁移和回放；
- Armillae 的底层边界保持简单，Loreloom 在应用层拥有产品策略。

### 11.2 成本与风险

- 需要显式设计大量领域 Component、Command、Event 和迁移；
- Observation Builder 比“把所有聊天丢给模型”更复杂；
- ECS 修改与 durable commit 的一致性需要谨慎协议；
- 角色私有知识和大规模关系可能给 Query、存档和上下文预算带来压力；
- 多 Agent 并发会产生过期 Observation 和冲突；
- Tool Schema 也是兼容接口，改动需要版本与测试；
- Mod 依赖、Patch、规则预算、内容迁移和不可信资源处理扩大兼容与安全测试面；
- TUI 流式体验与确定性 Transcript 需要区分暂存和提交状态。

## 12. 被拒绝的方案

| 方案 | 结论 | 原因 |
|---|---|---|
| 聊天记录作为唯一记忆 | 不采用 | 无法保证事实一致性、规则、存档和测试 |
| 每轮把完整 ECS 序列化进 Prompt | 不采用 | 泄漏信息、成本高且仍无权限边界 |
| 模型文本直接 Patch Component | 不采用 | 绕过类型、权限、规则与失败语义 |
| Tool 获得任意 `&mut World` | 不采用 | Capability 过宽，无法稳定审计或版本化 |
| 直接保存 Bevy `World`/`Entity` | 不采用 | 内部身份和布局不是长期协议 |
| 把 Agent Loop 放进 `LlmBridge` | 不采用 | 破坏 Armillae 单次调用边界和可组合性 |
| TUI 直接 Query ECS | 不采用 | 渲染与写入生命周期耦合，容易泄漏私有状态 |
| 为 Loreloom 锁定 MSRV | 不采用 | 项目明确选择跟随最新 stable Rust |
| 无限制并发运行所有 NPC | 不采用 | 成本、冲突、公平性和可恢复性不可控 |
| Narrator Tool 内同步递归调用 NpcAgent | 不采用 | 绕过 Runtime 的预算、取消、队列、错误与 Revision 边界 |
| 长期 NpcAgent 持有 Character 数据副本 | 不采用 | 世界变化后形成陈旧的第二事实源，难以持久化和恢复 |
| NpcAgent 直接持有 Provider Client 与可变 World | 不采用 | 角色、执行服务和副作用权限耦合 |
| Runtime 用关键词硬编码 NPC 是否需要 Agent | 不采用 | 自然语言与叙事重要性属于 Narrator 语义判断 |
| Narrator 直接输出 ECS Component Patch | 不采用 | 绕过 Schema、Factory、权限和世界不变量 |
| Content Importer 直接写 Bevy World | 不采用 | 内容解析、授权、世界不变量和提交边界会耦合且难以原子失败 |
| 用加载顺序隐式覆盖重复 Definition | 不采用 | 存档结果依赖环境顺序，无法稳定诊断、迁移或回放 |
| 用任意 JSON Bag 保存模组角色参数 | 不采用 | 缺少类型、引用、范围、可见性与迁移契约 |
| 数据 Mod 动态注册任意 Tool 或脚本 | 不采用 | 绕过 Capability、确定性、审计和副作用边界 |
| 第一阶段直接加载 Rust/C 动态库 | 不采用 | ABI 不稳定且原生代码默认拥有过宽系统权限 |
| 加载存档时重新生成运行时 NPC | 不采用 | Provider 输出不确定且会改变既有角色事实 |
| Character 内嵌完整 `Vec<Item>` 和 `Vec<Skill>` | 不采用 | 身份、引用、转移、来源、冷却和独立生命周期难以维护 |
| 同时维护 Character 背包列表和 Item 反向容器关系 | 不采用 | 形成两个可冲突的事实源 |
| 把技能效果函数或 Provider 对象写入存档 | 不采用 | 无法版本化、迁移或跨进程重建 |
| 保存 EffectiveAttributes/最终加成作为权威值 | 不采用 | 与装备、Condition、技能和基础值形成重复事实 |
| 用单个互斥 CharacterState 表达全部状态 | 不采用 | 无法同时表达生命、行动、姿态和多个 Condition |
| Narrator 文本直接 set attribute/health/condition | 不采用 | 绕过规则来源、Revision、Event 和恢复语义 |
| 把普通 Toasty batch 当作原子提交 | 不采用 | 非 SQL driver 不自动包装事务，必须使用显式 transaction handle |
| 用数据库 migration 代替领域记录迁移 | 不采用 | 物理表/索引 tracking 不理解 ModLock、record version 或领域不变量 |
| 依赖未发布或本机路径的 Store driver | 不采用 | 干净 checkout、CI 和发布无法独立解析，且把外部状态泄漏为项目假设 |

## 13. 验收场景

接受后的 Active Spec 和实现至少需要覆盖：

1. 创建含稳定身份、属性、位置和物品的世界，退出并加载后领域状态等价；
2. Bevy Entity 值变化后，Stable ID 引用仍正确；
3. NPC 只能在自己的 Observation 中看到被授权事实；
4. 模型声称“获得物品”但未调用成功 Tool 时，Inventory 不改变；
5. 转移或装备物品只修改唯一的容器/装备事实，背包 Query 不产生重复或丢失；
6. 嵌套容器拒绝循环，非法堆叠合并不会丢失实例状态；
7. 角色只能使用已被授予、当前启用、资源足够且冷却结束的技能；
8. Active Skill 通过 Tool 和规则 System 产生 Event，Passive/Reaction 不制造隐藏 Agent Loop；
9. Command Tool 经规则 System 修改世界，并返回结构化成功或拒绝原因；
10. Narrator 根据玩家意图决定 Mention/Materialize/NpcTurn，Runtime 只校验并执行受约束决定；
11. 预设 Definition 和运行时 Draft 都经同一 CharacterSpawnSpec/NpcFactory 创建 ECS 角色；
12. 生成 NPC 加载时使用已保存领域状态，不重新请求模型；
13. Scene-scoped NPC 在 Scene 内可恢复，被持久 Event/关系/物品引用前完成 World promotion；
14. NarratorPlan 中的 NpcTurnRequest 只有经 Runtime 校验后才按 Narrator 给出的顺序执行；
15. NpcAgent 每个 Turn 都从开始时 committed Revision 的 CharacterContext、SceneContext 和
    Assignment 创建；
16. NpcAgent 不能把 Snapshot 直接写回 ECS，所有修改仍经 Tool/WorldCommand；
17. Effective Attribute 按确定顺序从 Base、Modifier 和 Definition 重建；
18. Resource current、Condition source/stack/expiry 和正交角色状态保存后可等价恢复；
19. 未诊断 Condition 不会通过 Observation 向角色泄漏真实名称；
20. Narrator 初始 Attribute/Condition Hint 经 Factory 校验，创建后不能用文本直接改状态；
21. 一个 Agent Step 由多个 Armillae 单次调用组成时，预算、顺序和取消可观察；
22. 过期 Revision 的修改不会静默覆盖新状态；
23. Provider 失败不会生成幽灵 WorldEvent；
24. 已记录命令的回放不调用 LLM，并重建等价领域状态；
25. 存档、日志和错误中不含 Provider Secret；
26. TUI 左侧展示角色状态，右侧展示历史，右下输入始终可达；
27. 窄屏降级仍能查看状态、历史并输入；
28. Mock Bridge 可以完成无网络端到端测试；
29. 项目 Cargo manifest 不包含 `rust-version`，最新 stable 的完整质量门禁通过；
30. 内置内容和外部 Mod 使用相同 Package 校验、Registry、SpawnSpec、Factory 和提交路径；
31. 重复 Definition 默认拒绝，显式 Patch 只有目标 ID/版本匹配时才能生效；
32. Event Option 在 current Revision 重新校验，过期或不可用选项不会产生 Effect；
33. 模组 Parameter Set 只包含符合版本化 Schema 的类型值，并可保存、迁移和恢复；
34. Rule Trigger/Predicate/Effect 按确定顺序和预算执行，Effect 只能形成白名单 WorldCommand；
35. Content/Rule Mod 不能注册任意 Tool，也不能访问包外文件、网络、Shell 或 Secret；
36. 存档固定 Mod 依赖闭包与内容哈希，缺失或不兼容 Mod 时迁移或明确拒绝加载；
37. 回放同一 Mod lock 与规则记录时不调用 Provider，并重建等价 Event/Parameter/世界状态；
38. 一个 durable unit 的 RecordOp、WorldEvent、Transcript 和 Revision 要么全部可见，要么全部
    不可见；
39. 两个连接从同一 expected Revision 竞争提交时恰好一个成功，失败方得到可判断的 Conflict；
40. 在提交前、提交中和提交后杀进程并重开，只能观察到完整 Revision N 或 N+1；
41. 版本化 JSON payload 对嵌套对象、数组、大整数、未知字段和 JSON null 的策略有 round-trip
    证据；
42. 备份恢复、关闭后重开和存档切换不产生半份 Snapshot、旧文件锁或跨存档数据串扰；
43. Loreloom 的干净 checkout 只使用 registry release 或固定 public Git revision 解析 Store
    依赖，不依赖任何本机相邻仓库；
44. NarratorAgent 与所有 NpcAgent Turn 使用同一个串行执行槽，后一个 Agent 只在前一个结束后
    基于届时有效的 committed Revision 获取上下文；
45. 玩家自然语言只进入 NarratorAgent，由 NarratorPlan 决定 NPC Turn 的语义顺序与数量，Runtime
    不另设叙事优先级或公平性判断；
46. NpcTurnResult 区分 NPC 的发言/意图/动作描述与实际 ToolResult/WorldEvent，Narrator 不会把
    未成功提交的声称动作叙述为已经发生；
47. Narrator 正文只按自然语言使用；内部 Plan 只来自已接受 ToolCall。不存在固定 NPC 数量常量，
    循环由可配置整轮/单 Turn 资源上限与最大编排轮数终止；
48. 等待 Provider 时 TUI 的 Runtime thinking 状态、取消和退出保持响应，未完成 Provider 正文不
    展示；已解析 ToolCall 的安全名称与执行状态可以作为临时 Activity 实时显示，但不包含参数、
    ToolResult 或持久化语义；逻辑 World Clock 不随墙钟时间隐式推进。

## 14. 待决问题

以下问题必须在 RFC 接受前确认，或明确拆到阻塞相应实现的后续 RFC：

1. SurrealDB + SurrealKV 首选候选能否通过 P0 Store Spike，并相对 SQLite 对照在原子性、恢复、
   备份、性能、体积、公开发布和许可证方面成立？
2. ECS 执行成功但持久化失败时，Working World 如何回滚、重建或进入只读故障状态？
3. 多个修改类 ToolCall 是逐个提交、整批原子提交，还是默认只允许一次一个修改动作？
4. 玩家输入优先解释为“玩家说的话”“玩家角色行动”还是支持显式输入模式？
5. Runtime 内部 `NarratorPlan`、`NpcTurnRequest`、`NpcTurnResult` 的精确 Schema 是什么；整轮/
   单 Turn 预算有哪些字段、配置层级和默认值，最大编排轮数是多少？
6. Known Fact、Belief、Memory Record 和 Transcript 之间的最小 Schema 是什么？
7. 对话全文、摘要和嵌入索引分别保存多久，哪些属于存档兼容承诺？
8. Runtime thinking 状态在取消、失败与完成时如何清除或替换？
9. 初始可玩垂直切片包含哪些地点、角色、行动和胜负/进展反馈？
10. Item/Skill Definition 使用什么内容包格式、版本和迁移策略？
11. 物品堆叠的等价比较、拆分 ID 和容量/重量单位如何精确表示？
12. Skill Executor 第一阶段允许哪些数据驱动效果，哪些必须注册 Native System？
13. Content Pack、Character/Scene Definition、SpawnSpec 和 Origin 的精确 Schema 是什么？
14. `create_npc` 的 source/lifetime/mode、内部物化状态、Scene 清理和 promotion 条件如何冻结？
15. Attribute 数值类型、ID 注册、Modifier 优先级与 Multiplication/Override 语义是什么？
16. Resource maximum 变化时 current 使用 clamp、比例缩放还是 Definition 自定义策略？
17. Condition Stack/Duration/Executor、症状投影与 Life/Action/Posture 状态机的精确 Schema
    是什么？
18. Mod Manifest、ID 命名空间、依赖解析、内容哈希、显式 Patch 顺序与冲突诊断如何冻结？
19. Event Definition/Instance、Option、Parameter Definition/Set 和 Gameplay Action 的精确 Schema
    是什么？
20. 第一阶段允许哪些 Trigger、Predicate 与 Effect，可信 rule initiator/provenance、求值预算、
    递归/循环和确定顺序如何定义？
21. Mod 发现目录、包格式、压缩/资源限制、启停和开发期热重载策略是什么？
22. 存档中的 Mod lock、内容移除、版本升级、Patch 变化和迁移失败如何处理？
23. Extension Mod 明确拆到后续 RFC 时，是否采用 WASM Component，以及 Host API、Capability、
    签名和资源配额边界是什么？

## 15. 接受结果与后续工作

项目方于 2026-08-30 接受第 5 至 10 节的核心架构，并授权 Runtime Spec 转为 Active、建立实施
清单和初始化 Cargo workspace。第 14 节未决问题不因接受而获得默认答案；它们作为 Active Spec
中的范围化实施门禁，必须在相应产品 API、持久化格式或行为实现前通过后续 RFC、Spike 结论或
明确冻结记录解决。

后续按以下顺序执行：

1. 同步 `.agents/DESIGN.md` 和 Active Runtime Spec；
2. 创建 `../todos/runtime.md` 并更新 TODO 索引；
3. 使用 Cargo CLI 创建 workspace 与无公共领域 API 的 crate 脚手架；
4. 完成 Persistence commit、Bevy/Armillae integration、TUI、Agent Loop、Content 与 Mod P0 Spike；
5. 解除对应范围门禁后实现最小可玩垂直切片。

## 16. 决策依据

项目方在 2026-08-29 明确提出：

- 创建名为 Loreloom 的项目；
- 使用 Rust 开发 TUI 大世界游戏；
- 基于 Bevy ECS 系统与 Armillae LLM Bridge/Tool 能力；
- 把酒馆式应用中依赖模型记忆的持久数据重构为 ECS 组成部分；
- 把数据操作函数封装为 Tool；
- TUI 左侧显示角色状态等数据，右侧底部提供输入框；
- 不锁定 MSRV，直接使用最新版 Rust。

项目方随后于同日确认：

- 主 NarratorAgent 负责叙事和提出 NPC 调度请求，Runtime 负责校验、排队和实际执行；
- NpcAgent 在每个 NPC Turn 时由角色上下文、场景上下文和 Narrator Assignment 构造，不把
  Agent 实例作为角色存档；
- 角色上下文来自 ECS，Narrator 提供结构化场景 Directive/Assignment，隐藏模型历史不成为事实；
- AgentRunner 统一持有 LLM Bridge 和 Tool 执行能力，NpcAgent 不直接拥有 Provider 或可变 World；
- 预设 Character/Scene 使用版本化 Mod Package 中的 Content Pack 导入，运行时 NPC Draft 与
  预设 Definition 汇入同一 CharacterSpawnSpec/NpcFactory/WorldCommand 创建路径；
- NarratorAgent 根据玩家意图决定 NPC 的叙事提及、实体化、代理或独立 Agent 及生命周期；
  Runtime 不重做叙事判断，只校验 Schema、Revision、Capability、预算和数据不变量；
- 背包不作为 Character 内嵌物品数组，而以 Item/Container 实体和单一 `ContainedBy` 关系表达；
- 物品区分静态 Definition 与可持久化 Instance，装备、所有权和物理位置分别建模；
- 技能区分静态 Definition 与角色的 Skill Grant，等级、来源和冷却属于持久运行状态；
- 主动技能通过 Tool -> WorldCommand -> System 使用，被动和反应技能由明确规则入口驱动；
- Agent 只取得背包/技能上下文投影，详细数据通过受控 Query Tool 获取；
- 角色机械状态拆分为 Base Attribute、Resource current、来源明确的 Modifier、Condition Instance
  和正交 Life/Action/Posture 状态机；
- Effective Attribute、最终上限和可用行动是可重建派生值，不作为权威存档；
- Narrator 仅在 NPC 创建时提供受校验 Attribute/Condition Hint，创建后所有状态变化经
  Tool/WorldCommand/System；
- 内置内容与外部 Mod 共用版本化 Package/Definition/Factory 路径；第一阶段允许 Content Mod
  和声明式 Rule Mod 导入 NPC、Scene、事件选项、类型化角色参数和特殊玩法；
- Event Option 与模组效果必须经 Revision、Predicate、Capability 和 WorldCommand 校验，数据
  Mod 不能注册任意 Tool；复杂代码扩展留给后续沙箱 Extension Mod 设计。

本 RFC 在这些约束上补充了稳定身份、Observation、Command/Event、单写入所有者、持久化重建、
安全和回放等候选边界；这些补充仍需项目方通过 RFC 接受确认。

### 16.1 Mod 子系统确认记录

项目方于 2026-08-29 审阅文档后，明确确认第 5.3.3 节的 Mod 分层与安全方向，以及第 13 节
第 30–37 项对应的验收意图：

- 内置内容和外部 Mod 共用 ModPackage/ContentPack、Registry、Factory、提交和存档路径；
- 第一阶段支持 Content Mod 与声明式 Rule Mod；
- Event Option、Parameter Set 和 Gameplay Action 是类型化、版本化领域协议；
- 数据 Mod 不能注入任意 Tool、脚本、动态 Component 或外部副作用；
- 存档固定 Mod lock，Extension Mod 由后续沙箱 RFC 处理。

这次确认在当时只是 RFC 的已确认输入，不解决第 14 节的精确 Schema/协议问题，也没有单独改变
RFC 状态；整体接受结果记录于第 15 节与第 16.5 节。

### 16.2 持久化候选确认记录

项目方于 2026-08-30 审阅 SurrealDB/Toasty driver 的最新能力后，同意把 SurrealDB + SurrealKV
记录为 Loreloom 第一阶段 Store 的首选候选，并保留 SQLite 作为对照。审阅确认候选 driver 已覆盖
显式顶层事务、原生 JSON、migration tracking/安全范围自动生成和 SurrealKV 重开持久化。

这次确认只改变候选排序和 P0 验证输入，不冻结最终后端，也不授权产品实现。Loreloom 仍需独立
验证 durable unit、Revision CAS、ActionId 幂等、故障注入、进程崩溃、备份恢复、存档切换、性能、
构建体积、公开依赖可解析性与许可证；数据库 migration 不能替代领域 record/ModLock/payload
migration。该次确认当时未单独改变 RFC 状态；整体接受结果记录于第 16.5 节。

### 16.3 串行 Agent 调度确认记录

项目方于 2026-08-30 确认第一阶段 NarratorAgent 与 NpcAgent 严格串行：Runtime 只有一个 Agent
Loop 执行槽，多个 NpcTurnRequest 可以排队但不能并发运行；每个后续 Turn 在开始时
按届时 committed Revision 重新校验并捕获上下文。

该次确认当时只冻结并发边界，尚未冻结请求来源、Narrator 语义顺序、NPC 数量/预算、Narrator
汇总调用或 Provider 等待期间的世界推进策略；这些问题随后由第 16.4 节的确认进一步解决。

### 16.4 Narrator 编排模型确认记录

项目方随后于 2026-08-30 确认：

- 玩家自然语言只与 NarratorAgent 交互，由 Narrator 结合场景数据和上下文生成叙事计划；
- NarratorPlan 决定是否调用 NPC、调用哪些 NPC 及其语义顺序，Runtime 不实现叙事优先级或
  公平性判断；
- 每个 NpcAgent 完成 Tool Loop 后返回有界 NpcTurnResult，包含发言、意图、动作描述和实际
  ToolResult/WorldEvent 关联；
- NarratorSynthesis 只依据已提交事实描述已经发生的动作，并可以结束本轮或在总预算内生成下一轮
  NarratorPlan；
- NPC 数量不使用固定常量，Runtime 以可配置的整轮/单 Turn Model Call、ToolCall、Token、输出、
  墙钟时间和最大编排轮数限制资源；
- 等待 Provider 时 TUI 保持响应，但逻辑 World Clock 不随真实时间隐式推进；
- 早期的 NPC 激活请求术语由 `NpcTurnRequest`/`NpcTurnResult` 取代。

该确认冻结编排所有权、串行数据流和世界等待语义，但不冻结 Plan/Request/Result/Synthesis 的精确
Schema、预算字段、配置层级、默认值或最大编排轮数。

### 16.5 RFC 接受与 workspace 初始化确认记录

项目方于 2026-08-30 在被明确询问是否接受 RFC 0001、授权 Runtime Spec 转为 Active、建立实施
清单并初始化 Cargo workspace 后回复“开始吧”，确认进入实施阶段。随后进一步确认共享测试数据
采用 `tests/data/`。

本次接受冻结第 5 至 10 节的核心边界和以下工程约定：

- 使用 `crates/` 下的八成员 Cargo virtual workspace；
- 每个 crate 显式声明自身版本，禁止 `version.workspace`，项目不设置 MSRV 或 `rust-version`；
- 使用 Semifold Rust resolver 与 `.changes/` 管理版本；base branch 为 `main`，release branch 为
  非 `main` 的 `release`；
- 仓库内置 Mod 位于 `mods/`，共享测试数据位于 `tests/data/`。

第 14 节未决问题继续阻塞相应公共 API、持久化格式和产品行为，但不阻止无公共领域 API 的
workspace 脚手架、版本工具配置与 P0 Spike。

### 16.6 模型正文、持久 Scene 与 thinking 状态修订记录

项目方于 2026-08-31 进一步确认：

- Narrator、NpcAgent 与 generation stage 的模型正文必须是自然语言，Runtime 在任何情况下都不从
  正文解析 JSON 结构化控制数据；
- 结构化编排、世界操作与 NPC Draft 统一使用 Provider 原生 Tool Calling；ToolCall 被 Runtime
  接受后才构成内部 NarratorPlan、NpcTurnRequest 或领域命令；
- Scene、Scene-owned entity 及其状态在离开后继续持久化，Scene 切换只停用旧 Scene、激活或首次
  物化目标 Scene，不因故事阶段结束而自动删除；promotion 只改变角色的领域归属；
- TUI 只展示 Runtime phase 映射出的 thinking 状态，不展示 Provider 未完成正文。

本修订取代第 16.4 节中“模型生成结构化 NarratorPlan/NarratorSynthesis”与 Provider 正文流式展示
的早期表达，但不改变 Narrator 的编排所有权、NpcTurnRequest 的语义顺序、单一 Agent 执行槽、
Tool 权威边界、两级预算或 World Clock 等既有核心决定。持续约束实现的精确契约以 Active Runtime
Spec 为准。

### 16.7 Narrator NPC Tool 面压平记录

项目方于 2026-09-01 确认现有 `NarratorNpcDecision` 交叉组合对模型暴露了过多 Runtime 内部状态，
并接受把模型侧接口压平为 `create_npc` 与 `request_npc_turn`：前者只表达 Preset/Generated source、
Scene/Persistent lifetime 和 narrated/agent mode，后者只接受 Observation 中 committed ActorId 与
自然语言 assignment。Scene、Place、Revision、GenerationPolicy、AgentProfile、request identity 与
队列位置均由 Runtime 注入。

该修订保留质量优先的物化后重规划、单一 Agent 执行槽、NpcDraft Tool Calling、统一
CharacterSpawnSpec 路径和 Narrator 编排所有权，只撤销模型直接构造 `NpcTarget × Action × Lifetime
× Controller × Assignment` 组合的 wire。纯叙事提及不调用 Tool；Runtime 必须明确投影可调度 NPC，
并为 TUI 保留脱敏 Tool 拒绝码。

### 16.8 Scene/Place 动态创建确认记录

项目方于 2026-09-01 确认 Scene 是可反复出现的持久容器、Place 是 Scene 内具体地点，并接受由
Narrator 通过 Tool 创建二者：`create_scene` 原子创建 inactive Scene 与 entry Place，
`create_place` 在 active Scene 创建 Place 并连接玩家当前 Place；Runtime 负责 Stable ID、归属、
连接、GeneratedOrigin 与提交，创建后重新规划且不自动切换或移动。NpcAgent 不直接修改世界拓扑，
静态和动态 Scene/Place 都在离开后保留。

项目方同时确认把该能力与已冻结的 WorldLock/ModLock 候选内容协调一并实施。本节是对既有 Active
Runtime Spec 的增量确认，不引入通用 Definition migration，也不为公开版本前开发存档增加兼容
分支。

### 16.9 实时 Tool Activity 确认记录

项目方于 2026-09-01 确认 Agent ToolCall 不再等到完整玩家回合结束后统一显示：AgentRunner 在完整
ToolCall 已解析且即将执行时发布 Pending，并在 ToolResult 返回后原位发布 Succeeded、Rejected 或
Failed；Runtime/TUI 只传递 call ID、Tool 名称、状态与脱敏错误码，不传递原始参数或 ToolResult。

Tool Activity 是当前玩家回合的临时 UI 进度，不进入 Transcript、Agent Context 或存档；最终画面按
玩家输入、Tool Activity、Narrator 正文的叙事顺序投影。该修订不启用 Provider 正文 streaming，不
改变 Tool 授权、执行顺序、提交原子性、Narrator 编排所有权或已提交世界事实。
