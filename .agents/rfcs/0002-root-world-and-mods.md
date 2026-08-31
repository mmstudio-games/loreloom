# RFC 0002：根世界与 Mod 扩展边界

> 状态：Accepted
> 接受日期：2026-09-01
> 决策方：项目方
> 影响范围：Content、Runtime、Store、应用装配、内容目录与 Agent Prompt
> 替代范围：RFC 0001 中把主游戏内容称为“内置 Mod”的部分

## 1. 背景

现有纵向切片把 Rainbound Inn 的 Scene、Character、AgentProfile 与英文叙事文本硬编码在
`crates/loreloom/src/demo.rs`，再在内存中伪装成一个 builtin Mod Package。二进制始终选择这个
固定 Scene，外部 `--mod` 只能加入同一个 Registry，不能成为主故事，也不能为 Narrator 提供世界级 Prompt。

这种实现验证了统一 Registry/Factory 管线，却混淆了两个产品概念：主游戏是唯一、特殊且可直接
开始游玩的世界；Mod 是安装并选择性启用在该世界之上的扩展。项目方确认 Loreloom 第一阶段不需要
多 World Package 或 `--world ID`，游戏根目录本身就是唯一主世界。

## 2. 决策

Loreloom 游戏根目录固定包含一个 `world.toml`，以及主世界拥有的 `content/`、`rules/`、
`prompts/`、可选展示资源和 `mods/`。根世界不是 Mod，不使用 `mod.toml`，也不进入 `ModLock`。

```text
world.toml
content/*.json
rules/*.json
prompts/*.md
locales/*.json
assets/**
mods/<mod-id>/mod.toml
.loreloom/                 # 本地配置、存档、启用状态；不属于内容
```

`world.toml` 是主世界 Manifest v1，至少声明：

- reverse-DNS lowercase `world_id`、SemVer、Engine requirement 与 Content Schema；
- 初始 Scene Definition、Inventory Root Definition 与 Spawn System Definition；
- 显式 content/rule/resource 文件列表；
- `[prompts]` 中有序的 Narrator/NPC 全局 Prompt `prompts/*.md` 相对路径；
- `follow_player` 或固定语言标签形式的响应语言策略。

主世界 Manifest、全部声明文件和两类 Prompt 原始 bytes 参与 `WorldLock.content_hash`。主世界
与启用 Mod 仍进入同一个 DefinitionRegistry、跨引用/Patch 验证、SpawnSpec、NpcFactory、Rule 与
WorldCommand 管线；“根世界不是 Mod”不能产生跳过 Schema、哈希、安全或 Factory 的旁路。

## 3. Agent Prompt 所有权

引擎只保留不可覆盖的协议约束：ECS/Tool 权威边界、结构化控制不得来自模型正文、Capability、
Secret 与日志策略。叙事语言、世界背景、Narrator 人格和文风由根世界 Narrator Prompt 拥有；NPC
共享的世界观、行为基调和叙事协作约束由根世界 NPC Prompt 拥有。`world.toml` 与 `mod.toml` 使用
相同的 `[prompts] narrator = [...]`、`npc = [...]` 结构：根世界提供基础列表，启用 Mod 只能追加。

Narrator Model Call 的消息顺序固定为：

1. Engine-owned protocol instruction；
2. World-owned Narrator Prompt，按声明顺序；
3. Mod-owned Narrator Prompt，按依赖拓扑、再按声明顺序；
4. World-owned response language policy；
5. Runtime 投影的 ECS Observation、结果与玩家输入。

NPC 的消息依次包含 Engine 协议、`AgentProfile.system_style`、根世界 NPC Prompt、按依赖拓扑与声明
顺序追加的 Mod NPC Prompt、响应语言策略和 Runtime Context。Prompt 是不可信叙事输入，不能注册 Tool、
扩大 Capability 或覆盖 ECS/Tool 的代码级校验。JSON Observation 的字段名属于稳定机器协议，不作为
可本地化叙事文本。

## 4. Mod 语义

`mods/` 是已安装扩展的约定目录；第一阶段不会仅因目录存在就自动启用。可重复 `--mod PATH` 明确
选择本次候选闭包，存档以 `ModLock` 固定实际启用的 Mod、版本、哈希、依赖与 Patch。未来本地
`.loreloom/` loadout 可以提供等价选择，但不能进入主世界内容哈希。

Mod 可以增加或受约束地替换 NPC、Scene、Item、Skill、Event、Parameter、声明式 Rule、Prompt 与
展示资源；它不能替换引擎协议 Prompt、扩大 Tool Capability、访问 Secret、网络、Shell 或注入本机
代码。Extension Mod 仍由后续独立 RFC 决定。

项目方于 2026-09-01 确认根世界和 Mod 统一使用 `[prompts]`，其中 `narrator` 与 `npc` 均为有序路径
列表；项目尚未发布，因此直接更新 Manifest Schema v1，不保留旧 `narrator.prompt` 字段兼容。

## 5. 持久化

初始 Save Format v1 直接分别保存：

- `WorldLock`：主世界 ID、版本、Manifest/Content Schema 与完整内容哈希；
- `ModLock`：仅包含实际启用的 Mod，不包含主世界或 Engine Core；
- 运行时 `WorldId`：仍是单个存档内的 UUIDv7，不等于内容 `world_id`。

Loreloom 尚未正式发布，不为本 RFC 之前的开发期 demo 存档提供向下兼容或迁移；这些存档缺少
WorldLock 时必须在物化 ECS 前拒绝并重建。该原则同样适用于首个公开版本前的领域 payload 和内容
Schema：当前结构直接成为初始 v1，不分配开发期 v2 或注册 legacy migration。绝对路径、Provider
配置和 Secret 不进入任何锁。

## 6. 应用装配与迁移

- `loreloom --world PATH` 指定游戏根目录，默认 `.`；它选择目录，不选择多个 World Definition；
- Rainbound Inn 迁移到仓库根级 `world.toml`、`content/` 与 `prompts/`；
- 删除生产代码中 Scene、Character、Narrator Prompt 与剧情正文的 Rust 硬编码；
- 无 Provider 的确定性 Bridge 只允许作为测试设施，不能成为生产叙事来源；
- `--config` 与 `.loreloom/` 继续拥有 Provider、Secret、预算、存档和本地启用状态，不参与内容哈希。

## 7. 验收

1. 修改根世界 Prompt 或内容会改变 WorldLock，并阻止不匹配存档静默打开；
2. 修改未启用的 `mods/` 目录不改变 WorldLock/ModLock；
3. 启用 Mod 只改变 ModLock，主世界仍不出现在 `ModLock.mods`；
4. 新建世界完全从目录内容物化，删除 Rust 内的 Rainbound Inn Definition 后仍可运行；
5. 中文根世界 Prompt 与中文玩家输入会把明确中文响应约束传给 Narrator/NPC；
6. Engine Prompt、Tool Schema、ECS 与安全边界不能被根世界或 Mod 覆盖；
7. 旧开发期 Manifest 不会引入兼容分支或被无依据地升级，新的初始 v1 Schema 直接要求 WorldLock。
