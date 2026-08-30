# Freeze 0002：MVP Domain 与 Content v1

> 状态：Frozen
> 日期：2026-08-30
> 权威契约：[Runtime Active Spec](../specs/runtime.md)

## 数值与时间

- `Fixed` 是 signed i64 micros，scale 1,000,000；i128 中间值、ties-to-even、checked overflow。
- `WorldTime` 是只由显式世界 Command/System 推进的 u64 逻辑秒 tick。
- mass/capacity 使用 Fixed 克，距离使用 Fixed 世界单位，存档不保存 float。

## Domain record v1

Store codec 只接受以下 typed aggregate record：`world_state`、`scene`、`place`、`character`、`item`、
`condition`、`skill_grant`、`relationship`、`known_fact`、`goal`、`event_instance`、`parameter_set`、
`rule_state` 与 `transcript_item`。所有 payload 使用 `deny_unknown_fields`，并通过 Core record envelope
保存。多实例事实各自拥有 ObjectId；派生属性、容器内容列表、可用技能和 UI 文本不保存。

Character 保存 Profile、Controller/Lifetime、Location、Inventory root、Base Attribute、自由
Adjustment、Resource current/base maximum、Life/Action/Posture 与可选 AgentBinding。Item 保存
Definition、Stack、Durability、Container capability、唯一 ContainedBy、独立 OwnedBy/Equipped、
instance parameter 与 origin。Condition/SkillGrant/Relationship/Knowledge/Goal 使用独立 record。

## Content document v1

`content/*.json` 顶层固定为 `{ "schema_version": 1, "definitions": [...] }`；Definition 使用 `type`
tag 并拒绝未知字段。v1 支持 AgentProfile、Attribute、Resource、Condition、Item、Skill、Character、
Place、Scene 与关系/目标初值。Content compiler 与运行时 NpcDraft 都输出 Core 的同一个
`CharacterSpawnSpec`；NpcFactory 只消费该可信编译结果。

Item stack equivalence 精确比较 Definition/content lock、durability、custom name、bound actor、
instance parameters/modifiers 与 origin；ID、quantity、location、owner 不参与。Container、equipped
或拥有内容的 item 不可 stack。拆分复制等价字段并分配新 ObjectId。

Skill cost 是唯一 Resource 列表；target 是 tagged self/character/object/place；Active/Passive/
Reaction 都只使用 Engine allowlist 与声明式 Effect plan，Reaction 不启动 Agent。

## NPC 与长期上下文

Beat 只 MentionOnly；Scene/Persistent materialization 使用完整 Character record。Lightweight 表示
NarratorProxy/Rules controller 且无 AgentBinding，不是裁剪数据。请求 NpcTurn 要求 Agent
controller 与 enabled binding。跨 Scene 引用必须在同一 Command 先 promotion；Scene cleanup 只删
除无持久强引用的 Scene-owned closure。

KnownFact/Goal 是结构化决策事实，Transcript 是对话归档。第一阶段没有模型生成的隐藏 Memory 或
摘要模型，Agent 只接收确定性、有界、可配置投影。
