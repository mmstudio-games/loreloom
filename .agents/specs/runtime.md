# Loreloom Runtime 规范

> 状态：Active Spec
> 规范基线：2026-08-30
> 适用范围：Loreloom Core、Content、World、Agent、Store、Runtime、TUI 与应用装配
> 已确认基线：RFC 0001 核心架构、RFC 0002 根世界/Mod 边界与 workspace 初始化约定
> 设计入口：[Loreloom 设计索引](../DESIGN.md)
> 决策来源：[RFC 0001：Loreloom 架构](../rfcs/0001-loreloom-architecture.md)、[RFC 0002：根世界与 Mod](../rfcs/0002-root-world-and-mods.md)
> 基础设施依赖：[Armillae](https://github.com/mmstudio-games/armillae)

本文把已接受的 RFC 0001 展开成第一阶段工程契约。本文中的“必须”“不得”和“只”约束所有
进入实施的 Loreloom 代码、配置、测试和内容。

标记为 **OPEN** 的条款尚未冻结，相关公共 API、持久化格式或产品行为保持阻塞；它们不阻止
workspace、空 crate、版本工具和 P0 Spike 初始化。实现不得选择一个方便的答案后再反向修改规范。

## 1. 第一阶段范围

### 1.1 产品目标

第一阶段必须交付一个本地单玩家、终端运行的最小可玩纵向切片：

- 一个可创建、保存和加载的 Bevy ECS 世界；
- 一个负责叙事/调度的 NarratorAgent、一个玩家控制角色和至少一个按需调用的 NPC Agent；
- 角色具有结构化身份、位置、属性、状态、物品、装备、技能、关系、认知和目标数据的最小子集；
- 玩家可以输入自然语言，查看叙事/对话和结构化状态变化；
- Agent 只能通过已注册 Tool 查询或请求修改世界；
- 一次被接受的行动形成可恢复的版本化世界结果；
- 根目录唯一主世界与至少一个外部 Content/Rule Mod 通过同一 Registry/Factory 管线加载并可保存、重开；
- Mock Bridge 只用于无需网络的确定性测试，不作为生产叙事来源；
- TUI 在正常宽度下使用左状态、右内容、右下输入布局。

### 1.2 明确非目标

- 网络多人、账号、云存档和跨设备同步；
- 图形窗口、物理、音频和完整 `bevy_app` 主循环；
- 无限制后台 NPC、自主外部网络访问或 Shell Tool；
- 用户提供的任意脚本、动态 Component、Rust/C 原生动态库或生产期热加载；
- Embedding、Vector Store 和 RAG；
- 通用世界编辑器、角色卡兼容层或第三方酒馆导入；
- 生产级内容量、完整经济、战斗或任务系统；
- 为旧 Rust 编译器提供兼容保证。

## 2. 规范不变量

第一阶段所有实现必须满足：

1. **结构化事实**：影响规则或未来决策的数据不能只存在于模型上下文。
2. **稳定身份**：持久引用使用 Loreloom Stable ID，不使用 Bevy `Entity` 位值。
3. **显式副作用**：模型文本不修改世界；只有成功执行并提交的 Command 才能产生 WorldEvent。
4. **能力约束**：Agent 只能看到 Runtime 为当前 Actor 暴露的 Observation 和 Tools。
5. **单写入所有者**：Working World 在应用层只有一个逻辑写入所有者。
6. **版本一致性**：Agent 输入和修改请求都关联 Revision；过期写入不能静默成功。
7. **可重建存档**：长期格式是拥有所有权的版本化领域记录，可重新物化 ECS。
8. **下层单次边界**：Armillae Bridge 一次调用，Tool Executor 一次执行，由 Runtime 组合循环。
9. **UI 隔离**：TUI 只消费 UiSnapshot，不持有或查询 `World`。
10. **无 MSRV**：Loreloom 不声明 `rust-version`，CI 跟随最新 stable。
11. **根世界与 Mod 分离**：主世界不是 Mod；两者使用相同 Registry、Factory 和提交路径，但分别
    锁定为 WorldLock 与 ModLock。
12. **类型化扩展状态**：模组参数、事件和规则状态必须有命名空间、Schema、版本和 Stable ID。
13. **受限规则**：数据 Mod 只能执行有预算的声明式规则和白名单 Effect，不能注入代码或 Tool。
14. **Mod 可重建**：存档固定 Mod 依赖闭包与内容哈希，缺失或不兼容时迁移或明确失败。
15. **显式持久事务**：durable unit 不能依赖普通 ORM batch 的隐含原子性，只有后端显式事务成功
    后才能发布 committed Revision。
16. **单一 Agent 执行槽**：第一阶段 NarratorAgent 与所有 NpcAgent Turn 严格串行，排队不等于
    并发执行。

## 3. 逻辑模块与候选 crates

### 3.1 `loreloom-core`

拥有后端无关且可序列化的领域协议：

- Stable ID 与 Revision；
- WorldCommand、WorldEvent、CommandOutcome；
- Observation、TranscriptItem 和 UiSnapshot；
- Save metadata 与版本化 record envelope；
- 错误分类中需要跨 crate 判断的事实。

不得依赖 Bevy、Ratatui、Provider SDK、数据库 Client 或异步运行时。

### 3.2 `loreloom-content`

拥有：

- 根世界 Manifest、Mod Package manifest、Content Pack、Rule Bundle、Prompt/资源索引和拥有所有权 Schema；
- Character/Scene/Event、Item/Skill/Attribute/Resource/Condition/Parameter 与 Gameplay Action
  Definition；
- WorldLock、Mod 依赖图、版本兼容、内容哈希、Definition ID 唯一性、显式 Patch 和跨引用验证；
- 静态 Definition Registry；
- Character/Scene Definition 到公共领域 `CharacterSpawnSpec`/Scene spawn plan 的纯编译；
- Event/Rule Definition 到受限 Trigger/Predicate/Effect plan 的纯编译与静态预算检查；
- ContentOrigin 输入和可供迁移/诊断使用的内容 provenance。

依赖 `loreloom-core`。不得依赖 Bevy、Armillae LLM/Tool、Provider SDK、Store 后端或 TUI，也不得
直接访问 Working World、分配 Bevy Entity 或提交 WorldCommand。Core 拥有
`CharacterSpawnSpec`、持久领域 record 与共享值类型；Content 拥有 Definition/NpcDraft wire、
Registry 和两条输入到 SpawnSpec 的统一纯编译器。不得引入 Content -> World/Agent/Store 的反向
依赖，也不得让 World 直接接受原始 NpcDraft。

### 3.3 `loreloom-world`

拥有：

- Bevy Component、Resource、Relation entity 和索引；
- 领域规则与 Native Systems；
- Event Instance、Parameter Set、Rule State 与声明式规则求值；
- Armillae Simulation Module/entry 注册；
- Stable ID 与 `Entity` 的运行期映射；
- Observation 和持久化 record 的领域投影；
- 消费已验证 Definition/SpawnSpec，并由 NpcFactory 结合当前世界校验属性预算、Scene、领域
  引用与不变量；
- WorldCommand 到 Schedule/System 的执行。

依赖 `loreloom-core`、`loreloom-content`、`armillae-simulate`、`armillae-simulate-bevy` 和与其兼容的
`bevy_ecs`。不得依赖 LLM、Provider Adapter、TUI 或具体存储后端。

### 3.4 `loreloom-agent`

拥有：

- Actor Observation 到 canonical model request 的上下文组装；
- Agent profile、模型选择和预算策略的应用层解释；
- `NarratorAgent`、`NpcAgent`、Runtime 内部 `NarratorPlan`、`NpcTurnRequest`、自然语言
  `NpcTurnResult` 和不可变 Agent Context；
- 共享 `AgentRunner` 对 Armillae Bridge 与 ToolExecutor 的组合；
- ToolDefinitions 的能力过滤；
- Loreloom Tool Handler 与 `WorldGateway` 调用；
- Armillae Bridge/Tool 类型转换和 Mock Agent 场景；
- 把 `BridgeError` 投影为不含原始错误正文的 `ModelFailureDiagnostic`。

依赖 `loreloom-core`、`armillae-llm` 和 `armillae-tools`；具体 `armillae-llm-rig`
Adapter 优先由二进制装配。不得依赖 Bevy 类型或持久化后端。

### 3.5 `loreloom-store`

拥有：

- 存档元数据、Snapshot、ordered record 与 Schema version；
- 原子写入的后端实现；
- Save Head/current Revision 的条件更新、ActionId 幂等和 commit outcome 分类；
- 加载验证、迁移、恢复和完整性检查；
- 存档列表、创建、打开、checkpoint、备份、恢复、切换和关闭。

依赖 `loreloom-core`。若领域 record codec 必须由 World 提供，应通过 Runtime 注册拥有所有权的
codec，不得让 Store 查询 Bevy World。候选后端 adapter 可以依赖 Toasty、公开发布的 SurrealDB
driver 与对应嵌入式引擎，但这些类型不得穿透 Core/World/Agent API。

第一阶段后端固定为 SurrealDB + SurrealKV，SQLite 只作为 P0 与回归测试对照；commit/failure/
backup 协议遵循第 11.4 节。领域 record Schema、未知字段和领域迁移仍未冻结，因此当前只能保留
测试专用 adapter/模型，不能提前建立生产 Store 公共 API。

### 3.6 `loreloom-runtime`

拥有：

- 应用生命周期和 Working World 的逻辑写入所有权；
- PlayerInput、NarratorPlan、NpcTurnRequest 和 RuntimeCommand 队列；
- 从同一 Revision 生成 CharacterContext、SceneContext 和 NpcAssignment 并创建临时 NpcAgent；
- Agent Step / Tool Loop 与 Narrator 编排状态机；
- Revision 冲突、提交、取消与故障策略；
- Content/Generation 请求的 Capability、数量和模型预算策略；
- 根世界加载、Mod 发现、依赖/Patch 顺序、内容哈希、Capability、导入事务和 save lock；
- Content、Store、World 与 Agent 的组合；
- Transcript 提交和 UiSnapshot 发布。

Runtime 可以依赖 Core、Content、World、Agent 和 Store，是应用策略的唯一组合层。

### 3.7 `loreloom-tui` 与 `loreloom`

TUI 拥有终端初始化/恢复、事件映射、布局、输入编辑、滚动与渲染。它只依赖 Core 暴露的
View Model 和 Runtime Client，不依赖 Content/World/Agent/Store 实现。

`loreloom` 二进制拥有根世界路径、配置读取、Secret 解析、Mod Package 来源、Provider Adapter、Store
Backend、Runtime 与 TUI 装配，以及进程退出顺序。

二进制默认从当前目录 `world.toml` 加载唯一主世界；`--world PATH` 只覆盖游戏根目录路径。
`--save PATH` 选择 SurrealKV 存档，`--headless INPUT` 执行一个完整 Turn；可重复的 `--mod PATH`
显式启用目录 Mod。生产运行必须配置 Provider；无 Secret Mock Bridge 只用于确定性测试。

## 4. Rust、Cargo 与依赖策略

- 根目录使用 Cargo virtual workspace，八个成员位于 `crates/`；七个 library crate 与一个
  `loreloom` binary crate 的名称和职责遵循第 3 节。
- 每个成员 Manifest 必须显式声明自己的 `package.version`，禁止 `version.workspace`；初始版本为
  `0.1.0`。
- Semifold 使用 Rust workspace resolver 与根级 `.changes/` 变更集管理成员版本；base branch 为
  `main`，release branch 为非 `main` 的 `release`。全部 workspace package 使用 `alpha` 预发布
  通道，首次进入通道时保留当前稳定版本基线。Semifold 必须直接更新各成员 Manifest 中的版本。
- 根世界文件位于仓库根级 `world.toml`、`content/`、`rules/` 与 `prompts/`；`mods/` 只放扩展，
  跨 crate 共享测试数据位于 `tests/data/`。
- Rust edition 固定为当前可用的 `2024`。
- 工具链 channel 为 `stable`，不固定 `1.x.y` 数字。
- 所有 Loreloom package 都不得设置 `package.rust-version`。
- CI 不建立 MSRV Job；每次运行使用当时最新 stable。
- GitHub Actions 必须在 pull request 和 `main` push 上执行格式、检查、Clippy、测试、文档与
  依赖/Secret 安全门禁；pull request 由 Semifold 报告发布计划，`main` push 由 Semifold 独占
  version/release branch 更新与 crates.io 发布。
- 二进制 workspace 提交 `Cargo.lock` 并在常规验证中使用 `--locked`。
- 新增 package 使用 `cargo new`，新增依赖使用 `cargo add -p`，移除使用 `cargo remove -p`。
- 依赖默认使用实现时最新且相互兼容的稳定版本；安全或兼容问题必须更新 lockfile 并验证。
- Armillae 依赖使用 registry 版本，或公开 Git URL 与明确 tag/revision；项目构建不得依赖
  未提交的外部状态。
- Agent Loop Spike 固定使用公开 `https://github.com/mmstudio-games/armillae` revision
  `c9896fc4eb3a4f37918c0cabcefc84f8dcd69137` 的 `armillae-core`、启用 `mock` feature 的
  `armillae-llm` 与 `armillae-tools`（package version `0.1.0-alpha.1`）；不得使用本地路径。
- Toasty/SurrealDB Store 候选同样只能使用 registry release，或公开 Git URL 与明确 tag/revision；
  未进入公开来源的本地提交不构成 Loreloom 可采用的依赖版本。
- `bevy_ecs` 版本必须与选定 `armillae-simulate-bevy` 发布线精确兼容。
- TUI 固定使用 registry `ratatui 0.30.2`（关闭 default features，启用 `crossterm` 与
  `unstable-rendered-line-info` 以按 Ratatui 实际折行计算滚动边界）、`crossterm 0.29.0` 与
  grapheme-aware 输入实现；P0 Spike 已验证输入、恢复和窄屏边界。
- 第一阶段只加载 Content/Rule Mod 数据，不加载 Rust/C 原生动态库；WASM Extension Host 必须
  由后续 RFC 和独立 capability/sandbox Spike 授权。
- 生产代码对可恢复路径不得使用 `unwrap()`。

## 5. 身份、版本与公共数据规则

### 5.1 Stable ID

跨世界记录使用不同语义的新类型，至少包括：

```rust
pub struct WorldId(String);
pub struct ObjectId(String);
pub struct ActorId(ObjectId);
pub struct ActionId(String);
pub struct EventId(String);
pub struct SessionId(String);
pub struct SaveId(String);
pub struct ModId(String);
pub struct ContentDefinitionId(String);
```

运行期生成的 ID 固定为 RFC 9562 UUIDv7，并使用 canonical lowercase hyphenated UUID 文本。Wire
格式为 `<prefix>_<uuid>`，前缀固定为 `wld`、`obj`、`act`、`evt`、`ses`、`sav`、`gen`、`ntr`、
`trn` 与错误 correlation 使用的 `err`；
因此当前格式长度固定为 40 ASCII bytes。解析必须拒绝大写、非 canonical 文本、错误前缀、非
RFC 4122 variant 或非 version 7 UUID。`ActorId` 是 `ObjectId` 的语义新类型，序列化仍使用同一
`obj_` 文本，避免一个角色同时拥有两份运行身份。

生成器使用进程时间与安全随机源生成 UUIDv7；Store/World 必须把重复 ID 当作 Identity 错误，
不能自动覆盖。排序按完整 canonical ASCII/UUID bytes，是稳定确定性顺序且通常具有时间局部性，
但不能替代 Revision、Clock 或因果关系。测试不得给全局生产生成器安装 Seed；确定性测试使用
固定的合法 v7 文本，或依赖注入只在测试场景使用的 `IdGenerator`。

`ModId` 不是 UUID：固定为 3–127 bytes 的 lowercase reverse-DNS ASCII 标识，至少两个 segment，
segment 以字母或数字开始/结束并可在中间包含 `-`。`ContentDefinitionId` 固定为
`<mod-id>:<kind>/<local-key>`，总长不超过 255 bytes；kind 与 local key 使用 lowercase ASCII，
local key 可用 `/` 分层但不得含空、`.` 或 `..` segment。它们按 UTF-8 bytes 排序。Mod 版本与
hash 由 ModLock 记录，不进入 Definition ID。

所有 ID 必须是拥有所有权、可序列化、`Eq + Ord + Hash` 的领域值。禁止把
`bevy_ecs::entity::Entity`、指针、数组索引或显示名称作为 Stable ID。

Content Definition ID 与运行 ObjectId 分离。Definition ID 必须包含或规范关联 Mod 命名空间，
版本和内容哈希由 Mod lock/Registry 记录，不靠显示名称或加载顺序决定身份。

### 5.2 Revision

`Revision` 表示一个已提交世界版本：

```rust
pub struct Revision(u64);
```

- 新世界从规范化的 Revision `0` 开始；第一个成功修改提交生成 Revision `1`；
- 每个成功的修改提交生成严格递增 Revision；
- Query 可以声明读取的 Revision；
- 修改 Command 必须携带 `expected_revision`；
- Runtime 不能把过期 Command 自动当成当前 Command 执行；
- Revision 只在一个 World/Save 内比较，不是全局时间戳。

### 5.3 Record envelope

持久化领域记录至少携带：

- record type；
- schema version；
- stable object identity；
- payload；
- 可选 provenance；
- 所属 Snapshot/Revision。

Wire 字段使用 `snake_case`，枚举使用显式 `type` tag。Envelope 固定包含
`record_type`、非零 `schema_version`、`record_id`、`revision`、`payload` 与可选 `provenance`，
并拒绝未知 envelope 字段。`record_type` 是最长 64 bytes 的 lowercase snake_case ASCII；
`payload` 顶层必须为 JSON object。当前版本的领域 codec 必须拒绝未知 payload 字段；未知 record
type 或高于运行时支持版本的 record 必须拒绝 Load，不能 opaque-pass-through 后发布 World。

首个公开版本发布前，当前领域 payload Schema 直接固定为 v1。开发期发生破坏性变化时覆盖 v1
codec，旧开发存档直接拒绝并重建；不得为开发期数据分配 v2、注册 legacy migration 或保留兼容
Load 分支。

首个公开版本发布后，旧 payload 才能通过注册的连续 `vN -> vN+1` migration 链升级。Migration
必须纯、确定性、无 Provider/墙钟/随机 I/O，逐步保留 stable identity，并在全部 record、WorldLock、
ModLock、引用和领域不变量通过后才在一个显式事务/checkpoint 中发布升级结果；缺失步骤和 downgrade
均拒绝。原存档在完整升级发布前保持可恢复。

后端可以用原生 JSON 保存版本化 payload，但“原生 JSON”只是无损存储表示，不授权任意无 Schema
属性袋。嵌套对象和数组由具体 codec 明确约束；领域数值只使用有范围的 integer 或第 6.4 节 Fixed，
不保存 JSON float。`null` 只允许出现在 Schema 显式 nullable 的字段，普通 `Option` 缺失使用字段
omission；SurrealDB `NONE` 不是 JSON payload 值，读到 payload/envelope 必需列为 `NONE` 必须视为
corruption。超出 codec 所声明 i64/u64 范围的大整数必须拒绝。

## 6. Working World 领域模型

### 6.1 必需基础数据

最小纵向切片必须能表达：

| 数据 | 最小语义 |
|---|---|
| `PersistentId` | Stable ObjectId 与当前 Entity 的关联 |
| `ObjectKind` | Character、Place、Item、Relation 或领域扩展 |
| `DisplayName` | 玩家可见名称；不作为身份 |
| `LocatedAt` | 对象当前所在 Place/Object |
| `CharacterProfile` | 非秘密的基础角色设定与叙事风格引用 |
| `Controller` | Player、Agent 或 Rules；不携带 Provider Client |
| `BaseAttributes` | 经 Definition 注册的机械基础属性，不包含最终加成 |
| `ResourcePool` | Health、Stamina 等 current/base maximum |
| `ConditionInstance` | 状态效果、来源、层数、强度和基于 World Clock 的期限 |
| `LifeState/ActionState/Posture` | 可同时存在的正交角色状态机 |
| `InventoryOwner/Container/ContainedBy` | 背包入口、容器能力与唯一物理容器关系 |
| `ItemInstance/Stack/Durability` | 物品定义引用、数量和实例可变状态 |
| `OwnedBy/Equipped` | 与物理容器分离的所有权和装备状态 |
| `SkillGrant/SkillCooldown` | 角色被授予的技能、来源、等级、熟练度和冷却 |
| `ParameterSet` | 经模组 Parameter Schema 校验的类型化扩展参数 |
| `EventInstance` | 当前事件 Definition、节点、生命周期和已提交选择 |
| `RuleState` | 不能从其它事实重建且会影响未来规则执行的状态 |
| `Relationship` | 两个对象之间带类型与强度的关系实体 |
| `KnownFacts` | Actor 私有的结构化认知及来源 |
| `Goals` | Actor 的结构化目标、状态和优先级 |
| `WorldClock` | 当前世界时间或第一阶段选定 Clock |

最终 Rust 类型、规范化程度和数据规模必须在建立对应公共 API 前通过最小场景复核。不得用一个无 Schema
的 `HashMap<String, Value>` 代替全部领域建模；允许扩展值时也必须有命名空间、版本和校验。

### 6.2 物品、容器与装备

物品必须区分静态 Definition 和运行实例。候选内容协议：

```rust
pub struct ItemDefinition {
    pub id: ItemDefinitionId,
    pub display_name: String,
    pub tags: Vec<ItemTag>,
    pub stack_limit: u32,
    pub unit_weight: Weight,
}
```

Item Definition 属于版本化内容注册表，不为世界中的每件物品复制完整定义。存档必须记录所使用
的 content version；Definition 使用第 10.2 节冻结的 Content Document v1，迁移只按显式 content
schema version 执行。

每件或每组实际存在的物品必须是具有 Stable ObjectId 的 ECS Entity。第一阶段候选 Component：

```rust
pub struct ItemInstance {
    pub definition_id: ItemDefinitionId,
}

pub struct Stack {
    pub quantity: u32,
}

pub struct Container {
    pub capacity: Capacity,
    pub max_weight: Weight,
}

pub struct ContainedBy {
    pub container_id: ObjectId,
}

pub struct OwnedBy {
    pub owner_id: ObjectId,
}

pub struct Equipped {
    pub wearer_id: ActorId,
    pub slot: EquipmentSlotId,
}
```

规范要求：

- Character 的 `InventoryOwner` 只引用一个根 Container，不包含权威 `Vec<ItemId>`；
- `ContainedBy` 是物理容器关系的单一事实源，容器内容由 Query 反向求得；
- `OwnedBy`、`ContainedBy` 和 `Equipped` 分别表示所有权、物理位置和使用状态；
- 一个物品同一 Revision 最多存在一个 `ContainedBy`，没有容器时必须通过 `LocatedAt` 或明确的
  transit 状态定位；
- Container 可以嵌套，但提交前必须拒绝自包含和任意长度的容器循环；
- 堆叠数量必须大于零且不超过 Definition 上限；
- 只有 Definition 和所有影响行为的实例状态等价时才允许合并堆叠；
- 拆分堆叠会创建新的 Stable ObjectId，并用 WorldEvent 记录来源和数量；
- 容量、总重量、装备派生属性和可用物品列表是 Query/System 派生值，不作为第二份权威状态；
- 耐久、自定义名称、附魔、任务绑定等会影响行为的实例数据必须使用有版本的 typed Component，
  不能放入无 Schema JSON bag。

### 6.3 技能、授予关系与冷却

技能必须区分静态 Definition、角色 Skill Grant 和规则 Executor。候选内容协议：

```rust
pub struct SkillDefinition {
    pub id: SkillDefinitionId,
    pub display_name: String,
    pub kind: SkillKind,
    pub cost: SkillCost,
    pub cooldown: CooldownSpec,
    pub executor_id: SkillExecutorId,
}

pub enum SkillKind {
    Active,
    Passive,
    Reaction,
}
```

Skill Definition 属于版本化内容注册表。`executor_id` 是稳定标识，在启动阶段映射到已注册的
Native System/handler；Definition、ECS 和存档不得保存函数指针、Provider Client 或 Rust
Trait Object。

角色获得技能时必须建立有 Stable ObjectId 的 Skill Grant：

```rust
pub struct SkillGrant {
    pub owner_id: ActorId,
    pub skill_id: SkillDefinitionId,
    pub rank: u32,
    pub proficiency: u32,
    pub source: SkillSource,
    pub enabled: bool,
}

pub struct SkillCooldown {
    pub ready_at: WorldTime,
}
```

规范要求：

- 同一 Skill Definition 可以因不同来源产生不同 Grant；合并规则在内容 Schema 中明确；
- Active Skill 只通过显式 `UseSkillCommand` 执行；
- Passive Skill 由确定性领域 System 参与计算，不需要 LLM 调用；
- Reaction Skill 只在明确的 Event/Reaction Window 中执行，不自动启动 Agent；
- 冷却使用 World Clock 或领域 Clock，不使用不可回放的进程墙钟；
- 执行 Active Skill 前必须验证 Grant 所有者、enabled、资源、冷却、目标、可见性、距离和状态；
- 成功执行必须在同一世界执行边界内扣除资源、设置冷却并生成结构化 WorldEvent；
- Active Skill 的原子执行顺序固定为：先扣除有序 Resource Cost 并为每项 Cost 生成
  `ResourceChanged`，再通过通用白名单 Effect executor 执行完整 Effect plan，最后设置冷却并生成
  `SkillUsed`；由此产生的全部 Event 随后进入同一 candidate 的确定性 Rule cascade；
- Cost、任一 Effect、冷却计算、Event 构造、Rule cascade 或最终世界校验失败时，整个 candidate
  回滚，不得保留已扣资源、已创建对象、Parameter、Condition、冷却或部分 Event；
- 角色未拥有对应 Grant 时，不能凭 Skill 名称、Definition ID 或模型文本使用技能；
- 最终属性、装备加成、技能可用性和效果预览属于派生数据，可缓存但必须可从权威状态重建。

Skill Cost 固定为有序的 Resource Cost 列表，每项 amount 必须大于零；同一 Resource 不得重复。
Target Schema 固定为 `self`、`character`、`object` 或 `place` tagged variant，并为非 self target
声明 allowed kind、是否允许 self 与非负最大 range。第一阶段距离只使用同 Place 或已注册 Scene
graph distance，不让 Agent 提交距离结果。

Active Skill 使用已编译白名单 Effect plan；Passive Skill 把同一计划注册到确定性 modifier/query
入口；Reaction Skill 必须声明 Event type、Predicate 和每 World tick/Action 的触发上限。Reaction
Window 只在当前 WorldCommand 的 candidate cascade 内存在，结束后不持久，也不调用 Agent。
`executor_id` 只能引用 Engine 启动时注册的 allowlist；数据 Mod 可提供 typed config/effect plan，
不能提供函数、脚本、动态 Tool 或 executor code。

### 6.4 属性、资源、Condition 与正交状态

属性必须由版本化 Definition 注册，并使用可确定性序列化/计算的数值类型：

```rust
pub struct AttributeDefinition {
    pub id: AttributeId,
    pub minimum: Fixed,
    pub maximum: Fixed,
    pub aggregation: AggregationPolicy,
}

pub struct BaseAttributes {
    pub values: BTreeMap<AttributeId, Fixed>,
}
```

`BTreeMap` 只允许已注册 AttributeId 和经过 Definition 校验的 Fixed 值，不是无 Schema
`String -> JsonValue` 扩展袋。Fixed 固定为 signed i64 raw micros，全局 scale `1_000_000`，Wire
保存 integer micros 而不是 JSON float/decimal string。加减与最终结果使用 checked i64；乘除使用
i128 中间值并按 round-to-nearest、ties-to-even 回到 micros，除零或越界拒绝完整 candidate。
显示格式不是持久协议。WorldTime 固定为从 0 开始的 u64 逻辑秒 tick，只由显式 Command/System
推进，tick addition overflow 拒绝。

Modifier 必须携带 attribute、operation、value、priority 和稳定 source ID。有效属性按以下
规范顺序求值：

```text
Base
  -> Flat Addition
  -> Multiplication
  -> Override
  -> Definition Clamp
```

同一阶段按 `priority -> source_id` 稳定排序。装备和 Skill Definition 中可重建的 Modifier 不
重复保存为独立权威值；临时/永久的自由调整若没有其它来源事实，必须使用带来源的持久
`AttributeAdjustment`。

`EffectiveAttributes` 可以作为当前 Revision 的派生缓存，但不得进入存档或成为 Command 输入
中的独立权威值。Base、装备、Condition、Skill Grant 或 Definition 改变时必须失效并重算。

频繁变化的资源单独保存：

```rust
pub struct ResourcePool {
    pub resource_id: ResourceId,
    pub current: Fixed,
    pub base_maximum: Fixed,
}
```

`current` 是权威持久状态；effective maximum 可由 base maximum 与 Modifier 推导。Resource
Definition 必须选择 `clamp_current`、`preserve_ratio` 或 `allow_overcap` maximum policy。
`clamp_current` 在 effective maximum 下降时把 current 同 candidate 原子 clamp；
`preserve_ratio` 以前后 maximum 比例、Fixed ties-to-even 调整 current；`allow_overcap` 保留 current
但禁止继续增加，直到不再超限。所有 policy 都要求 current 非负，effective maximum 必须大于零。

Condition 同样使用 Definition + Instance：

```rust
pub struct ConditionDefinition {
    pub id: ConditionDefinitionId,
    pub tags: Vec<ConditionTag>,
    pub stack_policy: StackPolicy,
    pub duration: DurationPolicy,
    pub modifiers: Vec<ModifierDefinition>,
    pub executor_id: Option<ConditionExecutorId>,
}

pub struct ConditionInstance {
    pub id: ObjectId,
    pub target_id: ActorId,
    pub condition_id: ConditionDefinitionId,
    pub source: ConditionSource,
    pub stacks: u32,
    pub intensity: Fixed,
    pub applied_at: WorldTime,
    pub expires_at: Option<WorldTime>,
}

pub enum StackPolicy {
    Unique,
    RefreshDuration,
    IncreaseStacks { maximum: u32 },
    IndependentInstances,
}
```

要求：

- Condition Instance 具有 Stable ObjectId；
- Duration 使用 World Clock/领域 Clock，不使用进程墙钟；
- source 必须关联 Item、Skill、Environment、Character 或明确的系统来源；
- stacks 必须满足 Definition；合并/刷新/独立实例不能由调用者临时选择；
- 周期效果、触发和过期通过注册的 System/Clock entry 执行，不自动调用 Agent；
- Condition Definition 的 executor 使用稳定 ID，持久化不保存函数指针或 Trait Object；
- Condition 真相与 Known Fact 分离，Observation 根据感知/诊断只投影名称或症状。

`Unique` 在已有实例时拒绝；`RefreshDuration` 保持一个 instance/stacks=1 并重算 expiry；
`IncreaseStacks` 保持一个 instance、加到 Definition maximum，并由 Definition 决定是否 refresh；
`IndependentInstances` 每次创建新 ID 且受 Definition maximum instances 限制。Intensity merge 固定由
Definition 选择 `keep`、`replace` 或 `maximum`。Duration 只允许 `permanent` 或非零 finite ticks；
periodic effect 声明非零 interval 和白名单 Effect plan，按 `(next_tick, definition_id, instance_id)`
稳定执行。`AdvanceTime` 必须让 Working World Clock 按跨越的 scheduler boundary 前进；每个 boundary
先按 `(definition_id, instance_id)` 执行该 tick 的 periodic Effect，再重新读取 Condition 并按同一
顺序过期仍满足 `expires_at <= boundary` 的实例。因而 `next_periodic_at == expires_at` 时固定先执行
一次 periodic，再过期；periodic Effect 作用于该实例的 `target_id`，下一次 tick 从本次 scheduled
tick checked-add interval 计算，而不是从 Command 最终时间重新起算。跨越多个 interval 时逐次执行，
不能合并为一次 Effect。每次周期执行生成包含 instance ID 与 scheduled WorldTime 的结构化
`ConditionTicked` Event；最终 Clock 再前进到 Command 目标时间。过期、周期 Effect、stack/refresh、
Event、Rule cascade 和最终 Clock 更新属于同一 candidate，任一失败全部回滚。

互相可并存的角色状态必须使用正交类型：

```rust
pub enum LifeState {
    Alive,
    Downed,
    Dead,
}

pub enum ActionState {
    Idle,
    Acting(ActionId),
    Waiting,
}

pub enum Posture {
    Standing,
    Sitting,
    Prone,
}
```

不得使用单个互斥 `CharacterState` 同时表达生命、动作、姿态、疲劳、中毒和战斗等不同轴。
CharacterProfile 保存背景、价值观、性格和说话风格；只有规则相关 Trait 才通过明确 Modifier
影响 Attribute。LifeState、ActionState 和 Posture 是持久事实；转换必须由领域 Command/System
校验，Narrator 或 NpcAgent 文本不能直接覆盖。

Narrator 只可在 CharacterSpawnSpec 中提供受约束 Attribute/Resource/Condition Hint；
NpcFactory 必须按 Archetype、预算、上下限和允许的 Definition 校验。角色创建后不向 Agent
开放通用 `set_attribute`、`set_health` 或 `set_condition`；物品、技能和世界行动通过领域
Command/System 间接改变这些状态。

### 6.5 模组参数、事件与声明式规则

模组可扩展状态使用 Definition + Instance，不创建运行时 Rust Component 类型。第一阶段逻辑参数
协议为：

```rust
pub struct ParameterDefinition {
    pub id: ContentDefinitionId,
    pub value_type: ParameterType,
    pub default: ParameterValue,
    pub constraints: ParameterConstraints,
    pub visibility: ParameterVisibility,
    pub persistence: ParameterPersistence,
}

pub enum ParameterType {
    Bool,
    Fixed,
    Counter,
    Enum { variants: Vec<ParameterVariantId> },
    TagSet { allowed: Vec<ParameterTagId> },
    ObjectRef { allowed_kinds: Vec<ObjectKind> },
}

pub struct ParameterSet {
    pub schema_id: ContentDefinitionId,
    pub values: BTreeMap<ContentDefinitionId, ParameterValue>,
}
```

`ParameterValue` 必须是与 Definition 精确匹配的拥有所有权 tagged value；不得包含任意 JSON、
Bevy Entity、函数、Provider 类型或未验证对象引用。缺省值、范围、枚举/标签集合、引用种类、
对 Actor 的可见性和保存策略都由 Definition 决定。改变参数只能经领域 Command/System，不能由
Mod 文本或 LLM 直接 Patch `ParameterSet`。

Parameter Type 第一阶段固定为 Bool、Fixed、Counter、Enum、TagSet 和 ObjectRef：Fixed 必须与
Definition 的 scale 完全相同并在范围内；Enum/TagSet 只能取声明项且 TagSet 有 cardinality 上限；
ObjectRef 同时校验 Stable ObjectId 存在和 ObjectKind 白名单。类型不匹配、未知 tagged variant、
额外控制字段和任意 JSON object 均拒绝。Fixed 的最终位宽/舍入仍服从第 6.4 节门禁。

事件必须区分静态 Definition 与运行实例：

```rust
pub struct EventDefinition {
    pub id: ContentDefinitionId,
    pub trigger: TriggerDefinition,
    pub entry_conditions: Vec<PredicateDefinition>,
    pub nodes: Vec<EventNodeDefinition>,
}

pub struct EventOptionDefinition {
    pub id: ContentDefinitionId,
    pub visible_if: Vec<PredicateDefinition>,
    pub enabled_if: Vec<PredicateDefinition>,
    pub effects: Vec<EffectDefinition>,
    pub next_node: Option<ContentDefinitionId>,
}

pub struct EventInstance {
    pub id: ObjectId,
    pub definition_id: ContentDefinitionId,
    pub current_node: ContentDefinitionId,
    pub scene_id: Option<ObjectId>,
    pub started_at: WorldTime,
    pub status: EventStatus,
}
```

选择 Event Option 的 Command 必须携带 Event Instance ID、Option ID 和 expected Revision。
World 必须在 current Revision 重新求值 visible/enabled Predicate；旧 UiSnapshot 或 Observation 中
曾经可选不代表当前仍可执行。成功 Effect 形成带 Mod/Rule provenance 的 WorldCommand/Event，
失败不得推进 Event Instance。一个 Option 的 Effect plan 与节点推进必须作为一个复合
WorldCommand 形成 all-or-reject 领域结果，并按第 11.4 节形成一个 durable unit。

第一阶段 Rule Bundle 只允许注册引擎支持的有限节点：

- Trigger：已提交 WorldEvent、World Clock、Scene 进入/离开和显式 Gameplay Action；
- Predicate：类型化属性/资源/参数比较、Tag/Condition/关系/位置/认知存在性与布尔组合；
- Effect：引擎白名单中的参数调整、Condition、物品/技能授予、关系/认知变化、Spawn、
  Scene/Event 推进或其它类型化 WorldCommand；
- Gameplay Action：有稳定 Definition ID、参数 Schema、Capability 和 Effect plan 的显式入口。

编译后的规则按规范化 `priority -> definition_id -> instance_id` 顺序执行，并具有最大触发数、
Predicate 节点数、Effect 数和级联深度预算。第一阶段拒绝递归规则和静态可检测循环；超过预算时
返回结构化 Rule 错误并丢弃完整 candidate cascade，不发布前半段 Effect。默认预算为单 Rule 最多
64 个 Predicate 节点、32 个 Effect；单次 cascade 最多触发 128 个 Rule、求值 1,024 个 Predicate、
应用 512 个 Effect、深度 8。Host 配置可以收紧，Mod 不可扩大。规则随机性必须使用可记录 RNG
Context。Runtime/World 为
每次执行注入可信 rule initiator、Mod/Rule ID、source Event/Clock/Scene 和 Action provenance；
数据 Mod 参数不能覆盖 Actor、System principal、ActionId 或 Revision。

Gameplay Action Definition 只声明稳定 ID、有界 typed parameter Schema、Capability 和已编译 Effect
plan。第一阶段 Runtime 只注册 `list_gameplay_actions`/`perform_gameplay_action` 等引擎通用 Tool；
Definition ID 不是动态 Tool 名。参数缺失、额外参数、类型/范围错误或任一 Effect 失败均不推进
Revision。

产品 executor 固定把 Predicate 与 Effect 的隐含主体绑定到可信 Runtime 提供的 acting Actor；
`ResourceAtLeast`/`HasCondition` 查询该 Actor，`HasTag` 查询该 Actor profile 与当前 Place。第一阶段
`ResourceDelta`、`ApplyCondition`、`GrantItem` 和 `GrantSkill` 也只作用于该 Actor；Mod 参数不能改写
target。`SetParameter` 按 Parameter Definition 的 ContentOrigin pack 分组到全局 ParameterSet：`save`
值进入 RecordOp，`session` 值只存在当前 Runtime session 的 ECS 工作状态且不得进入 checkpoint/
RecordOp；Runtime 在 candidate 失败恢复时必须保留最后已提交的 session overlay，而新进程从 default
重新开始。ParameterSet 的 `schema_id` 固定为定义它的 pack ID，values 只能包含同 pack 且同
persistence 组的 Parameter ID。

`PerformGameplayAction` 参数是 `BTreeMap<ContentDefinitionId, ParameterValue>`，必须与 Action 的
parameter ID 精确匹配：required 缺失、额外 key、类型/范围或 ObjectRef kind 错误全部拒绝。Runtime
在构造 Command 前把 Action `capability` 与可信 Agent/Narrator capability set 比较；World 不接受
模型提供的 capability 声明。`ChooseEventOption` 的 Effect、Option history、next node（没有 next
node 时完成实例）与所有触发 Rule 属于同一个 candidate Revision。

每个成功 Command 先产生基础领域 Event，再以其规范 event type 触发 `WorldEvent` Rule；Gameplay
Action、跨 Scene Move 和 Clock boundary 分别追加对应 Trigger。匹配 Rule 按
`priority -> rule definition ID -> RuleState instance ID` 执行；同一 Rule 的 Predicate 失败只跳过该
Rule，执行错误或预算耗尽回滚完整 Command candidate。`EmitEvent` 产生带 source Rule ID 的领域
Event 并按 FIFO、depth+1 继续 cascade；Content 编译期拒绝静态可检测的 EmitEvent cycle，运行时
预算仍是最终防线。每个实际执行的 Rule 更新唯一 RuleState 的 trigger count/last tick，并产生可
审计的 Rule Event；Load/Replay 只恢复这些记录，不重新触发。

Definition Registry 在一个活动世界版本内不可被隐式替换。启停 Mod、应用 Patch 或热重载会
改变规则/Schema，只能通过未来明确的迁移与世界重建边界执行，不能在 Schedule 中间替换。

### 6.6 关系与认知

- 关系优先建模为有 Stable ID 的 relation entity，以便携带来源、方向、强度和历史；
- 世界真实关系与 Actor 对关系的认知必须可区分；
- Known Fact 必须指明 subject、predicate/value、source 和状态；
- “模型在上一轮说过”不能自动创建 Known Fact；
- Tool/System 只有在规则允许时才能创建、确认、反驳或遗忘认知；
- Observation Builder 只能读取当前 Actor 被允许的认知和感知结果。

第一阶段 `KnownFact` 是有 Stable ObjectId 的 typed record，包含 owner、`world | object | scene`
subject、ContentDefinitionId predicate、`bool | fixed | counter | bounded_text | object_ref | tag`
value、`believed | confirmed | disputed | forgotten` epistemic status、0..1 confidence、source 与首次/
最近确认 WorldTime。Belief 不另建隐藏字符串 Memory；它就是 status/confidence 未达到 confirmed 的
KnownFact。`forgotten` 保留 tombstone/provenance 但不进入普通 Observation。

Goal 是有 Stable ObjectId 的 record，包含 owner、有界 description、i32 priority、
`active | achieved | abandoned | blocked` 状态、source 与更新时间。Relationship 是有 Stable ObjectId
的有向 record，包含 source/target、Definition kind、Fixed strength 与 provenance。

TranscriptItem 使用独立 Stable ID，包含 Session、可选 committed Revision、Player/Narrator/Actor/
System speaker、有界正文、`committed | interrupted` 状态和可见 supporting Event ID。Transcript 是
对话档案而非结构化世界事实；引用它或模型上一轮文本不会自动创建 KnownFact。

### 6.7 不变量

- 同一 Working World 内 Stable ObjectId 唯一；
- 所有持久引用在提交时必须指向存在对象，或使用规范明确允许的 tombstone；
- Character Controller 同一 Revision 只有一个有效模式；
- Container/Location 关系不得形成规范禁止的循环；
- Inventory Query 只能由 `ContainedBy` 事实推导，不得与 Character 中的重复列表合并；
- Skill 使用必须引用 Actor 当前持有且启用的 Skill Grant；
- Base Attribute、Resource 与 Condition 必须引用已注册 Definition；
- Effective Attribute 缓存必须能由当前 Revision 的权威来源重建；
- Condition expiry 不能依赖进程墙钟，Life/Action/Posture 不得互相覆盖；
- 所有 Definition 引用必须解析到存档固定的 Mod lock/Registry，不能按加载顺序漂移；
- ParameterSet 的值、引用和可见性必须符合其版本化 Parameter Definition；
- Event Option 在执行 Revision 必须仍属于当前节点并满足 visible/enabled Predicate；
- 声明式 Rule 不能越过预算、白名单 Effect 或 Runtime Capability；
- 已删除对象的引用处理必须显式，不能依赖 Bevy despawn 后的随机失败；
- 规则随机性必须使用 Runtime 提供的可记录 RNG Context。

## 7. Command、Event 与 World 执行

### 7.1 Command envelope

修改世界的候选协议：

```rust
pub struct WorldCommand {
    pub action_id: ActionId,
    pub actor_id: ActorId,
    pub expected_revision: Revision,
    pub kind: WorldCommandKind,
}

pub enum SceneTransitionTarget {
    Existing { scene_id: ObjectId },
    Definition { scene_definition_id: ContentDefinitionId },
}

#[non_exhaustive]
pub enum WorldCommandKind {
    Move { destination_id: ObjectId },
    TransferItem { item_id: ObjectId, container_id: ObjectId },
    EquipItem { item_id: ObjectId, slot_id: ContentDefinitionId },
    SplitStack { item_id: ObjectId, quantity: u32 },
    UseSkill { grant_id: ObjectId, target: SkillTargetRef },
    AdvanceTime { ticks: u64 },
    SpawnCharacter { spec: Box<CharacterSpawnSpec> },
    PromoteCharacter { actor_id: ActorId },
    TransitionScene { target: SceneTransitionTarget },
    AppendTranscript { items: Vec<TranscriptItemRecord> },
    ChooseEventOption {
        event_instance_id: ObjectId,
        option_id: ContentDefinitionId,
    },
    PerformGameplayAction {
        action_id: ContentDefinitionId,
        arguments: BTreeMap<ContentDefinitionId, ParameterValue>,
    },
}
```

以上是第一阶段已经冻结的最小产品命令集合；UseItem、Knowledge/Goal 专用 Command 在对应 executor
实现时按本节共同规则增量加入，不能用任意 Component patch 代替。
`AppendTranscript` 只供可信 Runtime 保存 Player/Narrator/Agent 输出：Runtime 分配 TranscriptItemId、
Session/Revision 和 speaker binding，模型不能直接构造该 Command。每个 Command 必须：

- 从可信 Runtime Context 获得 Actor 与 Revision，模型参数不能覆盖；
- 完成 Schema、Capability、对象存在、可见性和规则前置条件校验；
- 由明确的 Simulation entry 执行；
- 返回结构化 Outcome；
- 在成功提交后生成一个或多个 WorldEvent。

Item Command 必须以实例 Stable ID 操作，并由 World 校验容器、所有权、容量、堆叠、装备槽和
物品状态。`UseSkill` 必须引用 Skill Grant Stable ID，不能让模型用任意字符串选择未授予技能。
Tool 参数中的 Actor、Action 和 Revision 不得覆盖可信 Runtime Context。

`ChooseEventOption` 必须验证 Event Instance/current node/Option/Predicate；
`PerformGameplayAction` 必须引用当前 Mod lock 中已注册且对 Actor 授权的 Action Definition，并按
其参数 Schema 校验 `ParameterInput`。两者都只能进入已编译的规则入口，不能携带脚本或 Effect
列表。

### 7.2 Outcome

`CommandOutcome` 至少区分：

- `committed`：包含新 Revision、生成 Event ID 和安全的 ToolResult projection；
- `rejected`：规则或权限拒绝，世界 Revision 不变；
- `conflict`：expected Revision 过期；
- `invalid`：参数或引用无效；
- `failed`：执行/存储基础设施失败，是否改变 Working World 必须由提交协议说明。

模型可见的 ToolResult 不得默认包含所有内部错误、私有对象或 Store 细节。

### 7.3 Event

WorldEvent 是已经发生的领域事实，不是待执行命令。它必须包含稳定 Event ID、Action ID、
Actor、Revision、事件类型、拥有所有权的 payload 与必要 provenance。

`TransitionScene` 成功至少产生 `SceneLeft { scene_id }`、`SceneEntered { scene_id }` 和玩家的
`CharacterMoved` Event；Scene Rule trigger 只由前两个显式 Event 驱动，不能把普通 Persistent NPC
跨 Scene 移动误判成世界 active Scene 的切换。

重建的唯一事实源固定为“目标 Revision 之前最近的已验证完整 checkpoint + 其后的连续有序
`RecordOp`”。每个修改 Revision 的 RecordOp 具有 action identity 和从零开始的无缺口 order，
只允许 `upsert` 完整 versioned record 或按完整 record key `delete`；重复 order、Revision 缺口、
checksum/digest 不匹配或删除不存在的 required record 都使重建失败。

WorldCommand 作为已接受输入、request digest 与 ActionCommit 幂等依据保存，但 Load/Replay 不重新
执行 Command，因为新版规则代码可能改变结果。WorldEvent 作为已经发生的领域事实、Agent 叙事
provenance 与审计记录保存，但不是第二套状态重建输入。Replay 不运行 Agent、Provider、Tool、Rule
或领域 executor；World 只从重建后的拥有所有权 record 重新物化 ECS，再验证 canonical record
投影等价。

## 8. Observation 与上下文组装

### 8.1 Observation

Observation 是某个 Actor 在某个 Revision 上的拥有所有权快照，至少包含：

- Actor 自身允许读取的状态；
- 当前地点与可感知对象；
- 相关的世界时间、最近事件和局部规则提示；
- Actor 的 Known Facts、Goals 和关系投影；
- 规则相关 Effective Attribute、Resource current/maximum 和允许识别的 Condition/症状；
- Actor 可见的模组 Parameter、活动 Event Option 和允许执行的 Gameplay Action 摘要；
- 有界背包摘要和当前可用技能视图；
- 当前请求或触发；
- 允许的 Tool Capability 摘要。

Observation 不包含：

- Bevy Entity、Query、Component 引用；
- 未授权 NPC 私有认知；
- Provider Secret 或 Store 连接信息；
- 任意完整 World dump；
- 仅为调试而存在的内部 Schedule/Component 信息。

Observation 必须携带 Revision。生成后 World 可以继续演化，但由它产生的修改必须通过
`expected_revision` 检测冲突。

NarratorAgent 接收以 Scene 和已提交事件为中心的 `SceneObservation`，不得默认读取所有 NPC
私有认知。NpcAgent 接收 `NpcContext`：其中 CharacterContext、SceneContext 和 NpcAssignment
必须绑定同一 Revision，Inventory/Skill 仍使用有界摘要与按需 Tool。

### 8.2 Context assembler

Context assembler 必须按确定顺序组合：

1. 产品安全与 Tool 规则；
2. 角色 Profile 与风格；
3. 本轮 Observation；
4. 相关结构化长期事实；
5. 有界近期 Transcript；
6. 当前触发/玩家输入；
7. ToolDefinitions。

裁剪策略必须优先保留规则、身份、当前状态和 Tool 合约，再裁剪较旧的展示文本。不得让摘要
覆盖结构化 World Fact。来自 Mod 的 Profile、文本和规则说明属于不可信内容，不能覆盖第 1 步
的产品安全与 Capability。第一阶段不调用摘要模型，也不保存模型生成的自由文本 Memory；完整
Transcript 分页保存在 Store，Agent Context 使用确定性有界投影。默认投影上限为 64 条/64 KiB
近期 Transcript、256 条 KnownFact、64 个 Goal、128 个可见对象/背包条目和 64 个可用 Skill，
Context 总预算默认 32,768 tokens。Host 配置可按模型收紧这些值或在资源允许时提高到编译时硬上限
的 4 倍；Model/Mod/Agent 请求不能改预算。超限按“安全/Tool 合约、身份、当前状态、当前输入、
Knowledge/Goal、最近 Transcript”的保留顺序确定性裁剪，并在 Observation metadata 标记 truncation。

产品 Runtime 使用以下 Host policy；`0` 可以显式关闭对应可选集合，超过默认值四倍则在配置解析
阶段拒绝。Runtime 按 Stable ID 排序后裁剪物品、Skill、KnownFact、Goal 与可见 Actor，Transcript
按 `(Revision, TranscriptItemId)` 保留最新窗口并同时满足条数和正文 UTF-8 byte 上限。
`SceneObservation.truncated` 与 `NpcContext.truncated` 只表示本次投影发生裁剪，不写入 ECS/Store，
也不允许模型通过 Tool 改写。

```rust
pub struct ContextProjectionPolicy {
    pub transcript_items: usize,       // default 64, hard max 256
    pub transcript_bytes: usize,       // default 64 KiB, hard max 256 KiB
    pub known_facts: usize,            // default 256, hard max 1,024
    pub goals: usize,                  // default 64, hard max 256
    pub visible_actors: usize,         // default 128, hard max 512
    pub inventory_items: usize,        // default 128, hard max 512
    pub skills: usize,                 // default 64, hard max 256
    pub max_context_tokens: u64,       // default 32,768, hard max 131,072
}
```

条目裁剪先于 Provider call；`max_context_tokens` 作为单次投影的独立 Host 上限，并与 Turn 的累计
`max_input_tokens` 同时生效。Provider 不提供调用前精确 tokenizer 时，Runtime 使用确定性、偏保守的
UTF-8/Unicode scalar estimate 做 preflight，Provider 返回 usage 后仍以实际 token usage 执行预算
校验。若保留安全规则、身份、当前状态、当前输入与 Tool 合约后仍超限，则在调用 Provider 前返回
结构化 context budget error，不能继续静默裁掉这些必需内容。

## 9. Tool 规范

### 9.1 分类

每个 Tool 注册时必须声明：

- 稳定名称和版本；
- Query、Command、Orchestration 或 Service 分类；
- 参数 JSON Schema；
- 所需 Actor Capability；
- 是否可能修改世界或访问外部系统；
- 结果的模型可见投影；
- 超时和大小上限。

第一阶段不得注册 Service Tool。

### 9.2 Query Tool

Query Tool 只读取 Runtime 在指定 Revision 提供的 Query/Observation Gateway：

- 不直接持有 `World`；
- 不扩大 Actor 可见范围；
- 不推进 Clock 或触发 Schedule；
- 返回拥有所有权、可序列化且有大小上限的结果；
- 读取目标 Revision 已不可用时返回结构化 conflict/unavailable。

### 9.3 Command Tool

Command Tool 只构造和提交 WorldCommand：

- ActorId、SessionId、ActionId 与 Revision 来自 ToolContext；
- 模型只能提供该 Tool Schema 允许的领域参数；
- Tool 本身不手写 Component Patch；
- Runtime/World 再执行权限、存在性和业务规则校验；
- 一次 ToolExecutor 调用只返回一次 ToolResult；
- 是否继续模型调用由 Runtime Agent Loop 决定。

#### 9.3.1 Orchestration Tool

Orchestration Tool 只构造当前 Agent 编排的 Runtime 临时请求，不形成 WorldCommand，也不得直接
读取或修改 ECS。它使用显式 Schema，返回结构化接受/拒绝结果，并受 Actor Capability、Revision
与整轮预算约束。`request_npc_turn` 由 Runtime 注入 request identity 与 Revision，并按已接受
ToolCall 的顺序向内部 NarratorPlan 追加请求；只有 Narrator Turn 完成并释放执行槽后，Runtime
才能按 Plan 顺序启动对应 NpcAgent。

`create_npc` 同样是 Orchestration Tool，而不是在 Narrator Turn 内同步生成角色的 Command Tool。
它只校验并暂存模型提供的 source/lifetime/mode；Preset 编译、`npc_generation` Agent Turn 与最终
`SpawnCharacter` Command 都必须等当前 Narrator Turn 释放唯一执行槽后开始。第一个成功的
`create_npc` ToolResult 是 Turn barrier：AgentRunner 不再向 Provider 请求自然语言收尾，也不执行
同一 Assistant response 中排在其后的 ToolCall；Runtime 随即处理该唯一创建请求并基于提交后的
Observation 重新调用 Narrator。Scene、Place、GenerationPolicy 与 AgentProfile 由 Runtime 注入。
ToolResult 只能表示请求已进入本轮待处理队列，不能在角色提交前返回成功的 ActorId。被拒绝的
`create_npc` 不触发 barrier，仍允许模型在剩余预算内修正。

`npc_generation` 是单用途 Tool stage，只提供 `submit_npc_draft`。第一个成功的 Draft ToolResult
直接完成该 stage，Runtime 不再要求或调用模型生成一个随后会被丢弃的自然语言收尾；只有 Draft
被拒绝时 Agent Loop 才把 ToolResult 交还模型并允许在剩余预算内修正。一个 stage 最多暂存一个
Draft，成功后的同一响应内剩余 ToolCall 不再执行。

### 9.4 角色状态、物品与技能 Tool 面

第一阶段使用少量稳定的领域 Tool，不为每一种 Item/Skill 动态注册任意可变 Tool：

| 分类 | Tool 候选 | 责任 |
|---|---|---|
| Query | `inspect_character_status` | 返回 Actor 被授权的属性、资源、状态/症状摘要 |
| Query | `list_inventory` | 返回当前 Actor 可见的有界物品摘要 |
| Query | `inspect_item` | 按实例 ID 返回允许披露的定义与实例状态 |
| Query | `list_available_skills` | 返回当前可用 Skill Grant、消耗、目标和冷却摘要 |
| Query | `inspect_skill` | 返回一个 Grant/Definition 的允许披露详情 |
| Command | `transfer_item` | 请求在合法容器/Actor 之间转移物品 |
| Command | `equip_item` | 请求把物品装备到合法槽位 |
| Command | `split_stack` | 请求拆分当前 Actor 拥有的可拆分物品堆叠 |
| Command | `use_item` | 请求执行 Item Definition 注册的领域行为 |
| Command | `use_skill` | 请求执行 Skill Grant 对应的 Active Skill |

`list_inventory` 与 `list_available_skills` 使用可选 `after` Stable ObjectId 与 `limit` 参数；默认
`limit = 32`、单次最大 `64`，按实例/Grant ObjectId 严格升序返回，并在仍有结果时返回
`next_after`。`inspect_item` 只接受当前 ToolContext Actor 拥有的 Item instance；
`inspect_skill` 只接受该 Actor 拥有的 Skill Grant。两类 inspect 返回对应实例和允许披露的固定
ModLock Definition，不能接受任意 ActorId 或遍历完整内容库。所有 Query 在绑定 Revision 上读取，
过期时返回结构化 unavailable，且拒绝时不改变 Revision。

ToolResult 使用 Stable ID，后续 Command 必须引用 Query 返回或 Observation 已授权的实例/Grant。
`transfer_item`、`equip_item`、`split_stack` 与 `use_skill` 的 Actor、ActionId 和 Revision 只来自
ToolContext；模型只能提供冻结 WorldCommand 中的领域参数。`use_item` 仍等待 Item 行为 Schema 与
对应 WorldCommand 增量冻结，本阶段不得用通用 Effect/Component patch 代替。

不得向普通 Narrator/NpcAgent 注册通用 `set_attribute`、`set_resource` 或 `set_condition`。
Agent 选择休息、使用物品、使用技能或其它领域行动，由相应 World System 计算资源和 Condition
变化。

### 9.5 Narrator NPC Tool 面

Narrator 可以使用：

| 分类 | Tool 候选 | 责任 |
|---|---|---|
| Orchestration | `create_npc` | 暂存 Preset/Generated source、Scene/Persistent lifetime 与 narrated/agent mode；成功即结束当前 Turn，并立即进入物化与重规划 |
| Command | `promote_npc` | 请求把已有 Scene-owned NPC 提升为世界级角色 |
| Orchestration | `request_npc_turn` | 请求 Runtime 为已有 AgentBinding NPC 追加一次 Turn |
| Query | `list_scene_transitions` | 返回当前 Scene 与当前 Revision 可用的 canonical Scene 切换目标 |
| Orchestration | `transition_scene` | 暂存目标 Scene，待当前 Narrator Turn 结束后原子停用当前 Scene 并激活目标 Scene |
| Orchestration | `create_scene` | 暂存显示名、framing 和 entry Place 文本，待 Turn 结束后创建 inactive Scene 与 entry Place |
| Orchestration | `create_place` | 暂存显示名与描述，待 Turn 结束后在 active Scene 创建并连接玩家当前 Place |

这些 Tool 不允许 Narrator 提供原始 Component、Provider、无限属性、未注册 Definition 或自定义
Executor。Runtime 只验证和执行 Narrator 通过原生 ToolCall 提交的结构化叙事决定，不从模型正文
解析控制协议，也不用关键词或硬编码剧情规则改写 Mention/Materialize/NpcTurn 选择。

同一 Narrator Turn 最多接受一个世界拓扑请求（`transition_scene`、`create_scene` 或
`create_place`），且它不能与待处理的 `create_npc` 或 `request_npc_turn` 共存；后出现的冲突
ToolCall 返回结构化拒绝。Narrator Turn 结束后 Runtime 才以 ToolCall 被接受时的 Revision 执行；
成功、stale 或领域拒绝都形成下一次 Narrator 输入并强制重新规划，旧投影中的 NPC 请求不得继续。

Narrator 在切换前必须调用 `list_scene_transitions`，并把其中某一项的 `target` 对象原样传给
`transition_scene`，不得从展示名称、玩家文本或模型记忆猜测 Stable ID。Query 与切换共享
`narrator.transition_scene` Capability，并绑定当前玩家 Actor 与 Revision；它返回当前 Scene 摘要，
以及以下互斥的目标：

- 已在 World 中物化且 inactive 的 Scene 返回 `existing` Scene ObjectId；
- 当前 Registry 中尚未物化的 Scene Definition 返回 `definition` Definition ID；
- 当前 active Scene 被排除；已物化 Definition 不再同时返回 Definition 目标。

目标不在该列表、参数形状错误或编排互斥时，ToolResult 必须给出可区分的结构化原因与恢复方向。
同一 Turn 对同一已接受目标的重复调用按幂等接受处理；不同目标仍按冲突拒绝。Narrator 不得在
`scene_transition_results` 报告 committed 前叙述已经抵达；若列表没有匹配玩家意图的目标，应说明
当前内容不可达，不能原样重试或隐式生成 Scene。

### 9.6 Event 与 Gameplay Action Tool 面

第一阶段只注册引擎拥有的通用 Tool，不按每个 Mod Definition 动态创建 Tool Handler：

| 分类 | Tool 候选 | 责任 |
|---|---|---|
| Query | `list_active_events` | 返回 Actor 可见的 Event Instance、当前节点和有界 Option 摘要 |
| Query | `list_gameplay_actions` | 返回当前 Actor 可执行的已注册 Gameplay Action 摘要 |
| Command | `choose_event_option` | 对 Event Instance 提交 Option ID，并在 current Revision 重新校验 |
| Command | `perform_gameplay_action` | 按 Action Definition 的参数 Schema 请求执行已编译 Effect plan |

Mod 只能提供 Definition ID、展示内容、参数 Schema 和受限规则计划；它不能注册 Tool 名称、覆盖
现有 Tool Schema、取得 ToolContext 或绑定 Service Tool。Runtime 根据 Actor Capability、Mod
lock 和当前 Observation 决定是否投影相应 Event/Action。

### 9.7 Tool 顺序与预算

- ToolCall ID 和模型返回顺序必须保持；
- AgentRunner 只能执行当前 CompletionRequest 实际投影的 ToolDefinition；即使名称属于 Runtime
  全局已注册 Tool，未向该 Turn 提供的 ToolCall 也必须返回关联的 `tool_not_offered` error result，
  不得进入 ToolExecutor；因此无 Tool 的 `npc_generation` stage 不能读取或修改世界；
- Query Tool 第一阶段也按顺序执行，后续有证据后才能并发；
- Command Tool 严格串行；
- 每个 Narrator/NPC Turn 有 ToolCall、Model Call、Token、耗时和输出预算，完整 PlayerInput 编排
  另有总预算与最大轮数；
- ToolResult 必须关联原 ToolCall ID，并在同一 assistant message 之后按原 ToolCall 顺序逐条写入
  canonical history；未知、无权限、参数错误与执行失败也必须生成关联原 ID 的 error result；
- 成功 Command ToolResult 必须投影 `CommittedAction.event_ids` 的完整有序列表；兼容性的单个
  `event_id` 不能导致 AgentRunner 丢弃同一 durable unit 的其它 Event，`NpcTurnResult.world_events`
  必须包含该 Turn 所有成功 Tool 的完整已提交 Event；
- ActionId 重复提交遵循第 11.4 节：相同 digest 返回已保存 outcome，不产生第二次世界修改；不同
  digest 返回 identity conflict；
- 一个模型响应中的多个 Command ToolCall **不构成跨调用原子事务**。每个成功 Command 是独立
  durable unit；后续 Tool 失败或取消不回滚先前已提交结果。需要原子修改多个领域对象时，必须由
  一个领域 Command Tool/World System 在单一 durable unit 内表达，不能依赖 Agent Loop 补偿。

## 10. Agent Runtime

### 10.1 Agent 角色与对象生命周期

第一阶段必须区分：

| 对象 | 生命周期 | 责任 |
|---|---|---|
| `NarratorAgent` | Runtime 长期逻辑角色；调用实例按 Turn 创建 | 解释玩家输入、调用编排/世界 Tool、生成自然语言叙事 |
| `NarratorPlan` | Runtime 从一次 Narrator Turn 的已接受 ToolCall 构造 | 保存有序 NpcTurnRequest，不是模型正文 wire |
| `NpcAgent` | 单次 NPC Turn | 消费不可变角色/场景上下文并产生 ToolCall 与有界结果 |
| `AgentBinding` | ECS 持久状态 | 把 Character 绑定到 Agent Profile 与自治策略 |
| `NpcTurnRequest` | Runtime 临时队列记录 | 指定 NPC、Scene、Assignment 与计划所基于的 Revision |
| `NpcTurnResult` | 单次 NPC Turn 临时结果 | 关联自然语言响应与实际 ToolResult/WorldEvent |
| `AgentRunner` | 可共享运行服务 | 执行单次 Bridge、Tool Loop、取消、预算和结果关联 |

第一阶段产品 API 冻结为拥有所有权的下列语义形状。`NarratorPlan`、`NpcTurnRequest` 与
`NpcTurnResult` 是 Runtime/Agent 之间的内部类型，可以序列化后作为模型输入，但绝不能要求模型
在正文中生成它们：

```rust
pub struct NpcAgent {
    pub definition: AgentDefinition,
    pub context: NpcContext,
}

pub struct AgentDefinition {
    pub profile_id: ContentDefinitionId,
    pub system_style: LongText,
    pub model_alias: ShortText,
    pub allowed_tools: BTreeSet<String>,
}

pub struct NpcContext {
    pub actor_id: ActorId,
    pub revision: Revision,
    pub character: CharacterContext,
    pub scene: SceneContext,
    pub assignment: NpcAssignment,
    pub recent_dialogue: Vec<TranscriptItemRecord>,
    pub truncated: bool,
}

pub struct NarratorPlan {
    pub based_on_revision: Revision,
    pub npc_turns: Vec<NpcTurnRequest>,
}

pub struct NpcTurnRequest {
    pub request_id: NpcTurnRequestId,
    pub actor_id: ActorId,
    pub scene_id: ObjectId,
    pub based_on_revision: Revision,
    pub assignment: BoundedText<4096>,
}

pub struct NpcTurnResult {
    pub request_id: NpcTurnRequestId,
    pub actor_id: ActorId,
    pub observed_revision: Option<Revision>,
    pub final_revision: Revision,
    pub status: NpcTurnStatus,
    pub response: Option<LongText>,
    pub failure: Option<ModelFailureDiagnostic>,
    pub tool_call_ids: Vec<String>,
    pub world_events: Vec<EventId>,
}
```

`CharacterContext` 固定包含 Actor/Revision、展示身份/Profile、Location、Base/Effective Attribute
摘要、Resource、可感知 Condition、Inventory、可用 Skill、KnownFact 与 Goal 的有界拥有所有权
投影；`SceneContext` 固定包含 Scene/Revision、展示 framing、World Clock、当前 Place、可见 Actor 与
已提交 Event 摘要；`NpcAssignment` 只把 request assignment 与投影时 Revision 绑定。集合元素使用
Core 的 Stable ID/Fixed/WorldTime，不引入 Bevy Entity 或 Armillae Provider 类型。

展示投影不能要求 Agent/TUI 再访问 Content Registry。第一阶段 Core View wire 因此固定包含下列
补充形状；所有结构拒绝未知字段，集合按 Stable ID 排序，`effective` 是 current Revision 的派生值：

```rust
pub struct AttributeView {
    pub attribute_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub base: Fixed,
    pub effective: Fixed,
}

pub struct ResourceView {
    pub resource_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub current: Fixed,
    pub maximum: Fixed,
}

pub struct ConditionView {
    pub condition: ConditionRecord,
    pub display_name: Option<DisplayName>,
    pub symptoms: Vec<ShortText>,
}

pub struct InventoryView {
    pub item: ItemRecord,
    pub display_name: DisplayName,
}

pub struct SkillView {
    pub grant: SkillGrantRecord,
    pub display_name: DisplayName,
    pub available: bool,
}
```

`CharacterContext.attributes` 使用 `Vec<AttributeView>` 同时承载 Base/Effective，不再暴露只有 Base
的 map；`known_facts` 使用 owner 已过滤的 `Vec<KnownFactRecord>`。Condition symptom 只包含
`minimum_intensity <= instance.intensity` 的当前可感知文本。Resource maximum 是 effective maximum。
这些都是可丢弃投影，不能写回存档。

Condition 诊断固定使用普通 KnownFact，而不向 ConditionRecord 增加观察者相关字段。观察者 Actor 的
Fact 满足以下全部条件时，才投影对应 Condition 的真实 `display_name`：subject 为 Condition target
Actor Object、predicate 为引擎内置 `games.loreloom.core:tag/diagnosed_condition` Tag Definition、value
为该 Condition Definition ID 的 `FactValue::Tag`、status 为 `confirmed`。否则 `display_name` 为
`None`，只投影当前 intensity 允许的 symptoms；TUI 使用本地通用占位文本，Agent 不接收真实名称。
Believed/disputed/forgotten、其它 owner、其它 target 或其它 Condition ID 均不构成诊断。

`NpcTurnStatus` 值固定为 `completed | stale | rejected | cancelled | budget_exhausted | failed`；
`observed_revision` 对未开始的结果为 `None`，开始投影成功后为 `Some`。同一内部 Plan 中的
request ID 必须唯一。

规范要求：

- 自然语言 PlayerInput 只交给 NarratorAgent。模型正文只作为自然语言使用；Runtime 不得对任何
  Narrator、NpcAgent 或 generation stage 的正文调用 JSON/Schema 反序列化来取得控制数据；
- Narrator 只能通过该 Model Call 的 Provider 原生 ToolCall 请求结构化编排或世界修改，不能在
  Tool Handler 内同步递归调用 NpcAgent；
- `request_npc_turn` 的模型参数只包含 committed ActorId 与 assignment；Scene、request ID、
  ToolContext Revision 与队列位置由
  Runtime 注入。已接受 ToolCall 的顺序就是 `NarratorPlan.npc_turns` 的语义执行顺序，Request 不携带
  `priority`，Runtime 不重新计算叙事优先级或公平性；
- 内部 NarratorPlan 只能引用 ToolCall 开始前已经提交且在该 Observation 中标记为
  `npc_turn_available` 的 ActorId；尚未物化的 Preset/Generated source 不进入 `NpcTurnRequest`；
- Runtime 必须从 World 注入 Scene，验证 Actor、Scene membership、Revision 和 Capability，并在独立预算配置
  下确认仍有资源后才排队；模型或 Mod 不能通过 Request 扩大预算；
- `based_on_revision` 记录 Narrator 制定计划时的 provenance，不是后续世界写入可沿用的
  `expected_revision`；每个请求开始时必须针对当前 committed Revision 重新校验 Actor、Scene 和
  Assignment，条件已失效时返回关联该 request ID 的 stale/rejected NpcTurnResult；
- Runtime 同时只能运行一个 Narrator/NPC Turn；队列中的请求不持有 ECS 引用，出队开始时
  必须按届时 committed Revision 重新校验并重新投影上下文；
- CharacterContext 来自 ECS Character/Inventory/Skill/Knowledge 投影；
- SceneContext 来自已提交 `SceneState`、`DirectorState` 和 Actor 可见 WorldEvent；Narrator
  隐藏模型历史不是场景事实；
- NpcAssignment 表达本次任务或关注点，不自动成为 World Fact；
- Runtime 必须从同一个 Revision 生成 CharacterContext、SceneContext 和 Assignment binding；
- `NpcAgent::new(agent_definition, character_context, scene_context, assignment)` 只创建一次性
  不可变执行对象；Agent Definition 由 AgentBinding 指向的版本化 Profile 解析；
- NpcAgent 不持有 ECS Query/引用、`&mut World`、Provider Client、Store 或可持久化服务；
- AgentRunner 持有 Bridge 与 Tool 执行能力；所有世界写入仍通过带 Actor/Revision 的
  ToolContext 和 WorldGateway；
- NpcTurnResult 的 `response` 是 NPC 提供给 Narrator 的自然语言 claim，不是 World Fact；只有成功 ToolCall /
  WorldCommand 对应的 ToolResult/WorldEvent 能证明世界动作已经发生；
- 后续 Narrator Turn 的输入必须把 NPC response 明确标为 claim，并把 committed WorldEvent 独立
  投影；NPC 不得反向直接调度 Narrator；
- 每个已接受的 NpcTurnRequest 都必须产生同 request ID 的 NpcTurnResult；未开始即被取消、预算
  截止或重新校验失败的请求也不能从结果序列中静默消失；
- 当前 Plan 的请求全部完成、取消或失败后，Runtime 才把有序 NpcTurnResult、物化结果和届时已提交
  WorldEvent 投影给下一次 Narrator Turn。Narrator 可以继续调用 Tool；当本 Turn 没有待处理的
  materialization 或 NpcTurnRequest 时，其最终自然语言正文直接成为玩家可见叙事；
- Transcript 的 supporting Event ID 由 Runtime 从本次编排成功 ToolCall 的 committed event 集合
  确定性收集，模型不生成、选择或复述 Event ID；
- 不设置每轮固定 NPC 数量常量；实际数量由 Narrator 决定并受请求合法性、总预算与最大编排轮数
  约束；
- NpcAgent 完成后即丢弃；角色长期状态只保存在 ECS、Transcript、WorldEvent 和 AgentBinding。

模型自然语言正文的语义真实性不能由字符串验证器完全证明。Runtime 通过 ToolResult/WorldEvent
维护实际状态 provenance，并在 Prompt 中要求 Narrator 只把已提交变化叙述为事实；任何 narration
或 NPC claim 本身仍不成为 ECS 或存档世界事实。

### 10.2 根世界/Mod Content、SpawnSpec 与运行时生成

预设 NPC/Scene 必须来自版本化根世界或已启用 Mod 的 Content Pack。物理文件布局、编码和 Manifest
语法由 Mod/Rule Spike 冻结；本节已经冻结以下逻辑内容：

- Mod/Pack ID、版本、Loreloom Schema/Engine 兼容范围、依赖和内容哈希；
- Character/Scene Definition；
- Agent Profile、Item、Skill、Attribute、Resource、Condition、Parameter 等静态 Definition；
- 可选 Event/Rule/Gameplay Action Definition；
- 初始位置、关系、物品、Skill Grant、Knowledge、Goal 和 Scene Narrative 数据。

每个 `content/*.json` 固定使用 Content Document v1：顶层只有 `schema_version: 1` 与
`definitions`；definition 是拒绝未知字段的 `type` tagged union，并具有完整 namespaced ID 与有界
display/description。v1 字段按 kind 固定为：

- AgentProfile：system style、model alias、Tool capability allowlist 与默认 autonomy；
- Attribute：minimum、maximum 与允许的 aggregation/modifier operation；
- Resource：minimum/effective maximum、maximum policy 与可选关联 Attribute；
- Condition：tags、stack/intensity/duration、症状投影、Modifier、可选 periodic allowlisted Effect；
- Item：tags、stack limit、Fixed gram unit weight、可选 durability/container/equipment slot、Modifier；
- Skill：active/passive/reaction kind、唯一 Resource cost、cooldown ticks、typed target、registered
  executor/effect plan 与 reaction window；
- Character：Profile、Archetype、可选 AgentProfile、placement、Base Attribute/Resource、Condition、
  inventory、Skill、Knowledge、Goal 与 trusted spawn constraints；
- Place：名称、描述、tags 与 Scene graph edge；
- Scene：入口 Place、初始 Character/Item、关系、活动 Event 与 narrative framing；
- Parameter/Event/GameplayAction/Rule：服从第 6.5 节已经冻结的 tagged Schema。

所有文本分别受字段上限约束，Content compiler 规范化 map/set 顺序并拒绝重复 key。Content schema
升级只能通过完整 document 的连续纯 migration 进行；Pack 中任一 document、跨引用或 migration
失败时不发布 candidate Registry。Core 拥有运行时领域值、持久 record 和
`CharacterSpawnSpec`；Content 拥有 Definition/NpcDraft wire、Registry 与两者到 SpawnSpec 的统一
纯编译器。这一归属不允许 Core 依赖 Content，也不允许 World 接收原始 NpcDraft。

Item v1 的质量单位固定为 Fixed 克，Container capacity 同样使用克和最大直接 child 数。两个 stack
只在 Definition/content lock、durability、custom name、bound actor、instance Parameter/Modifier 与
Origin 全部相等时等价；ObjectId、quantity、location 和 owner 不参与。Container、equipped 或已有
child 的 item 不可 stack。拆分复制全部等价字段/Origin、分配新 ObjectId，并在同一 Command 调整
数量、位置和 provenance。

导入管线按以下边界执行：

1. Content 层解析完整 Pack，并验证 Schema、版本、依赖、Definition ID 和所有跨引用；
2. Content 层先收集 Pack 内所有 Definition ID/kind，再解析前向引用和引用种类；重复 ID、缺失
   引用、错误 kind 或非法 local key 使完整 Pack 失败；
3. Content 层在不访问 Working World 的情况下生成 candidate immutable Definition Registry，并纯编译
   CharacterSpawnSpec/Scene spawn plan；
4. 只有完整 candidate 通过才原子发布 Registry；规范迭代顺序按 Definition ID，不依赖文件或
   JSON array 顺序；
5. Runtime 建立单一初始化提交边界，把验证结果交给 World；
6. NpcFactory/World 在 candidate world 中先为全部实例建立 Stable ObjectId/Entity，再解析 local
   key、existing ObjectId 等跨对象引用并校验世界不变量；
7. 任一步失败都丢弃暂存结果，不能发布部分 Pack、Scene、Entity、关系、Event 或已经推进的 ID
   allocator。

Content Definition ID 不是运行 ObjectId。预设实例必须记录：

```rust
pub struct ContentOrigin {
    pub mod_id: ModId,
    pub mod_version: ModVersion,
    pub pack_id: ContentPackId,
    pub definition_id: ContentDefinitionId,
    pub content_version: ContentVersion,
    pub content_hash: ContentHash,
}
```

运行时 NPC 从受限 Generation Request 产生 Draft：

```rust
pub struct NpcGenerationRequest {
    pub scene_id: ObjectId,
    pub role: ShortText,
    pub purpose: LongText,
    pub desired_traits: BTreeSet<ContentDefinitionId>,
    pub importance: NarrativeImportance,
}

pub enum NarrativeImportance {
    Ambient,
    Supporting,
    Principal,
}

pub struct GeneratedOrigin {
    pub generation_id: GenerationId,
    pub generator_version: ShortText,
    pub source: GenerationSource,
}

pub enum GenerationSource {
    PlayerInput { transcript_id: TranscriptItemId },
    WorldEvent { event_id: EventId },
}
```

Character、Scene 与 Place 的来源统一使用 `EntityOrigin = Content | Generated | System` tagged
provenance；当前产品只从 Content 或 Narrator 生成 Scene/Place。预设 Scene/Place 包装完整
ContentOrigin，运行时创建保存完整 GeneratedOrigin，不用虚构 Definition ID，也不从 Prompt 或模型
正文重建。该字段直接属于未发布的 payload v1，不保留旧开发期仅允许 ContentOrigin 的解码分支。

NpcDraft 不是 World Fact，不能直接反序列化为 Component。预设 Character Definition 与运行时
NpcDraft 必须汇入同一个候选协议：

```rust
pub struct CharacterSpawnSpec {
    pub origin: CharacterOrigin,
    pub profile: CharacterProfileInput,
    pub agent_profile: Option<AgentProfileId>,
    pub placement: PlacementInput,
    pub attributes: AttributeInput,
    pub resources: ResourceInput,
    pub conditions: Vec<ConditionGrantInput>,
    pub inventory: Vec<ItemGrantInput>,
    pub skills: Vec<SkillGrantInput>,
    pub goals: Vec<GoalInput>,
    pub trusted_constraints: SpawnConstraintInput,
}
```

Runtime 必须先验证来源 Capability、数量和模型预算；NpcFactory 再验证所有 Definition、属性
预算、Resource 范围、Condition、物品/技能、Scene 和当前世界引用，形成 `SpawnNpcCommand`。
两种来源共享 Factory、World System、Event、持久化和 Agent Turn 路径。

`trusted_constraints` 只能由 Content compiler 从 Character Archetype 或 Runtime 从已授权
GenerationPolicy 注入；NpcDraft、模型输出和 Mod 文本不能声明或扩大 attribute points、允许的
Definition、数量、lifetime 或 capability。NpcDraft 只携带请求的领域值，编译器拒绝额外控制字段。

`submit_npc_draft` wire 只包含模型拥有的 `display_name`、`profile`、`base_attributes`、
`resources`、`conditions`、`inventory`、`skills`、`knowledge` 与 `goals`。它不得包含
AgentProfile、GenerationPolicy、Scene、Place、controller、lifetime、Capability、Revision 或
provenance。只有 `display_name`、`profile.summary` 与 `profile.speaking_style` 必填；省略的
`profile.values`、`profile.narrative_tags` 及其它集合字段确定性地解释为空集合。Tool JSON Schema
必须完整描述 Knowledge/Goal tagged value 和枚举，且与 Runtime wire parser 使用同一字段集合。
Runtime 拒绝非法 Draft 时只返回稳定 `invalid_npc_draft`、顶层字段类别和
`retry_unchanged = false`，不得回显参数、模型正文或底层 serde 错误。

Scene spawn plan 每个 entry 具有只在该 plan 内有效的唯一 local key、一个 CharacterSpawnSpec 和
有界跨对象引用；local key 不进入长期身份。Factory 第一阶段为规范顺序的全部 entry 分配 ObjectId，
第二阶段才把 local key/existing ObjectId 解析为类型化关系。成功的 Content/Generated 角色使用同一
`character_spawned` WorldEvent 和领域实例形状，Origin 只保留来源/provenance 差异。

Scene Definition v1 的每个初始 Character entry 固定增加 `controller: player | narrator | rules |
agent` 与 `lifetime: scene | persistent`；`scene` 在物化时绑定新分配的运行 Scene ObjectId，内容不能
直接填写运行 ID。作为新 World 的 bootstrap Scene 必须恰有一个 `player` entry；`agent` entry 的
Character Definition 必须引用 AgentProfile。entry local key、Place/Character Definition ID 都必须
唯一或有效，并按 local key byte-order 物化。

Content 的 `compile_scene` 纯生成拥有所有权的 `SceneSpawnPlan`，包含带 ContentOrigin 的 Scene/Place
投影、Place Definition edge 和有序 Character entry，不分配 Stable ID 或访问 World。编译器要求
每条 edge 的两端都属于同一 Scene 且双向声明；World 物化时先为全部 Place 分配 Stable ID，再把
Definition edge 解析为持久 Place ObjectId。World 的共享 Character Factory 同时被
正常 `SpawnCharacter` Command 与 bootstrap 使用：Runtime 预先为 Scene、全部 Place、Character 和
owned child 分配 Stable ID，Factory 用同一 CharacterSpawnSpec、Definition 校验、物品/Condition/
Skill/Knowledge/Goal 物化逻辑生成 candidate records。全部 records 必须能在 Revision 0 重建并通过
World 不变量后，才由 Save create 的单个显式事务与精确 ModLock 一起发布；任一 entry 失败不创建
Save，也不消耗可观察的持久 ID。该初始化事务是第 10.4 节的初始化提交边界，不逐角色发布 Revision。

运行时生成 NPC/Scene/Place 在成功提交后保存完整领域状态与 GeneratedOrigin；Load 不调用模型，也
不依赖原 Prompt 重建实例。ContentOrigin 保留物化时的 provenance，但候选 WorldLock/ModLock 不因
hash 不同而被要求与其逐字相等；Runtime 必须用候选 Registry 重建并验证所有仍参与模拟的
Definition 引用，成功后原子采用候选 Lock，失败时保持存档及原 Lock 不变。GeneratedOrigin 不替代
当前领域 Definition lock，因为生成实例仍可能引用 Attribute、Item、Skill 等内容定义。

### 10.3 NPC 创建、可调度投影与物化生命周期

NarratorAgent 根据自然语言、SceneContext 和玩家意图拥有叙事决定，但模型侧只使用两个正交 Tool。
`create_npc` 的 wire 固定为：

```rust
pub struct CreateNpcRequest {
    pub source: NpcCreationSource,
    pub lifetime: NpcLifetime,
    pub mode: NpcCreationMode,
}

pub enum NpcCreationSource {
    Preset { character_id: ContentDefinitionId },
    Generated { role: ShortText, purpose: LongText },
}

pub enum NpcLifetime {
    Scene,
    Persistent,
}

pub enum NpcCreationMode {
    Narrated,
    Agent,
}
```

纯叙事提及不调用 Tool，也不创建 Entity。`Narrated` 创建没有 AgentBinding、由 Narrator 叙述的完整
Character；`Agent` 创建带 enabled AgentBinding 的完整 Character。两种 mode 使用相同领域 record，
不存在丢字段的轻量持久格式。模型不得在 `create_npc` 中提交 SceneId、PlaceId、Revision、
GenerationPolicyId、AgentProfileId、controller 或 assignment。

根 `world.toml` 必须以 `npc_generation_policy` 指向当前 WorldLock 中一个
`GenerationPolicy Definition`。该 Definition 拥有 SpawnConstraints 和生成 NPC 使用的唯一
AgentProfile；Preset Agent mode 从 Character Definition 自身解析 AgentProfile。世界编译在进入
Runtime 前验证这些引用。Host 只配置 Provider 与资源上限，不在本地产品配置中重复声明世界的生成
策略。Mod 可以通过既有显式 Patch 规则修改世界策略 Definition，但不能在运行时替换锁定结果。

Generated source 由 Runtime 注入当前 Scene/Place 与根世界 GenerationPolicy，再通过现有 Narrator
Provider 的独立 `npc_generation` stage 调用专用 `submit_npc_draft` Tool；NpcDraft 来自 Provider
原生 ToolCall 参数，Runtime 不解析该 stage 的自然语言正文。NpcDraft 不选择 AgentProfile；绑定由
锁定策略决定，GenerationPolicyId 与 AgentProfile 也不进入该 stage 的模型 payload。第一个成功
Draft ToolCall 即为该 stage 的终态，不追加自然语言收尾 Model Call；只有被拒绝的 Draft 才继续
Tool continuation。该 stage 计为一个 started Agent Turn，并累计到同一 PlayerInput 的
Model/Token/byte/time 预算，不新增 generator Provider 配置。

Preset/Generated 使用统一、质量优先的两阶段编排：

1. Narrator 在当前 Turn 中调用 `create_npc`；Tool 只把合法请求加入 Runtime 临时队列，并以成功
   ToolResult 直接结束当前 Turn；
2. AgentRunner 不请求无用的自然语言收尾，也不执行同一响应内后续 ToolCall；Runtime 释放唯一
   Agent 执行槽后立即处理创建并再次调用 Narrator；
3. Runtime 按 ToolCall 顺序处理队列，注入当前 Scene/Place。Preset 使用 `compile_character`；
   Generated 启动独立 `npc_generation` Turn，再用锁定 GenerationPolicy 调用 `compile_draft`；
4. 两种来源都只通过 `CharacterSpawnSpec -> SpawnCharacter` 提交，并从 committed
   `CharacterSpawned` Event 取得 ActorId；失败不得发布 ActorId、注册 Agent 或遗留部分领域记录；
5. Runtime 把每项成功 ActorId 或结构化拒绝原因、更新后的 Scene Observation 作为下一次 Narrator
   输入，在同一次 PlayerInput 内重新调用 Narrator；该次重规划计入最大编排轮数和整轮资源预算；
6. 只有后续 Narrator Turn 才能用 committed ActorId 调用 `request_npc_turn`。若新角色具有 enabled
   AgentBinding，Runtime 从当前 Registry 解析 AgentProfile 并绑定 Host 已配置的 NPC Bridge。

`create_npc` 不携带 assignment，保证 Narrator 在决定具体任务前先看到生成后的姓名、Profile、属性、
关系和状态。不设置 ReviewRequired、重要度等级或可选复核状态。Load 已保存 Generated NPC 时不得
重新生成。同一 PlayerInput 内结构完全相同的 CreateNpcRequest 具有编排幂等语义；首次成功物化后，
后续相同 ToolCall 返回既有 ActorId。需要多个同类角色时，Narrator 必须给出可区分的 role/purpose。

`request_npc_turn` 的模型参数固定为 committed `actor_id` 与自然语言 `assignment`。SceneId、Revision、
request identity 与队列位置由 Runtime 注入。Scene Observation 的每个 visible actor 显式包含
`npc_turn_available`；它只在角色与玩家位于同一可见 Place、controller 为 Agent、AgentBinding
enabled 且 Profile 可解析时为 true。同一 Revision 中使用该 ActorId 的请求必须接受；真正执行前若
世界已推进，则返回 stale NpcTurnResult 并重新规划，不能把隐藏前置条件留到 Tool Handler 拒绝。

Runtime 仍验证 Schema、Capability、数量、Provider/Tool budget 和世界不变量，禁止 Narrator 提交
原始 Component、未注册 Definition、Provider Client 或自定义 Executor。角色产生物品、关系、
Knowledge、Goal、Quest 或持久 WorldEvent 前必须完成实体化。Scene-lifetime 实体与所属 Scene 一起
持久保存；Scene 停用或故事阶段结束都不删除它们。角色需要跨 Scene 移动或成为世界级自治角色时
必须先升为 Persistent。

Runtime 拒绝 Tool 时返回脱敏、稳定且可恢复的结构化 code；AgentRunner、UiSnapshot 与 TUI 必须保留
该 code，但不得保留完整 Tool 参数或 ToolResult。Runtime 数量门禁只使用 Host 的可配置资源 policy
（每次编排生成数、每 Scene materialized 数和全存档 persistent generated 数），默认分别为 32、256
与 1,024。promotion 是独立 WorldCommand，只把 Character lifetime 从 Scene 改为 Persistent。

#### 10.3.1 Scene 激活与重复进入

第一阶段同一 World 只有一个 active Scene。`transition_scene` 接受已提交 Scene ObjectId，或当前
ModLock 中的 Scene Definition ID：

在提交前，Narrator 通过 `list_scene_transitions` 取得当前 Revision 的可选目标。该投影以 Working
World 与当前 Registry 为权威：inactive Scene 使用既有 ObjectId，未物化 Definition 使用 Definition
ID，当前 Scene 和重复 Definition 目标不进入结果。这个发现步骤只读取状态，不修改 Revision，也不
授予运行时创建未注册 Scene 的能力。

- 已提交目标必须是 inactive Scene；Runtime 使用其原有 SceneRecord、Place、Character、Event 和
 其它领域状态，不重新从 Definition 覆盖；
- Definition 目标若已在本 World 物化，Runtime 重新激活既有实例；尚未物化时才通过共享
  `compile_scene -> SceneSpawnPlan -> NpcFactory` 路径创建一次；
- Scene Definition 中唯一的 Player entry 只用于新 World bootstrap；首次把该 Definition 物化到已有
  World 时忽略此 entry，不创建第二个玩家，现有 `WorldState.player_actor` 移动到目标
  `SceneRecord.entry_place`；其余 Character entry 按既有 Factory 路径创建一次；
- 当前 Scene 标记为 inactive，目标 Scene 标记为 active，WorldState.active_scene、玩家 Location 和
  Scene left/entered Event 在一个 candidate/durable unit 内提交；
- 目标创建或校验任一步失败时，当前 Scene、玩家 Location 与 Revision 都保持不变；
- 离开 Scene 不删除任何 Scene-owned record。再次进入必须恢复离开时的权威状态，Load/Replay 不
  调用 Provider；
- inactive Scene 第一阶段仍可保留在 Working World。按需卸载只能作为后续内存优化，不能改变存档
  事实或 Agent 可见性规则。

Scene Definition 第一阶段在单个 World 中采用单实例语义；需要同一模板的多个独立实例属于后续
Schema 扩展，不能靠重复物化产生无法区分的副本。World 不变量要求恰好一个 Scene 为 active，且
必须与 `WorldState.active_scene` 一致。

#### 10.3.2 Scene/Place 运行时创建与连接

`SceneRecord` 是持久叙事/生命周期容器，拥有 display name、framing、entry Place、active 状态与
EntityOrigin；`PlaceRecord` 是该 Scene 内的位置节点，拥有 display name、description、tags、
EntityOrigin 和 `BTreeSet<ObjectId>` 双向 edges。每个 edge 必须存在、指向 Place、属于同一 Scene、
不能自连接，并在对端包含反向 edge。Character.location 必须指向 Place；普通 `Move` 还必须要求
目标位于当前 Place 的 edges 中。Scene transition 直接把玩家放到目标 entry Place，不受普通 edge
约束。SceneContext 必须投影当前 Place 的 canonical adjacent Place ID、名称和描述，使 Agent 只需从
当前 Observation 选择合法 `move_character` 目标，不猜测运行 ID 或遍历整个 Scene。

Narrator Tool wire 固定为：

```text
create_scene {
  display_name,
  framing,
  entry_place_name,
  entry_place_description
}

create_place {
  display_name,
  description
}
```

模型不能提供 SceneId、PlaceId、edge、origin、Revision 或激活状态。Runtime 从可信 ToolContext 与
本次 PlayerInput provenance 构造延迟 Command：

- `CreateScene` 在一个 candidate 中分配 Scene 和 entry Place ObjectId，保存同一个
  GeneratedOrigin，创建 inactive Scene；成功产生 `SceneCreated` Event，不修改 active Scene 或玩家
  location；
- `CreatePlace` 读取执行 Revision 的 active Scene 与玩家当前 Place，分配 Place ObjectId，把新 Place
  与当前 Place 双向连接并在一个 candidate 中保存两条完整 PlaceRecord；成功产生 `PlaceCreated`
  Event，不移动玩家；
- 任一校验或 durable commit 失败都不留下 Scene、Place 或半条 edge；成功、stale、拒绝均进入下一次
  Narrator 输入并强制基于当前 Revision/Observation 重规划；新 Scene 如需进入，必须先从更新后的
  `list_scene_transitions` 取得其 existing ObjectId，再单独调用 `transition_scene`。

NpcAgent 不获得 `narrator.create_scene` 或 `narrator.create_place` Capability。Generated Scene 不进入
未物化 Scene Definition 列表，也不参与 Definition 单实例集合；它作为已物化 inactive Scene 进入
canonical transition targets。Definition 专属的 SceneLeft/SceneEntered Rule trigger 只为
ContentOrigin Scene 投影；Generated Scene 仍产生通用 WorldEvent trigger。

### 10.4 Mod 加载、冲突与扩展边界

主游戏根目录是唯一可直接游玩的 World 边界：

```text
world.toml
content/*.json
rules/*.json
prompts/*.md
locales/*.json
assets/**
mods/<mod-id>/...
```

`world.toml` Manifest Schema v1 声明 `world_id`、SemVer、Engine requirement、Content Schema、
初始 Scene、Inventory Root、Spawn System、显式 content/rule/resource 文件列表、`[prompts]` 中有序的
Narrator/NPC Prompt 路径。根世界 Narrator Prompt 列表非空，NPC 列表可为空。只读取 Manifest 显式
声明的世界文件；`mods/`、`.loreloom/`、Cargo
源码和其它根目录文件不进入世界 payload。Manifest 原始 bytes、声明 payload 和 Prompt 全部进入
WorldLock 内容哈希，但该 hash 只表达完整性与内容 provenance；Prompt-only 差异不得成为读档硬门禁。

回复语言是 World/Mod Prompt 拥有的叙事内容，不是 Manifest 或 Runtime 协议。Manifest v1 不声明
`response_language`；Agent 定义、Narrator/NPC Request Builder 与 Runtime 不保存或追加独立语言策略，
也不执行语言检测、翻译或模型重试。需要固定语言或跟随玩家语言时，World 必须在 Narrator/NPC Prompt
中分别用自然语言说明，Mod 可以按普通 Prompt 追加规则扩展该取向。

Mod Package 是主世界扩展的分发和完整性边界，逻辑上包含：

```text
Manifest
  ├── Content Pack
  ├── optional Rule Bundle
  ├── localized text
  └── bounded package resources
```

第一阶段 Mod 只支持显式启用的**目录包**，不支持 archive 自动解压。`mods/` 只表示已安装位置，
目录存在不等于启用；可重复 `--mod PATH` 形成候选闭包。目录布局固定为：

```text
mod.toml
content/*.json
rules/*.json
patches/*.json       # only files declared by Manifest
locales/*.json       # optional display-only data
assets/**            # optional bounded opaque resources
prompts/*.md         # optional agent prompt resources
```

产品启动时把世界根 `mods/` 的直接子目录作为 installed candidate，按稳定路径顺序使用与启用路径相同
的 Package resource limit、symlink、Manifest、Engine compatibility、payload group、content hash
与 Definition 文档解码/分组校验。只有通过上述独立 Package 校验的 candidate 才进入只读 installed
catalog；依赖图、跨包引用和 Patch 目标只在显式启用闭包中解析。无效、不可读或符号链接 candidate
只累计为 unavailable 数量，不能阻止未启用该包时启动。显式 `--mod` 仍是唯一启用入口；
位于 `mods/` 外但通过 `--mod` 启用的目录包仍进入 enabled 列表。同一 Mod ID 与版本同时出现在目录
和当前 `ModLock` 时只显示为 enabled；不同版本分别显示。catalog 是启动期 Host 投影，运行中目录
变化不热更新，重启后重新扫描。

`mod.toml` 是 UTF-8 TOML，Manifest Schema v1 至少声明 reverse-DNS lowercase Mod ID、SemVer
version、属于该 Mod 且 kind 为 `pack` 的 Pack ID、Engine SemVer requirement、Content Schema
version、required/optional dependencies、`content | rules` capability、64-byte lowercase hex payload
SHA-256、显式 Patch，以及可选的 `[prompts] narrator = [...]`、`npc = [...]` 有序路径列表。JSON
文件为 versioned tagged Schema，拒绝未知控制字段。第一阶段没有
package signature/authenticity 承诺；包来源信任由用户配置表达，SHA-256 只提供内容完整性和存档
provenance，不能单独判定内容与存档不兼容。第一阶段 Loreloom Engine compatibility version 固定为
`0.1.0`，独立于各 Rust crate 的
Semifold patch release；只有协议兼容边界变化时才显式提升。

`content/*.json` 使用第 10.2 节 Content Document v1，并只允许 AgentProfile 到 Scene 的静态
Definition；`rules/*.json` 复用同一 document envelope，但只允许 Parameter、Event、GameplayAction
与 Rule Definition。对应目录存在时 Manifest 必须分别声明 `content`/`rules` capability。
`patches/*.json` 不参与普通 Definition 扫描，只能由 Manifest 精确引用；Patch Document v1 顶层为
`schema_version: 1` 与非空 `operations`，第一阶段唯一 operation 是 `replace_definition`，其 value
为完整 Definition。替换值必须与声明的 target Definition 保持相同 ID 和 kind，保留 target 的
ContentOrigin，并在全部 Patch 应用后重新执行字段、跨引用、Rule plan 与能力校验。第一阶段不支持
JSON Pointer、字段级 merge、删除/重命名 Definition 或可执行 Patch。

payload hash 输入为把 `content_hash` 清空并把 dependency、capability、Patch 分别按 ID 排序后规范
TOML 序列化的 Manifest，以及按相对 path byte-order 排序的全部 payload。Digest 输入先写 manifest
byte length 的 little-endian `u64` 与 manifest bytes，再为每个 payload 写 path byte length 的
little-endian `u64`、path bytes、内容 byte length 的 little-endian `u64` 与原始内容 bytes。
Manifest TOML 空白不影响 hash，JSON/asset 原始 bytes 或路径变化会改变 hash。

声明为全局 Prompt 的路径必须唯一、位于 `prompts/*.md`、存在、为非空 UTF-8 且满足 `LongText`
上限。未在 `[prompts]` 声明的 Prompt 文件只作为普通包资源，不注入 Agent。最终 Narrator Prompt
顺序为根世界声明顺序，再按 Mod dependency topology 与各 Mod 声明顺序追加；NPC 使用相同合并顺序，
但位于角色 `AgentProfile.system_style` 之后。Prompt 不能改变 Runtime 实际注册的 Tool 或 Capability。

包路径统一使用 `/` 的相对路径。加载器在解析内容前拒绝 absolute、反斜杠、NUL、空/`.`/`..`
segment、symlink 和重复规范路径。Host 默认上限为 256 个 payload 文件、单文件 1 MiB、总 payload
16 MiB、路径深度 8、Manifest 256 KiB；内置与外部包相同，Host 可收紧，Manifest 不可扩大。
目录包根级 `.gitignore` 是唯一跳过的本地作者元数据，不计入目录项、payload、资源限额或内容哈希；
其它未知根文件、嵌套 `.gitignore` 与隐藏文件仍按普通 payload 执行严格路径和类型校验。

Runtime/Content 加载顺序必须是：

1. 从明确的游戏根目录读取并验证 `world.toml` 及其显式 payload；
2. 从显式启用来源读取 Mod，规范化路径并执行文件数量、单文件/总大小和深度上限；
3. 在不修改 Working World 前解析全部 Manifest，验证兼容范围、内容哈希和依赖闭包；
4. required dependency 缺失即失败；optional dependency 缺失可忽略，但若已安装仍必须满足 SemVer；
   生成确定性的依赖拓扑顺序，零入度 tie 按 Mod ID byte-order，循环和不兼容版本直接失败；
5. 验证所有 Definition/Rule/Patch，重复 Definition 默认失败；
6. 显式 Patch 只有 patching Mod 显式依赖目标 namespace，且目标 World/Mod、Definition 和版本约束匹配时
   才能按 `dependency topology -> patch ID` 顺序应用；普通 Definition 不能靠加载顺序覆盖；
7. 纯编译一个不可变 Definition Registry、Spawn plan 和 Rule plan；
8. Runtime 在一个初始化提交边界内安装 Registry 并物化 `world.toml` 指定的初始 Scene；
9. 成功后分别生成 WorldLock，以及只保存已启用扩展的 ModLock。

WorldLock 保存最后成功采用的根世界 ID、SemVer、content hash 与 Manifest/Content Schema；ModLock
每项保存
Mod ID、resolved SemVer、content hash、Manifest/Content Schema version、
`builtin | directory` source kind、已解析 dependency ID/version/optional flag 和按序 applied Patch ID；
不保存机器绝对路径。打开存档必须重新构建 candidate lock；不一致时进入内容兼容性协调，不能直接
把“内容已变化”判定为“存档不兼容”。

协调顺序固定为：完整编译候选包与不可变 Registry；在未发布 Working World 的边界内读取领域记录并
使用候选 Registry 重建；验证 Stable ID、Definition 引用、Rule/Parameter 状态、Scene/Place、领域
不变量和候选 Mod 依赖闭包。Prompt-only 修改、纯增量 Definition/Mod、未被领域记录或依赖闭包引用的
内容移除必须允许；已物化 Character/Scene 使用存档拥有的领域值，不从修改后的模板静默覆盖。重建
成功后才在显式事务中更新 SaveManifest 的 WorldLock/ModLock 并发布 Registry/World；重建或 Lock
提交失败时，原存档、原 Lock 和已发布世界都保持不变。缺失或不兼容时错误必须列出稳定的 Mod ID、
Definition ID 与失败分类，不得要求或自动执行删档。

根世界/Mod 文本、Agent Profile 和展示资源均视为不可信数据，不能改变 Engine Prompt 优先级、Tool
Capability 或日志/Secret 策略。Content/Rule Mod 没有包外文件、网络、Shell、Provider 或 Secret
访问能力。数据文件不能注册 Tool Handler、Native System、动态 Component 或脚本解释器。

Content 产品 API 以根世界路径、显式 Mod root 或测试 virtual directory 为输入；它们必须经过同一
path/size、hash、Definition、Patch 与 Registry 验证。编译结果拥有不可变 DefinitionRegistry、规范
WorldLock/ModLock 与有界资源索引。打开既有 Save 时 Runtime 必须在发布 World 前比较 candidate
WorldLock/ModLock 与 SaveManifest；不匹配进入上述协调。只有候选内容无法完整重建或通过校验时才
返回稳定拒绝；不能先替换 Registry、更新 Lock 或加载部分 World。

第一阶段不支持在活动 World 中热替换 Definition/Rule。启停或升级 Mod 必须关闭当前世界，并在
重开时通过版本检查、迁移和完整重建边界执行。

Extension Mod 不属于第一阶段。若后续采用 WASM Component，必须由独立 RFC 冻结 Host API、
Capability、Fuel/Memory/Time 配额、确定性、签名、升级和存档恢复；不得回退为不受约束的本机
动态库加载。

### 10.5 Agent Turn 与 Narrator 编排状态机

单个 Narrator/NPC Turn 的逻辑状态：

```text
Idle
  -> PreparingObservation
  -> CallingModel
  -> HandlingResponse
  -> ExecutingTool
  -> CallingModel | Completing
  -> Completed | Cancelled | Failed
```

一个自然语言 PlayerInput 的外层编排状态：

```text
PlayerInput
  -> PersistingInput
  -> NarratorThinking
  -> ResolvingOrchestration
  -> NpcThinking (zero or more, in accepted ToolCall order)
  -> UpdatingWorld (when Runtime commits orchestration or transcript state)
  -> NarratorThinking | Completed | Cancelled | Failed
```

面向 Runtime Client/TUI 的稳定粗粒度状态固定为：

```rust
pub enum RuntimePhase {
    Idle,
    PersistingInput,
    NarratorThinking,
    ResolvingOrchestration,
    NpcThinking,
    UpdatingWorld,
    Completed,
    Cancelled,
    Failed,
}
```

这些值不等于内部 Agent Step，也不携带 Actor、Prompt、模型正文、Tool 参数或错误详情。
`GameRuntime` 在状态实际开始前同步通知应用层提供的 observer；应用 adapter 把它转换为
`RuntimeUiEvent::PhaseChanged`。同一 observer 还接收当前玩家回合的完整安全 Tool Activity 列表：

```rust
pub enum RuntimeProgressEvent {
    PhaseChanged(RuntimePhase),
    ToolActivityChanged(Vec<ToolActivity>),
}
```

AgentRunner 在完整 ToolCall 已解析、通过预算门禁且准备交给 `ToolExecutor` 前先发布 Pending；执行
返回后用相同 call ID 与名称把该项原位更新为 Succeeded、Rejected 或 Failed，再继续下一次 Model
Call。列表只包含 call ID、Tool 名称、状态和经过约束的错误码，不包含 Tool 参数、ToolResult、模型
正文或隐藏推理。observer/channel 关闭不能改变 Tool 执行或 World 结果，终态仍由 committed 或失败
Snapshot 收敛。

规范行为：

- 每次状态转换可观察，但日志不包含默认敏感正文；
- Model Call 只能在没有 ECS 可变访问时等待；
- 模型返回纯文本时，Runtime 可以完成本次 Agent Turn；
- 模型返回 ToolCall 时，Runtime 逐个执行并把 ToolResult 加入 canonical history；
- 只有预算允许且策略要求继续时，才发起下一次单次 Model Call；
- 未知 Tool、无权限 Tool、无效参数和过期 Revision 都形成结构化 ToolResult；
- 取消正在进行的 Provider 请求后，不执行尚未开始的 Tool；
- 已提交 Tool 不因后续取消回滚；
- Agent 最终正文在完整 Model Call 成功后才交给 Runtime；Provider 未完成正文不进入 TUI。

第一阶段的单一 Agent Loop 执行槽已经冻结：NarratorAgent、NpcAgent 和不同 NpcAgent 之间不得
重叠运行 Model Call/Tool Loop；前一个 Turn 达到 Completed/Cancelled/Failed 并释放槽位后，下一个
才能开始。NPC 不得反向调度 Narrator。

Runtime 配置必须同时形成单个 Agent Turn 与完整 PlayerInput 编排两级资源限制。第一阶段字段与
默认值如下；这些是可配置默认值，不是 NPC 内容数量规则：

| Agent Turn 字段 | 默认值 |
|---|---:|
| `max_model_calls` | 16 |
| `max_tool_calls` | 64 |
| `max_input_tokens` | 524,288 |
| `max_output_tokens` | 32,768 |
| `max_total_tokens` | 557,056 |
| `max_model_output_bytes` | 1,048,576 |
| `max_elapsed_ms` | 600,000 |
| `require_reported_tokens` | false |

| PlayerInput 编排字段 | 默认值 |
|---|---:|
| `max_started_agent_turns` | 48 |
| `max_orchestration_rounds` | 8 |
| `max_model_calls` | 128 |
| `max_tool_calls` | 512 |
| `max_input_tokens` | 4,194,304 |
| `max_output_tokens` | 524,288 |
| `max_total_tokens` | 4,718,592 |
| `max_model_output_bytes` | 8,388,608 |
| `max_elapsed_ms` | 1,800,000 |
| `require_reported_tokens` | false |

`max_started_agent_turns` 统计每个 Narrator Turn、每个实际启动的 NPC Turn 与 generation Turn；
未启动即 stale、取消或预算拒绝的 Request 不消耗 started turn，但仍产生结果。NpcTurnRequest 数量
不另设固定常量，实际可执行量自然受 started turn、Model/Tool/Token/time 与轮次预算共同约束。
默认档按单 Turn 最多 16 次 Model continuation、每次最多 32,768 input context token 的累计上界设置
input budget；它是防失控上限而非目标消耗量，模型可随时以正文完成 Turn，玩家也可随时取消。

配置来源为 Runtime 全局、World/Save 与 Agent Profile。每轮开始时生成不可变 effective budget：
每个数值字段取所有适用层中最小值，`require_reported_tokens` 使用逻辑 OR；缺失层不扩大已存在上限。
模型与 Mod 只能请求更小限制。下一轮配置变更不得追溯扩大已开始 PlayerInput 的快照。

Provider usage 缺失必须计为 `unknown`，不能加成 0；`require_reported_tokens = false` 时继续依靠
Model/Tool Call、输出 byte 与 monotonic deadline 硬限制，设为 true 时缺失 usage 立即以
`budget_exhausted/missing_token_usage` 停止。已报告 usage 在每次 response 后累计，超限时不得执行
该 response 中尚未开始的 Tool；请求的 `max_output_tokens` 还必须收紧到剩余预算。

取消使用可唤醒的 Runtime cancellation token 与在途 Model future 竞速；取消获胜时丢弃 future 并
忽略迟到结果。每个 Model Call 前、每个 ToolCall 前和 NpcTurnRequest 出队时
再次检查取消。已进入 Store commit critical section 的 Command 按第 11.4 节解析，不能被普通取消
打断；已提交 Tool 不回滚。

应用装配取得的 cancellation token 必须跨 PlayerInput 保持同一共享身份；新 Turn 只在没有旧 Turn
运行时 reset 该状态，不能替换 token 导致 TUI 持有的 clone 失效。

Narrator 决定 NpcTurnRequest 的数量与语义顺序，Runtime 不设置固定 NPC 数量、叙事优先级或公平性
算法。每次 Narrator Turn 结束后，Runtime 从该 Turn 已接受的 ToolCall 取得下一轮工作，并重新经过
总预算与 current Revision 校验。

等待 Provider 时 Runtime 向 TUI 发布安全的阶段状态，例如 Narrator thinking、NPC responding 或
updating world；TUI 必须继续处理取消和退出，但不展示 Provider 未完成正文。第一阶段逻辑 World
Clock 暂停，不随墙钟时间隐式推进；世界只通过显式 WorldCommand/System 变化。

## 11. Persistence 规范候选

本节冻结后端、事务、重建与恢复边界；各领域 payload Schema 必须在对应领域协议冻结后才能进入
生产 Store codec。

第一阶段 Store 后端固定为通过 Toasty 使用嵌入式 SurrealDB + SurrealKV；SQLite 只保留为测试
对照，不进入默认产品装配。Store Spike 已验证显式顶层事务、原生 JSON、migration tracking、
文件引擎重开、冲突、崩溃恢复和 10,000 Record 规模；确定性关闭后的物理操作仍受下述上游门禁。
后端类型不得穿透 Core/World/Agent API。

第一阶段使用公开 Git 仓库 `https://github.com/noctisynth/toasty-driver-surreal` 的固定 revision
`0a7c87408e0daae0d6f5ed9f2b9d1ebf01d08549`。crates.io 的 `0.1.0-alpha.0` 发布包早于上述能力，
仍缺少显式事务、SurrealKV、原生 JSON 和 migration tracking，不能作为替代。该 driver 为
`AGPL-3.0-only`；Loreloom 的分发方式必须与该许可证兼容，或在发布前取得兼容的重新许可。
项目方于 2026-09-02 确认 Loreloom 全部 workspace package 采用 `AGPL-3.0-only`，并在仓库根级
维护许可证文本与 Cargo 发布元数据。

上述 revision 的 `drop(Db)` 会触发连接任务和 SurrealDB local router 的异步退出，但 Toasty/driver
没有“等待 SurrealKV flusher、router 与文件句柄全部关闭”的产品 API；上游已确认当前 SurrealDB
SDK 不暴露可等待的 local router/datastore shutdown。固定 sleep 只能作为 Spike 观测，不能满足
本节确定性关闭契约。事务、重建与领域 checkpoint 继续实现；物理备份、恢复、存档切换和产品
`close` 标记为 **UPSTREAM-GATED**，必须在上游提供能力后用“无 sleep/重试地立即复制或重开”验证。

Loreloom 必须显式打开事务；普通 Toasty batch 不具有本 Spec 所需的原子性承诺。数据库 migration
只管理物理表、索引和 migration ID；领域 record、ModLock、payload version、未知字段和领域不变量
迁移仍属于 Loreloom Store/Core/Content 契约。

### 11.0 Core 持久化契约

初始 Save Format 固定为 v1，并直接包含 WorldLock。Loreloom 尚未正式发布，Save Format、领域
payload 与内容 Schema 的破坏性变化都直接压平进各自初始 v1；不为开发期数据分配后续版本或增加
向下兼容分支，这些存档必须重建。Core 使用拥有所有权、
`deny_unknown_fields` 的类型表达以下 wire
shape；Store adapter 可以使用不同的私有数据库 row，但不得把 Toasty、SurrealDB 类型暴露给
Core、World、Agent 或 Runtime：

```rust
pub const SAVE_FORMAT_V1: u32 = 1;

pub struct WorldLock {
    pub world_id: ModId,
    pub version: semver::Version,
    pub content_hash: ContentHash,
    pub manifest_schema: u32,
    pub content_schema: u32,
}

pub enum ModSourceKind {
    Builtin,
    Directory,
}

pub struct LockedDependency {
    pub mod_id: ModId,
    pub version: semver::Version,
    pub optional: bool,
}

pub struct LockedMod {
    pub mod_id: ModId,
    pub version: semver::Version,
    pub content_hash: ContentHash,
    pub manifest_schema: u32,
    pub content_schema: u32,
    pub source_kind: ModSourceKind,
    pub dependencies: Vec<LockedDependency>,
    pub applied_patches: Vec<ContentDefinitionId>,
}

pub struct ModLock {
    pub mods: Vec<LockedMod>,
}

pub struct SaveManifest {
    pub format_version: u32,
    pub save_id: SaveId,
    pub world_id: WorldId,
    pub world_lock: WorldLock,
    pub mod_lock: ModLock,
}
```

v1 Manifest 必须包含 WorldLock。`ModLock.mods` 只保存启用的扩展 Mod，不得包含根世界或 Engine
Core；其 dependency topology 顺序且 Mod ID 唯一。每个 `dependencies` 按 Mod ID
byte-order 排序且唯一。required dependency 必须存在、版本精确相等并排在依赖者之前；optional
dependency 可以缺失，但已安装时同样必须精确匹配并排在依赖者之前。Manifest/Content Schema 必须
非零。`applied_patches` 保存实际应用顺序且 ID 唯一，不能在保存时重新排序。`source_kind` 不携带
机器路径。Load 使用完整 WorldLock 与 ModLock 检测候选变化，不能只比较 ID/版本，也不能把完整
Lock 不等直接作为拒绝原因；最终兼容性由第 10.4 节的候选重建与原子 adoption 决定。

Runtime 到 Store 的 durable unit 使用以下后端无关语义；具体 Rust 构造器可以通过
`ExecutionChangeSet` 生成它，但必须在打开事务前完成全部关联校验：

```rust
pub struct CommitRequest {
    pub command: WorldCommand,
    pub record_ops: Vec<VersionedRecordOp>,
    pub events: Vec<WorldEvent>,
    pub transcripts: Vec<TranscriptItemRecord>,
    pub safe_outcome: CommittedAction,
}

pub struct CommittedAction {
    pub action_id: ActionId,
    pub revision: Revision,
    pub event_ids: Vec<EventId>,
    pub safe_summary: ShortText,
}

pub enum CommitResult {
    Committed(CommittedAction),
    AlreadyCommitted(CommittedAction),
    Conflict { expected: Revision, actual: Revision },
    ActionIdentityConflict { action_id: ActionId },
}
```

所有 RecordOp 必须与 Command 的 ActionId、`expected_revision + 1` 和无缺口 order 一致；Event
必须与 Command 的 ActionId/Actor/new Revision 一致；Transcript 的 committed Revision 必须为 new
Revision，且它的完整 record upsert 必须出现在同一个 RecordOp 列表。`safe_outcome` 只能引用本
durable unit 的 Event，并与其 Action/Revision 一致。任何关联错误在事务开始前拒绝，不写入
ActionCommit。

第一阶段 request digest 固定为 SHA-256：输入为 ASCII domain separator
`loreloom.world-command.v1\0`，随后是 `WorldCommand` v1 按已冻结字段/tag 顺序、无额外空白编码的
UTF-8 JSON。所有 map/set 使用其 Core 类型的稳定排序。Checkpoint records 按 `RecordKey` 排序；
Checkpoint、RecordOp、WorldEvent、Transcript 与 ActionCommit 的 JSON payload 分别保存 SHA-256
完整性校验。校验输入均使用带版本的独立 domain separator，避免不同 row 类型间互换。Load 必须
先验证 checksum，再反序列化/迁移/重建；不能把数据库能解析 JSON 当作领域完整性证明。

### 11.1 必须保存

- Save/World identity、Schema version、规则/content version；
- 完整 `ModLock`：Mod/Pack ID、版本、内容哈希、依赖闭包、显式 Patch 与必要 migration
  provenance；
- 当前 committed Revision；
- 所有持久对象的 Stable ID 和持久 Component/Resource records；
- ContentOrigin/GeneratedOrigin，以及活动 Scene 中需要恢复的 Scene-scoped NPC；
- Character 的 AgentBinding、已提交 SceneState/DirectorState 和结构化 Narrative Directive；
- BaseAttributes、ResourcePool current/base maximum、AttributeAdjustment、Condition Instance、
  LifeState、ActionState 和 Posture；
- Item Instance、Container/Containment、Ownership、Equipment 和所有可变实例状态；
- Skill Grant、来源、等级、熟练度、enabled 和基于 World Clock 的冷却；
- ParameterSet、EventInstance、不能从其它事实重建的 RuleState 和已提交 Gameplay Action；
- 关系、Known Facts、Goals、Clock 和其它影响未来行为的状态；
- WorldCommand/WorldEvent 中被选为重建依据的有序记录；
- 持久 Transcript item 及其 committed/interrupted 状态；
- RNG stream/seed 中回放所需的事实；
- 必要的 migration provenance。

### 11.2 不得保存

- Provider API Key、Authorization header 或 Secret；
- Bevy `Entity`、ComponentId、Archetype、Schedule 和借用状态；
- Provider Client、HTTP response object 或 Rig 私有运行期类型；
- NarratorAgent/NpcAgent/AgentRunner 实例、ECS Snapshot 引用、临时 NarratorPlan、未开始的
  NpcTurnRequest 和 NpcTurnResult；
- TUI widget/focus 的临时内部对象；
- 未经明确产品决定的模型隐藏推理；
- 正在执行且没有明确恢复协议的 Future/Task；
- 可从当前采用的 ModLock 和 Definition 重新编译的 Registry 索引、Predicate/Effect plan 缓存；
- 可从 Definition、实例和规则重新计算的 EffectiveAttributes、effective resource maximum、
  总重量、装备加成、技能可用性、可用行动或 UI 状态文本缓存。

### 11.3 Save/Load

Save 必须：

- 写入临时/事务边界，不能把半个 Snapshot 暴露为可加载存档；
- 包含完整性校验和 Schema version；
- 只从一个一致 Revision 导出；
- 保留上一份可恢复状态，具体数量由 Store policy 决定。

Load 必须：

- 在物化 World 前验证元数据、版本和记录完整性；
- 在物化 World 前解析完整 ModLock，取得精确 Package/Hash，验证依赖/Patch 并重新编译 Registry；
- 缺失、不兼容或哈希不同的 Mod 必须经明确迁移或拒绝，不能使用同名近似版本继续；
- 创建新的 Stable ID -> Entity 映射；
- 分阶段创建实体、安装数据、解析引用、验证不变量；
- 任一关键错误时不把部分 World 发布给 Runtime；
- 不调用真实 Provider；
- 不重新生成 GeneratedOrigin NPC，直接物化存档中的完整领域状态；
- 验证所有 Parameter/Event/Rule record 仍符合锁定 Definition 与迁移结果；
- 成功后生成与 loaded Revision 一致的 UiSnapshot。

### 11.4 提交、幂等与 ECS/Store 顺序

一个成功 WorldCommand 的 durable unit 至少包含：

- 使 Snapshot/工作状态可重建的有序 RecordOp；
- 由该 Command 接受的 WorldEvent；
- 同一结果产生的持久 Transcript 变化；
- Save Head 的新 current Revision；
- 用于重复提交判断的 ActionId/commit identity。

这些记录必须在一个**显式** Store transaction 中 all-or-reject。事务先读取 Save Head 并验证
`expected_revision`，再写入全部 durable unit 并把 Head 更新为 `expected_revision + 1`。两个连接
从同一 Revision 竞争时，SurrealKV 的 write conflict 必须让恰好一个 commit 成功；serialization
failure 映射为 `Conflict`，Store 和 Runtime 都不得静默重试部分操作。

`ActionId` 在单个 Save 内唯一。`ActionCommit` 必须与其它 durable records 同事务写入，并至少保存
ActionId、Revision、规范化请求摘要或 digest 以及可安全重放的 committed outcome。重复 ActionId
且请求 digest 相同时返回已保存的 `AlreadyCommitted` outcome，不创建第二组 RecordOp、Event 或
Transcript；相同 ActionId 对应不同 digest 时返回 `ActionIdentityConflict`，不得把旧结果冒充为新
请求结果。只发生 expected Revision 冲突且没有 durable commit 的尝试不写入 ActionCommit；重新
规划必须使用新的 ActionId。

第一阶段选择“先修改受隔离的当前 ECS，durable commit 后才发布”的顺序：

1. Runtime 取得唯一世界执行槽，保留最后一个 committed UiSnapshot，并阻止新的 Observation；
2. World System 从 committed Revision N 执行 Command，在内存中产生 candidate N+1，同时返回
   拥有所有权的 RecordOp、WorldEvent、Transcript delta 和 Action identity；
3. Runtime 进入不可被普通取消打断的 commit critical section，使用 expected Revision N 提交
   durable unit；
4. 只有 Store 明确返回 committed，Runtime 才把 candidate 标记为 committed、发布 UiSnapshot 和
   成功 ToolResult；
5. 已知 rollback/Conflict 不得发布 candidate。Runtime 进入 recovery barrier，丢弃当前 World，
   从最后 committed Store Revision 重新物化后才恢复行动；恢复期间最多继续显示先前 committed
   UiSnapshot 与错误状态，不得查询 candidate World；
6. commit 结果不确定时，Runtime 关闭并重开 Store，通过 ActionId、request digest 和 Head
   Revision 判定结果，然后无条件从 durable Store 重新物化；判定完成前不得继续世界行动或发布
   新 Snapshot。

该方案符合 Armillae 当前同步 mutation-oriented System API，且不要求克隆任意 Bevy World。Spike
同时评估了“纯变更计划后应用”和“克隆 World 后交换”：前者会在第一阶段重复规则验证/应用路径，
后者无法要求所有 Mod Component/Resource 可克隆，因此不采用。崩溃时 ECS 只在内存中；重开仅以
Store 为准，所以提交前或事务中止恢复 N，commit 完成后恢复 N+1。

每个 Save 使用独立 SurrealKV 数据目录。物理备份第一阶段只在 command gate 关闭、所有事务已
解析且 Store handle 确定性关闭后执行；备份写入临时目录，记录 SaveId、Revision、Schema version
和内容校验，再经 fsync/原子 rename 发布。恢复与存档切换同样只在 handle 关闭时进行，并在物化
World 前验证备份 manifest、Head 和 records。SurrealDB driver 不暴露手动 KV checkpoint；领域
checkpoint 是一个普通显式事务中的版本化 Snapshot/compaction record，物理一致点由确定性关闭
承担。

## 12. TUI 规范

### 12.1 布局

宽度 `>= 80` columns 时：

- 左 pane 默认占约 30%，展示当前角色与可见世界状态；产品配置可以在 25%–35% 内调整；
- 右 pane 上部以 Transcript 叙事阅读为视觉中心，Runtime/Tool 状态只作临时或弱化辅助信息；
- 右 pane 底部固定保留多行输入编辑区；
- 输入区下方或边缘提供当前模式、Provider/Agent 状态、Revision 和快捷键提示；
- 使用克制的边框、间距和颜色层级，不直接展示 Rust Debug 枚举或为每一行添加协议标签；
- resize 后输入内容、grapheme cursor、滚动位置、已提交输入历史和 thinking 状态不得丢失。

Transcript 默认锚定当前窗口的最新内容。`transcript_scroll` 表达距最新内容的视觉行数，而不是从
顶部开始的绝对偏移；最大值必须按当前宽度折行后的内容行数与可见 viewport 计算并约束。
`PageUp`/鼠标滚轮向上查看旧内容，`PageDown`/鼠标滚轮向下返回最新；Page key 按一页减一行移动，
滚轮按少量视觉行移动。位于底部时新 Snapshot 自动跟随最新内容；用户已向上滚动时 Snapshot 更新
保留本地滚动距离，resize 只重新计算合法范围。这些交互不触发 Runtime 请求或世界修改。

宽度 `< 80` columns 时使用 State/Story 两个可切换页面，输入区在任一页面始终固定可达。布局
切换只是同一 UiSnapshot 与本地交互状态的纯投影，不能触发 Runtime 请求或修改数据。

### 12.2 UiSnapshot

UiSnapshot 至少包含：

- Revision 和当前 Session；
- 当前玩家 Actor 的展示身份与控制状态；
- 可见 Effective Attribute、Resource current/maximum、Condition 名称或症状、Life/Action/Posture、
  Location、Clock、Inventory、Available Skill、Goal、模组 Parameter 与活动 Event Option 摘要；
- Transcript items 的有界窗口与分页 Cursor；
- 当前 Runtime/Agent 阶段；
- 输入是否可提交、取消或等待；
- 非敏感错误和通知。

UiSnapshot 是拥有所有权且不可变的 View Model。TUI 不能通过 Widget callback 修改 ECS。

第一阶段 wire 形状补充冻结为：

```rust
pub struct VisibleActorView {
    pub actor_id: ActorId,
    pub display_name: DisplayName,
    pub controller: CharacterController,
    pub life_state: LifeState,
    pub action_state: ActionState,
    pub posture: Posture,
}

pub struct ParameterValueView {
    pub parameter_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub value: ParameterValue,
}

pub struct ParameterSetView {
    pub set_id: ObjectId,
    pub schema_id: ContentDefinitionId,
    pub values: Vec<ParameterValueView>,
}

pub struct EventOptionView {
    pub option_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub enabled: bool,
}

pub struct ActiveEventView {
    pub event_id: ObjectId,
    pub definition_id: ContentDefinitionId,
    pub display_name: DisplayName,
    pub current_node: ContentDefinitionId,
    pub node_text: ShortText,
    pub options: Vec<EventOptionView>,
}

pub struct TranscriptWindow {
    pub items: Vec<TranscriptItemRecord>,
    pub before_cursor: Option<TranscriptItemId>,
}

pub struct UiSnapshot {
    pub revision: Revision,
    pub session_id: SessionId,
    pub player: CharacterContext,
    pub scene: SceneContext,
    pub parameters: Vec<ParameterSetView>,
    pub active_events: Vec<ActiveEventView>,
    pub packages: PackageCatalogView,
    pub transcript: TranscriptWindow,
    // Runtime phase, submit/cancel/wait flags, Tool activity, notices and supporting Event IDs.
}
```

`SceneContext.visible_actors` 使用 `Vec<VisibleActorView>`，不能只给展示层裸 ActorId。Snapshot 的
Parameter 只包含 `public` 值；`narrator | owner | hidden` 需要独立授权投影，不能泄漏给 TUI。
Event Option 在 current Revision 求值 `visible_if`，不可见项不进入列表；可见项的 `enabled` 由
`enabled_if` 求值。`TranscriptWindow` 默认最多 64 项；存在更旧内容时 `before_cursor` 等于当前窗口
第一项 ID，调用方以“严格早于该 ID”请求上一页，没有更旧内容时为 `None`。

`PackageCatalogView` 只包含主世界 ID/版本、按 enabled-first 后 ID/版本稳定排序的 Mod ID/版本、
`enabled | installed` 状态、依赖数量、`PackageContentView` 和 unavailable installed candidate 数量。
`PackageContentView` 计数 Mod 自己声明的顶层 Definition，并分为 characters、scenes、places、items、
skills、conditions、events、gameplay actions、rules、parameters 与 support definitions；support 汇总
Agent Profile、Generation Policy、Tag、Relationship Kind、Attribute、Resource 和 Equipment Slot。
Narrator/NPC Prompt 文件数和 Patch 声明数独立计数；不递归统计 Event Node、Option、Predicate、
Effect 或其它嵌套对象。enabled 摘要来自实际编译候选在应用跨包 Patch 前拥有的内容；installed 摘要
来自启动扫描中通过独立 Package 校验的内容。catalog 不得包含完整内容哈希、本地绝对路径、包内文本
或字节数。World/ModLock 仍是 enabled 事实源，摘要和 disabled catalog 都是非持久化 Host 投影，
不进入 ECS、Agent Context、Transcript 或存档。

### 12.3 输入与 thinking 状态

- 输入按 Unicode grapheme cluster 编辑，支持多行、cursor 移动、删除、提交、取消和历史；
- `Enter` 提交，`Alt+Enter` 或 `Ctrl+J` 插入换行；运行中 `Esc` 产生 Cancel intent，空闲时
  `Esc` 不产生世界行为；`Ctrl+C` 产生 Quit intent；
- `Ctrl+O` 或 `F2` 打开/关闭只读 Mods overlay；继续接受终端实际发送 `ALT` modifier 的 `Alt+M`，
  并把 macOS 默认 `Option+M` 产生的 `µ` 作为兼容入口。`Command+M` 通常被终端截获，`Ctrl+M` 与
  Enter 使用同一控制码，二者不作为 TUI 快捷键承诺；overlay 打开时 `Esc` 只关闭 overlay，不取消
  Runtime，Up/Down、PageUp/PageDown 与鼠标滚轮滚动 catalog，其它字符不进入 editor；
- TUI 把输入作为数据发送给 Runtime，不自行构造 WorldCommand；
- `RuntimeClient::submit` 成功把输入加入 command queue 后，TUI 必须立即在 Transcript 末尾投影
  一条 `› ` 前缀的本地 pending 玩家行，再显示 thinking row，并回到最新内容；同步提交失败时不得
  显示 pending 行且必须恢复 editor；
- pending 玩家行不是 committed `TranscriptItemRecord`，不能进入 Snapshot、存档或 Agent Context。
  后续 Snapshot 包含匹配的 committed 玩家输入时由该记录替换；Completed、Cancelled 或 Failed
  终态 Snapshot 未包含该输入时也必须清除 pending 行，避免重复或残留；
- 等待 Provider 或 Runtime 编排时显示一个 ephemeral thinking row；文本由本地 Runtime phase 映射
  产生，例如 Narrator is thinking、NPC is responding、Updating world，不来自模型正文；
- thinking row 可以使用本地 spinner 动画；完成、取消或失败后由最终 Snapshot 替换或清除；
- Tool pending、succeeded、rejected 和 failed 通过 Runtime progress event 在执行发生时即时显示，并
  使用确定性、可区分的视觉状态；颜色只是增强信息，必要的文本含义必须保留；只显示名称、状态和
  脱敏错误码，不显示原始参数或 ToolResult；
- 当前回合 Tool Activity 是 ephemeral UI projection，不进入 Transcript、Agent Context 或存档；
  终态 Snapshot 到达后仍按“玩家输入、Tool Activity、Narrator 正文”顺序显示，但成功的常规 Tool
  不应淹没叙事内容；
- 终端 session 初始化顺序为 raw mode、alternate screen、隐藏 cursor、bracketed paste、mouse
  capture；正常退出、部分初始化失败与 panic unwind 均按逆序尝试恢复所有已启用状态；
- 渲染测试不得访问网络或真实 Provider。

### 12.4 TUI 产品状态与 Runtime Client

TUI 的公共产品边界冻结为同步、非阻塞 command/event adapter；这不是第二个 Runtime，也不允许在
render callback 中等待模型：

```rust
pub enum UiIntent {
    Submit(String),
    Cancel,
    Quit,
}

pub enum RuntimeUiEvent {
    Snapshot(Box<UiSnapshot>),
    PhaseChanged(RuntimePhase),
    ToolActivityChanged(Vec<ToolActivity>),
}

pub trait RuntimeClient {
    fn submit(&mut self, input: String) -> Result<(), UiClientError>;
    fn cancel(&mut self) -> Result<(), UiClientError>;
    fn try_recv(&mut self) -> Result<Option<RuntimeUiEvent>, UiClientError>;
    fn shutdown(&mut self) -> Result<(), UiClientError>;
}
```

`Box` 只避免大 View Model 扩张整个 event enum，仍表示事件独占拥有 Snapshot；它没有共享可变状态或
持久化语义。

`submit` 只把输入加入 Runtime command queue，不能同步执行 Agent Turn；`try_recv` 不得阻塞。具体
Runtime adapter 在应用装配层拥有 Tokio task/thread、`GameRuntime` 和可唤醒 cancellation token，
TUI crate 不依赖 Runtime/World/Store/Provider。`UiClientError` 只暴露稳定安全 code，不携带 Prompt、
输入、模型正文或 Tool 参数。第一阶段 adapter 只接受一个排队或运行中的 PlayerInput；重复提交返回
`runtime_busy`。`shutdown` 必须幂等地停止接受新输入、触发同一 cancellation token 并唤醒 worker
退出；之后提交返回 `runtime_shutdown`。worker/channel 故障只暴露 `runtime_start_failed`、
`runtime_snapshot_failed` 或 `runtime_disconnected`。这里的 shutdown 只定义 UI command/event adapter
生命周期，不解除第 11 节对 Store 确定性关闭、物理备份、恢复和存档切换的 driver 门禁。

`TuiApp` 拥有 `UiSnapshot`、grapheme editor、输入历史、窄屏页面、只读 Mods overlay、距最新内容的 transcript scroll、
当前折行 viewport 指标、至多一条已入队但尚未由 Snapshot 确认的 pending 玩家输入、当前 working
phase、本地 spinner frame 与当前 Turn 的 live Tool Activity。Mods overlay 拥有独立的有界滚动状态；
Snapshot 更新不得覆盖 editor/scroll
等本地交互字段；终态 Snapshot 清除 working phase/live Activity 并与 pending 输入对账，resize 只
重新 render。Editor 最大 UTF-8 bytes
固定为 `LongText` 的 65,536 bytes，单次插入 all-or-reject。`TuiConfig.state_width_percent` 默认 30，
只允许 25–35；event poll 默认 50 ms。

产品 `run` loop 每轮先无阻塞 drain Runtime event、再 render，并使用有限 poll 等待 Crossterm event；
因此等待 Provider 时仍可处理 cancel、quit、resize 和 spinner。`RuntimeUiEvent::Snapshot` 是唯一能
改变 committed transcript/世界状态的 UI 输入；本地 pending 玩家行、phase event 与 Tool Activity
event 都只是 ephemeral 投影，不获得 committed 语义。

## 13. 配置、Secret 与日志

配置至少分为：

- 非敏感：Provider kind、model、endpoint policy、Token/Tool/Rule budgets、UI 设置、save path、
  Mod source 与启用列表；
- Secret：API key/token，只通过 Secret source 解析；
- 世界内容：版本化根世界、Mod Package、Rule Bundle、Prompt 与资源，不混入 Provider 凭证；
- Session：当前 save/actor 等运行期选择，不当作全局配置。

要求：

- Secret 类型不得实现暴露值的 `Debug` 或序列化；
- 存档导出和错误报告自动排除 Secret；
- 默认 tracing 不记录完整玩家文本、Prompt、模型正文、Tool args/result；
- 可记录 Session/Actor/Action/Event ID、Revision、阶段、模型别名、Token usage、耗时和错误分类；
- Mod 日志只记录 Mod ID、版本、内容哈希、Definition ID、阶段和脱敏错误，不默认记录完整包内
  文本、规则参数或资源内容；
- 自定义 Provider endpoint 遵守 Armillae 的 URL/host 安全策略。

第一阶段产品二进制必须以 `--config PATH` 读取严格、`deny_unknown_fields` 的 TOML v1；未提供时在
创建/打开 World 和 Save 前明确失败。配置文件必须提供 `schema_version = 1`、`narrator` 与 `npc` 两个
Armillae `BridgeConfig`，并可收紧/配置单 Turn、整轮编排、Rule 与 TUI budget。`--world`、`--save`
与可重复的 `--mod` 仍是非敏感 CLI 配置源；它们不写入 Provider 配置或
Secret source。

Provider credential 只能使用 Armillae `CredentialRef::Environment` 或 `CredentialRef::File`；TOML
不能出现原始 key/token 字段，第一阶段二进制也不安装通用 `Resolver`。配置解析、错误 Display、
`Debug`、测试 Fixture 和存档都不得暴露解析后的 `SecretString`。二进制通过固定 Armillae revision 的
Rig factory 装配其支持的 Provider；真实网络测试保持 ignored，只有用户显式提供配置和凭证时运行。

二进制装配层必须在打开 World/Save 前分别预检 `narrator` 与 `npc`，并使用启动期
`ProviderSetupDiagnostic` 报告失败。诊断固定包含 `err_` correlation ID、Provider 槽位和稳定
setup code，可以包含经过安全字符约束的 Provider 名称；Environment credential 失败时还可以包含
配置中的环境变量名。用户提示必须说明修复动作，例如变量必须被导出到启动 Loreloom 的同一进程
环境，而不是只重复 `invalid_configuration`。Secret 值、凭证文件内容、完整文件路径和 Armillae
自由文本错误正文不得被保留或显示。

第一阶段 setup code 至少覆盖 `credential_reference_missing`、
`credential_environment_missing`、`credential_environment_invalid`、
`credential_environment_empty`、`credential_file_unreadable`、`credential_file_empty`、
`credential_resolver_unsupported`、`invalid_bridge_configuration`、`endpoint_not_allowed`、
`unsupported_provider`、`credential_resolution_failed`、`provider_configuration_rejected` 与
`bridge_creation_failed`。配置层必须从本地可验证事实产生这些代码；Bridge resolve/create 的兜底
分类只检查结构化 `BridgeError` variant，不解析 message/code。

显式 endpoint 必须同时通过 Armillae 结构校验和 Host `allowed_endpoint_hosts` 精确 allowlist；非
loopback host 只允许 HTTPS，HTTP 只允许显式列出的 localhost 或 loopback IP。未配置 endpoint 的
命名 Provider 使用 Adapter 自身默认 endpoint，不经过自定义 host 例外。配置加载或任一 Bridge
resolve/create 失败必须在创建/打开 World 和 Save 前结束；不能先发布 World 再发现 Provider Secret
或 endpoint 无效。

## 14. 并发、取消与故障

- Runtime Command 在 Working World 外部排队，由单一逻辑 owner 处理；
- Provider I/O 可以异步，但返回结果必须重新经过 Revision 校验；
- TUI render 与输入不得阻塞世界写入或 Provider cancellation；
- 一次 World execution 内部可以使用 Bevy 合法并行，业务事件顺序仍需确定；
- Rule 级联只能在 World 逻辑 owner 内按规范顺序推进，并受触发数、节点数、Effect 和深度预算；
- Store I/O 的并发和 backpressure 必须防止跨 Revision 乱序；
- Shutdown 顺序必须停止接收输入、取消未提交 Agent work、完成或安全中止 Store、恢复终端；
- panic 不属于常规错误恢复；可恢复失败必须通过 Result 和结构化状态传播；
- 如果 committed 状态不确定，Runtime 必须阻止继续行动，不能猜测成功。

## 15. 错误分类

跨层至少保留以下可判断分类：

| 类别 | 示例 |
|---|---|
| Input | 空输入、超过大小、非法命令模式 |
| Identity | Stable ID 非法、对象缺失、重复身份 |
| Permission | Actor 不可见、无 Capability、Tool 禁用 |
| Conflict | expected Revision 过期、对象状态已变化 |
| DomainRule | 距离不足、物品不可转移、前置条件不满足 |
| Agent | 预算耗尽、响应无效、未知 Tool |
| Content | Pack/Schema/版本/依赖/引用无效、Definition 冲突、SpawnSpec 编译失败 |
| Rule | Trigger/Predicate/Effect 未知或无效、求值预算耗尽、循环、Event Option 失效 |
| Generation | Generation Request 无效、Draft 越界、预算拒绝、无法形成合法 SpawnSpec |
| Provider | 鉴权、限流、超时、协议、不支持能力 |
| Tool | Schema、执行、结果序列化、重复注册 |
| Simulation | 生命周期、entry、system、clock 或 backend failure |
| Store | I/O、corruption、version、migration、commit uncertain |
| UI | terminal init/restore、render、input backend |
| Cancelled | 用户、shutdown、timeout 或上游取消 |

错误 Display 不得包含 Secret、完整 Prompt、完整 Tool 参数或私有世界事实。向模型、玩家和日志
投影的错误内容可以不同，但必须保留共同 correlation ID。

模型调用错误使用临时、非持久化的 `ModelFailureDiagnostic`。每次失败生成 `err_` UUIDv7
`FailureId`，并固定包含 invocation（`provider_setup | narrator | npc | npc_generation`）、调用阶段
（`configuration | request_encoding | projection | invocation`）和归一化 category。Category 至少覆盖
`request_encoding`、`invalid_configuration`、`unsupported_capability`、`invalid_request`、
`projection_incompatible`、`authentication`、`permission_denied`、`rate_limited`、`timeout`、
`cancelled`、`transport`、`provider_rejected`、`invalid_provider_response`、`stream_interrupted` 与
`unknown`。

诊断可以附带经过长度和可打印 ASCII 约束的 Provider 名称与 Provider Request ID、合法 HTTP 状态、
retryable 和 retry-after 毫秒；不得保留 `BridgeError` 的 message/code、HTTP body、header、URL、
Prompt、模型正文或 Tool 内容。`TurnOutcome::Failed` 必须携带诊断；NPC 失败时同一诊断进入
`NpcTurnResult.failure` 和玩家 warning，fatal Narrator 失败进入 `RuntimeError`，TUI 与 headless
显示同一 correlation ID 和安全字段。Bridge resolve/create 的启动期失败必须使用第 13 节的
`ProviderSetupDiagnostic`，并遵守与模型调用诊断相同的 correlation ID 和脱敏边界，不能退化为
无槽位、无稳定 setup code 的 Provider 错误。缺失模型调用诊断属于 Loreloom model protocol 错误，
不能伪造 `bridge_unavailable`。

## 16. 测试与验收门禁

### 16.1 P0 Spikes

产品 API 与最小可玩垂直切片实现前需要；workspace 与空 crate 脚手架可以先行：

1. **Store commit Spike**：以 SurrealDB + SurrealKV 为首选候选、SQLite 为对照，验证至少两个
   ECS/Store 提交策略并解决第 11.4 节；SurrealDB driver 候选固定为公开 Git URL
   `https://github.com/noctisynth/toasty-driver-surreal` 的 revision
   `0a7c87408e0daae0d6f5ed9f2b9d1ebf01d08549`，不得使用功能较早的 registry alpha 或本地路径。
   Spike 必须覆盖：
   - 显式事务原子写入 RecordOp + WorldEvent + Transcript + Save Head Revision；
   - 两连接 expected Revision CAS、serialization conflict 和 ActionId 重复提交；
   - 每个写入阶段的强制错误，确认没有部分 durable unit；
   - 提交前/中/后杀进程，重开后只能得到完整 Revision N 或 N+1；
   - 备份/恢复、确定性关闭、立即重开和多个存档路径切换；
   - 版本化 JSON 的未知字段、大整数、数组、嵌套对象、数据库空值与 JSON null；
   - 至少一万条 Record 的加载、重建、checkpoint 和提交延迟，以及构建体积；
   - 依赖可从公开 release/fixed revision 解析，且许可证与 Loreloom 发布方式兼容；
2. **Armillae/Bevy Spike**：以公开 Git URL
   `https://github.com/mmstudio-games/armillae` 的固定 revision
   `c9896fc4eb3a4f37918c0cabcefc84f8dcd69137` 为候选输入；该 revision 的 Simulation crates 尚无
   registry release，因此 Loreloom 必须验证干净 checkout 可拉取该 revision，并与其精确依赖的
   `bevy_ecs = 0.19.1` 使用同一类型版本。Spike 验证 Loreloom Component/System、ToolContext WorldGateway、
   Observation capture、Item/Container relation、Skill Executor、Attribute aggregation、
   Condition/Clock 和 Revision conflict 可由当前 Armillae API 表达；
3. **TUI Spike（已通过）**：以 registry `ratatui 0.30.2`、`crossterm 0.29.0` 为固定输入，已验证
   双栏、多行 Unicode 输入、resize、窄屏降级和终端恢复；Spike 中的 streaming 证据只证明
   ephemeral UI 状态能力，产品协议改用 Runtime thinking phase；证据见 [Spike 0003](../spikes/0003-tui.md)；
4. **Agent Loop Spike（已通过）**：固定使用与 World Spike 相同的 Armillae public Git revision，并用其
   `armillae-llm` Mock Bridge 与 `armillae-tools` ToolExecutor contract 验证多 ToolCall 顺序、ToolResult correlation、
   单一执行槽无重叠、请求开始时重新投影、配置化整轮/单 Turn budget、cancel 和 stale Revision；
   Spike 的模型 JSON envelope 不进入产品协议，产品 Plan 由原生 ToolCall 构造；证据入口为
   [Spike 0004](../spikes/0004-agent-loop.md)；
5. **Content/NpcFactory Spike（已通过）**：验证预设 Definition 与运行时 Draft 汇入同一 CharacterSpawnSpec、
   双阶段引用解析、全包失败回滚、GeneratedOrigin 保存和加载不重新调用模型；证据入口为
   [Spike 0005](../spikes/0005-content-npc-factory.md)；
6. **Mod/Rule Spike（已通过）**：验证内置/外部包共用加载管线、依赖/Patch/哈希锁定、类型化 Parameter、
   Event Option stale Revision、Rule 预算、包路径/资源限制和保存后精确重开；证据入口为
   [Spike 0006](../spikes/0006-mod-rule.md)。

Spike 只生成 `.agents/spikes/` 证据，不提前创建产品 API。

### 16.2 测试分层

- Core：ID、Revision、Serde/Schema round-trip、未知字段和错误投影；
- World：Component/System 规则、不变量、Stable ID 映射、容器循环、堆叠、装备、Skill Grant、
  消耗/冷却、Attribute 聚合、Resource 边界、Condition stack/expiry、正交状态、Parameter、
  Event/Rule 求值与确定性顺序；
- Tool：Schema、Capability、Actor Context、角色状态/物品/技能参数、Narrator NPC 决定、冲突和
  结果关联，以及 Event Option/Gameplay Action 通用 Tool 不允许 Mod 注入 Handler；
- Agent：Mock Bridge 的纯文本、单 Tool、多 Tool、无效 Tool、预算、取消和 Provider error；
- Orchestration：玩家输入只进 Narrator、Mention/Materialize/NpcTurn 语义、ToolCall/Plan 顺序、
  Runtime 校验、同 Revision Context、临时 NpcAgent/NpcTurnResult 生命周期、严格串行、自然语言
  结束或继续调用 Tool、整轮/单 Turn 预算、Provider 等待时 World Clock 暂停和 Narrator/NPC 权限隔离；
- Content：Mod/Pack/Definition/Rule Schema、依赖图、哈希、显式 Patch、跨引用、冲突、
  CharacterSpawnSpec/Rule plan 编译、Content/Generated Origin、部分失败和版本不匹配；
- Store：Snapshot/record round-trip、ModLock、Parameter/Event/RuleState、领域与数据库迁移、
  Revision CAS、ActionId 幂等、事务中途失败、截断写入、corruption、backup/restore、存档切换、
  rebuild 和 crash simulation；
- Runtime：输入到提交/UiSnapshot 的端到端状态机，以及 Mod 发现、原子安装和精确版本重开；
- TUI：布局和 View Model snapshot、窄屏、resize、Unicode、多行输入、terminal restore；
- Replay：不调用 Provider，从固定初态、ModLock 和记录重建等价领域状态；
- Security：Secret redaction、未授权 Observation/Tool、包路径逃逸、资源耗尽、恶意 Patch 和
  Prompt/Tool Capability 注入。

真实 Provider 测试默认 ignored，只在显式凭证与授权下运行。

### 16.3 质量门禁

实现完成时至少运行：

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
```

CI 使用最新 stable，不执行 MSRV Job，不允许 manifest 出现 `rust-version`。

## 17. 第一阶段验收

只有以下事实都有自动化或可复现证据，第一阶段才可标记完成：

1. 新建世界后可在 TUI 看到玩家角色、地点、基础状态和输入框；
2. 玩家输入可产生 Transcript，并在需要时通过 Agent Tool 请求世界行动；
3. 无成功 Command Tool 时，模型文本不会改变结构化状态；
4. 成功移动或物品转移后，左侧 UiSnapshot 在新 Revision 显示结果；
5. 背包由唯一 `ContainedBy` 关系推导，转移、装备、嵌套和存档重建不产生双重事实源；
6. 物品堆叠只在实例状态等价时合并，拆分创建可追踪的新 Stable ID；
7. Agent 只能使用当前 Actor 已持有且可用的 Skill Grant；
8. Active Skill 经 `use_skill`、WorldCommand 和规则 System 结算资源、冷却和 Event；
9. Passive/Reaction Skill 不会隐式启动无预算 Agent Loop；
10. NarratorAgent 根据玩家意图选择纯叙事提及、`create_npc` 或 `request_npc_turn`；创建请求只包含
    source/lifetime/mode；
11. Runtime 注入 Scene/Place/Revision/GenerationPolicy/AgentProfile，不用关键词替 Narrator 判断
    叙事重要性，也不把内部物化状态暴露为模型参数；
12. 预设 Definition 与运行时 Draft 汇入同一 CharacterSpawnSpec/NpcFactory/SpawnNpcCommand；
13. Content Pack 跨引用错误不会留下部分 Scene，Generated NPC 加载时不重新调用模型；
14. Scene-scoped NPC 与 Scene 状态在停用后仍可存档恢复，不因离开 Scene 而删除；
15. Preset/Generated NPC 只在 Narrator Turn 结束后物化，并在同一玩家输入内把 committed ActorId
    与完整角色投影交给 Narrator；物化前不能为该角色启动 NpcAgent；
16. `request_npc_turn` ToolCall 经 Runtime 校验后，才按已接受 ToolCall 的顺序创建一次性 NpcAgent；
17. NpcAgent 的 CharacterContext、SceneContext 和 Assignment 绑定该 Turn 开始时的同一
    committed Revision；
18. NpcAgent 不持有 Provider/World，也不能把 Character Snapshot 直接写回 ECS；
19. EffectiveAttributes 按 Base、Flat、Multiply、Override、Clamp 与稳定 source 顺序重建；
20. Resource current 和 Condition Instance 保存后等价恢复，effective maximum 可重新计算；
21. Condition stack、duration、source、periodic effect 和 World Clock expiry 符合 Definition；
22. LifeState、ActionState 和 Posture 可以并存，不能由单一 CharacterState 互相覆盖；
23. 未诊断 Condition 只投影允许感知的症状，不泄漏真实 Condition 名称；
24. Narrator 初始 Attribute/Condition Hint 经 NpcFactory 校验，创建后不能直接 set 状态；
25. NPC Observation 不包含未授权世界事实或完整物品/技能内容库；
26. Agent Tool 顺序、预算、取消和过期 Revision 行为符合本 Spec；
27. 保存、关闭、重开后 Stable ID 与领域状态等价，不依赖旧 Bevy Entity；
28. 回放固定记录不访问 Provider 并获得等价世界结果；
29. Store 写入故障不会让 Runtime 把不确定状态继续当作已提交；
30. Provider 失败后世界可继续或进入明确可恢复状态；
31. 测试专用 Mock Bridge 的完整纵向切片无需网络通过，生产二进制不装配 Mock Bridge；
32. 宽屏双栏和窄屏降级都能完成查看、输入、取消与退出；
33. 日志、错误、存档和测试快照不含 Secret；
34. 最新 stable 的全部质量门禁通过且无 `rust-version`；
35. 根世界与外部 Mod 通过同一 Registry/Factory/提交路径加载，并分别进入 WorldLock/ModLock；
36. Mod 依赖缺失、循环、哈希不匹配、重复 Definition 或无效 Patch 在 World 修改前失败；
37. 模组 Parameter 值按 Schema、范围、引用和可见性校验，并可等价保存恢复；
38. Event Option 在 current Revision 重新验证，过期或不满足 Predicate 时不产生 Effect；
39. Rule Trigger/Predicate/Effect 按稳定顺序和预算执行，只能产生白名单 WorldCommand；
40. `choose_event_option`/`perform_gameplay_action` 使用引擎通用 Tool，数据 Mod 不能注入 Handler；
41. 包路径逃逸、超限文件/递归/解压和对文件、网络、Shell、Secret 的访问被拒绝；
42. WorldLock/ModLock 变化触发候选重建：Prompt-only 与兼容增量内容可以采用，缺失实际依赖时精确
    报告 Mod/Definition 且不修改存档，同名近似版本不会被静默替代；
43. 相同 ModLock、初始状态与规则记录无需 Provider 即可重建等价 Event/Parameter/世界状态；
44. RecordOp、WorldEvent、Transcript 和 Revision 在故障注入下始终同成同败；
45. 双连接从同一 expected Revision 竞争提交时恰好一个成功，重复 ActionId 不产生第二份结果；
46. 提交前/中/后崩溃并重开时只出现完整 Revision N 或 N+1，不发布不确定 UiSnapshot；
47. **UPSTREAM-GATED**：SurrealDB 暴露可等待 embedded shutdown 后，一致备份可恢复到可加载
    Revision，关闭、立即重开和存档切换不会串扰数据；
48. Store 依赖可由干净 checkout 从公开来源解析，许可证、构建体积和性能满足发布门槛；
49. NarratorAgent 与所有 NpcAgent Turn 不重叠执行，排队请求只在获得单一执行槽后从当前
    committed Revision 重新校验和投影；
50. 玩家自然语言只进入 NarratorAgent，NpcTurnRequest 的数量和语义顺序由其原生 ToolCall 决定，
    Runtime 不增加叙事优先级或公平性判断；
51. NpcTurnResult 的自然语言 response 不会直接成为世界事实；成功 ToolCall/WorldCommand 对应的
    ToolResult/WorldEvent 才是结构化状态 provenance；
52. Narrator 可以结束编排或在总预算内继续调用编排 Tool；不存在固定 NPC 数量常量，配置化整轮/
    单 Turn 预算和最大编排轮数能终止循环；
53. 等待 Provider 时 TUI 的 thinking 状态、取消和退出保持响应，不展示未完成模型正文，逻辑
    World Clock 不随墙钟时间隐式推进；
54. Scene 切换原子停用当前 Scene 并激活或首次物化目标 Scene；再次进入恢复原状态且不调用 Provider。
55. Observation 标记可调度的现有 NPC 后，Narrator 只需提交 ActorId 与 assignment；Tool 拒绝时
    AgentRunner、UiSnapshot 与 TUI 保留脱敏错误码，且不持久化完整参数或 ToolResult。
56. Content Place edge 物化为同 Scene 双向 ObjectId 连接，普通移动不能越过未连接 Place；存档重建
    后连接等价且未知、跨 Scene、单向或自连接 edge 被拒绝。
57. Narrator 可延迟创建 inactive Scene + entry Place，或在 active Scene 创建并双向连接 Place；
    Runtime 分配身份与 GeneratedOrigin，创建不自动切换/移动，提交后强制重规划，NpcAgent 无该权限。
58. ToolCall 在开始执行前实时显示 Pending，并在结果返回后原位收敛；UI 只接收 call ID、名称、状态
    和脱敏错误码，终态按玩家输入、Tool Activity、Narrator 正文排序，且 Activity 不进入 Transcript、
    Agent Context 或存档。
59. Provider 启动失败在创建/打开 World 和 Save 前报告 `narrator`/`npc` 槽位、稳定 setup code、
    安全 Provider 名称与可执行提示；Environment credential 问题可以显示变量名，但任意错误 Display、
    Debug 与测试输出均不包含 Secret、凭证文件内容或 Armillae 原始错误正文。
60. `Ctrl+O`/`F2` 打开只读 Mods overlay，并兼容终端可区分的 `Alt+M`/macOS `Option+M`；面板单独
    显示主世界、enabled ModLock 与通过独立 Package 校验的 installed-but-disabled 目录包。每个 Mod
    显示顶层 Definition 总数、非零分类、Prompt 与 Patch 数量；无效 installed candidate 只显示汇总
    数量，列表不含 hash、路径、内容文本或字节数，滚动与关闭不产生 Runtime intent、WorldCommand
    或持久化变化。

## 18. Active Spec 下的范围化实施门禁

RFC 0001 已于 2026-08-30 被项目方接受。以下事项继续阻塞对应公共 API、持久化格式或产品行为，
但不阻塞 workspace、空 crate、Semifold、测试数据目录和 P0 Spike：

- Stable ID 编码、record envelope 与 Command/Event/RecordOp 重建权威关系已冻结；首个公开版本前
  只有当前 v1 codec，不存在已激活的领域兼容 migration；
- Store driver 的 AGPL 兼容分发方式已通过 Loreloom 采用 `AGPL-3.0-only` 确认；后端、公开依赖
  revision 与 commit/failure 协议已由 P0 Spike 冻结；物理 backup/restore/switch 仍受 SurrealDB shutdown
  上游能力门禁；
- 第一阶段 PlayerInput 不做代码层说话/行动关键词分类，原文只进入 Narrator；`create_npc` 的
  source/lifetime/mode、`request_npc_turn` 的最小参数、内部物化状态、可配置数量 policy、promotion
  与持久 Scene 激活语义已冻结；
- Known Fact/Belief/Goal/Transcript 的 v1 最小 Schema 与有界上下文投影已冻结；
- Content Pack 的目录布局、Manifest 与加载边界已冻结；Definition ID 的最终编码和各领域完整 JSON
  字段/迁移版本仍受对应门禁。CharacterSpawnSpec、NpcFactory、Content/Generated Origin 和导入
  transaction 的逻辑边界已由 P0 Spike 冻结；
- Mod Manifest/ID/版本/兼容范围、目录布局、依赖图、内容哈希、包来源/资源限制、显式 Patch、冲突
  策略和 ModLock 已由 P0 Spike 冻结；第一阶段明确不承诺 package signature；
- Event Definition/Instance、Option、Parameter Definition/Set、Gameplay Action、Trigger/Predicate/
  Effect 白名单、可信 rule initiator/provenance、执行顺序和预算边界已冻结；Fixed 的底层数值编码
  仍由第 6.4 节门禁决定；
- 第一阶段明确排除 Extension Mod；WASM Host API、Capability、配额与签名拆到独立后续 RFC；
- Core 拥有 CharacterSpawnSpec 与持久领域 record；Content 拥有 Definition/NpcDraft 与统一纯编译器；
- Attribute ID/Fixed、Modifier、Resource maximum、Condition、症状投影与正交状态 v1 已冻结；
- Item/Skill Definition v1、content version/migration、堆叠/拆分、质量单位和受限 Skill Effect
  Executor 已冻结；
- TUI 技术栈 P0 结论记录；
- Armillae 当前 API 与候选架构的 Spike 通过；
- 新增 **OPEN** 标记必须链接到明确阻塞范围的后续 RFC；当前第一阶段产品协议不再包含未路由的
  OPEN 条款。

本 Spec 保持 Active；实施清单必须把上述门禁映射到具体任务，未解除门禁的 crate 只能保留无公共
领域 API 的脚手架或承载明确标注的 Spike。
