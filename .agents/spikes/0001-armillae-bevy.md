# P0 Spike 0001：Armillae/Bevy 集成

> 状态：Completed
> 开始日期：2026-08-30
> 完成日期：2026-08-30
> 规范来源：[Runtime Active Spec §16.1](../specs/runtime.md#161-p0-spikes)

## 目标

验证 Loreloom 可以通过公开、固定的 Armillae revision 使用 Simulation/Bevy API，并在不泄漏
Bevy `Entity`、不跨异步边界持有 World 借用的前提下表达第一阶段世界协议。

## 固定候选输入

- Public Git：`https://github.com/mmstudio-games/armillae`
- Revision：`c9896fc4eb3a4f37918c0cabcefc84f8dcd69137`
- `armillae-simulate` / `armillae-simulate-bevy`：`0.1.0-alpha.0`
- `bevy_ecs`：`=0.19.1`，必须与 adapter 使用同一解析版本

Simulation crates 当前没有 registry release，因此不得使用本地路径或浮动 Git branch。若固定公开
revision 无法从干净 checkout 解析，本 Spike 失败。

## 验收

- [x] 固定 public Git revision 可由 Cargo 独立解析；
- [x] 注册并激活 Loreloom Bevy module；
- [x] 类型化命令输入经 JSON Schema 校验后执行 System 并返回结构化输出；
- [x] `expected_revision` 冲突在 World System 内被结构化拒绝，成功命令单调推进 Revision；
- [x] Observation 通过 closure-scoped 只读投影获得，不返回 ECS 引用或 Bevy `Entity`；
- [x] Item/Container、Skill Grant、Attribute Modifier、Resource、Condition 与 World Clock 可以由
  Component/关系/System 表达并确定性查询；
- [x] Armillae execute/advance 等待边界之外不长期持有 ECS 可变访问；
- [x] Mock/测试不调用真实 Provider；
- [x] 记录依赖图、构建证据、缺口与最终结论。

## 禁止提前冻结

本 Spike 可以定义测试专用 Component、Resource、Command 和 Observation，但不得把它们作为
`loreloom-core` 或 `loreloom-world` 的稳定公共 API。最终类型必须在 Spike 结论同步回 Active Spec
后才能建立。

## 自动化证据

测试文件：`crates/loreloom-world/tests/armillae_bevy_spike.rs`。

- `typed_command_uses_revision_cas_and_owned_observations`：从 Revision 0 执行类型化休息命令，
  成功推进到 1；旧 expected Revision 返回 `Rejected`，Simulation 保持 Active；
- `json_schema_rejects_invalid_commands_before_world_execution`：缺字段、错误类型和未知字段在
  World System 前被 JSON Schema 拒绝，Revision 不变；
- `world_clock_advances_without_exposing_ecs_entities`：类型化 Clock 从 41 推进到 42；
- Observation 在 `inspect_world` closure 内从 Component 生成完全拥有所有权的投影，按 Logical ID
  确定性聚合 Attribute Modifier、Skill Grant、Resource、Condition 和容器内物品；返回值不含
  Bevy `Entity` 或 World 引用。

验证命令：

```sh
cargo test -p loreloom-world --test armillae_bevy_spike
```

结果：3 passed。

## 依赖与边界结论

- Cargo 解析到固定公开 revision 的 `armillae-simulate` / `armillae-simulate-bevy
  0.1.0-alpha.0` 和精确 `bevy_ecs 0.19.1`；Manifest 与 lockfile 不含本地路径；
- `BevyModule`、`BevySimulationBuilder`、`ExecuteContext`、typed Clock 和 closure-scoped World
  access 足以承载第一阶段边界；不需要修改 Armillae；
- Armillae 把 `SystemExecutionError` 视为 Simulation fault。Revision conflict 属于预期领域拒绝，
  因此必须通过类型化 `Rejected` output 返回；真正的世界结构/执行错误才返回 System failure；
- 当前 execute 是 mutation-oriented System，并不提供任意 Bevy World clone 或 ECS transaction。
  Store 提交失败必须由 Runtime 隔离 candidate Observation 并从 durable Store 重建，不能假设
  Armillae 自动回滚；
- 测试只使用确定性输入，不加载 LLM Bridge 或真实 Provider。

## 最终结论

Spike 通过。固定 Git revision 可以作为 Loreloom 第一阶段 World/Simulation 输入；稳定领域类型、
WorldGateway 与 Tool API 仍须按 Runtime Spec 的后续门禁冻结，本测试中的类型不升级为公共 API。
