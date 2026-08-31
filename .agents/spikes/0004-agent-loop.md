# P0 Spike 0004：Agent Loop 与 Narrator 编排

> 状态：Completed
> 开始日期：2026-08-30
> 完成日期：2026-08-30
> 规范来源：[Runtime Active Spec §10](../specs/runtime.md#10-agent-runtime)

> 历史证据说明：本 Spike 验证的串行执行、Tool correlation、预算、取消和 Revision 结论继续有效；
> 其中由模型正文生成 NarratorPlan/NpcModelOutput/NarratorSynthesis JSON 的实验协议已于
> 2026-08-31 被产品协议取代。当前权威行为见 Active Runtime Spec 10.1/10.5：模型正文是自然语言，
> 内部 Plan 由 Provider 原生 ToolCall 构造。产品回归证据位于 `loreloom-runtime/tests/runtime_flow.rs`。

## 目标

验证 Loreloom 可以在 Armillae 单次 `LlmBridge`/`ToolExecutor` 合约之上显式实现 Agent Turn 和完整
PlayerInput 编排，并保持 Narrator/NPC 严格串行、current Revision 上下文重投影、Tool 结果关联、
两级预算、取消与 durable fact 边界。

## 固定候选输入

- Public Git：`https://github.com/mmstudio-games/armillae`；
- Revision：`c9896fc4eb3a4f37918c0cabcefc84f8dcd69137`；
- `armillae-core` / `armillae-llm` / `armillae-tools`：`0.1.0-alpha.1`；
- `armillae-llm` 只启用 `mock` feature，不加载真实 Provider Adapter；
- 测试只使用 registry async/test 辅助依赖，不使用本地 path、网络、ECS、Store 或真实时钟等待。

## 候选预算与状态语义

- `TurnBudget` 与 `OrchestrationBudget` 都是配置值，不允许模型或 Mod 扩大；
- 单 Turn 至少限制 Model Call、ToolCall、input/output/total token、最终输出 bytes 与墙钟 deadline；
- 整轮至少限制 Narrator/NPC Turn、累计 Model Call、ToolCall、token、输出 bytes、墙钟 deadline 和
  最大编排轮数；不建立固定 NPC 数量常量；
- 缺失 Provider usage 不能按 0 token 处理：保留 unknown usage，并依靠 Model/Tool Call、输出与
  deadline 硬限制；若配置要求精确 token accounting，则在缺失 usage 时结构化停止；
- 每次 Tool 前再次检查 cancel 与预算；已提交 Tool 保留，未开始 Tool 不执行；
- queue 中每个已接受 NpcTurnRequest 恰好产生一个同 ID result，包括 stale、cancelled 和
  budget-exhausted；
- Narrator 最终文本只能引用 durable gateway 返回的 committed event/fact，NPC 自述的动作文本不
  自动成为世界事实。

这些字段和默认值只有在 Spike 通过并同步回 Active Spec 后才能冻结。

## 验收

- [x] Armillae Mock Bridge 记录每次 canonical request，Loreloom 显式决定是否再次调用；
- [x] 单个 completion 中多个 ToolCall 按模型顺序逐个交给一次调用一次结果的 ToolExecutor；
- [x] ToolResult 保持原 ToolCall ID，并按 assistant message 后接 tool-result message 的 canonical
  history 进入下一次 Model Call；
- [x] PlayerInput 只进入 Narrator planning，随后按 Plan 顺序运行零个或多个 NPC，最后 synthesis；
- [x] execution-slot 探针证明 Narrator/NPC Model/Tool Loop 不重叠；
- [x] 每个 NPC 开始时从 current committed Revision 重新校验并投影同 Revision Character/Scene/
  Assignment；stale request 不调用 Bridge；
- [x] NPC 输出文本不伪造 committed fact，synthesis 只接收成功 Tool 对应 committed event；
- [x] 单 Turn 与整轮限制都能独立停止执行，限制从全局/save/profile 取最严格值；
- [x] cancel 与在途 Model future 竞速，并停止未开始 Tool/NPC，同时为全部已接受 request 生成关联结果；
- [x] Model/Tool/provider 错误、未知 Tool 和 stale Revision 都形成结构化状态；
- [x] 测试不访问网络、真实 Provider、ECS、Store 或 wall-clock sleep；
- [x] 记录 Armillae 边界、缺口与最终结论。

## 禁止提前冻结

Spike 可以定义测试专用 Agent/Context/Plan/Result/Budget/Runner/WorldGateway，但不得把这些类型直接
升级为 `loreloom-core`、`loreloom-agent` 或 `loreloom-runtime` 公共 API。精确 Schema、默认预算和
错误枚举必须在证据同步回 Active Spec 后建立。

## 自动化证据

测试文件：`crates/loreloom-agent/tests/agent_loop_spike.rs`。

| 测试 | 证据 |
|---|---|
| `armillae_multi_tool_calls_are_serial_and_canonically_correlated` | 两个 ToolCall 按返回顺序执行；未知 Tool 也生成同 call ID error result；第二次 Model Call history 为 assistant + 两个有序 ToolResult |
| `pending_model_call_is_dropped_when_runtime_cancel_wins_the_race` | wake-aware cancel 与 pending Bridge future 竞速，取消获胜后 future 被 drop，未执行 Tool |
| `narrator_plan_reprojects_each_npc_and_serializes_synthesis_after_commits` | PlayerInput 只给 Narrator；Mira 从 Revision 0 commit 后 Tomas 在 Revision 1 重投影；缺失 Actor stale 且不调用 Bridge；slot maximum 为 1；Synthesis 只取得 committed event |
| `synthesis_rejects_a_claim_that_has_no_committed_event_basis` | NPC 声称打开金库但没有 Tool/Event；Synthesis 引用 fabricated event 被拒绝，World 仍为 Revision 0 |
| `cancellation_keeps_committed_tools_and_correlates_every_accepted_request` | 第一 Tool commit 后取消；第二 Tool 与后续 NPC 不启动；三个已接受 Request 均有同 ID result |
| `configurable_turn_and_orchestration_budgets_fail_independently` | global/save/profile 数值取最小、strict-token policy 取 OR；单 Turn Tool cap 和整轮 Model cap 分别停止 |
| `token_output_and_fake_clock_limits_never_treat_unknown_usage_as_zero` | missing usage 严格模式停止；reported token 超限停止；fake monotonic clock 在第一 Tool 后阻止第二 Tool |
| `continuation_obeys_configured_round_limit_without_fixed_npc_count` | Continue plan 由可配置 max rounds 截止，不建立剧情 NPC 数量常量 |

验证命令：

```sh
cargo test -p loreloom-agent --test agent_loop_spike
cargo clippy -p loreloom-agent --all-targets -- -D warnings
```

结果：8 passed，Clippy 无警告。

## Armillae 边界与缺口

- `LlmBridge::complete` 正确保持“一次调用”；Loreloom 必须保存 canonical history 并显式决定下一次
  Model Call。Armillae 不执行 Tool，也不维护 Narrator/NPC 状态；
- `ToolExecutor::execute` 正确保持“一次 ToolCall -> 一次 Result/error”；Loreloom 负责未知 Tool
  error 到关联 ToolResult 的规范化以及 durable WorldGateway；
- Armillae Bridge trait 不接收 Loreloom cancellation token。Runtime 必须在自己的 async runtime 中
  race Bridge future 与 wake-aware cancellation，drop 失败/取消方并忽略迟到 Provider 结果；
- Provider `TokenUsage` 是 optional；Loreloom 需要 unknown accounting 和 strict policy，不能把缺失值
  当 0；
- free-form narration 无法进行完备事实证明。第一阶段增加 `supporting_events` provenance，并把 NPC
  文本标成 claim；文本永远不直接修改 ECS。

## 最终结论

Spike 通过。当前 Armillae public revision 足以承载 Loreloom 的显式 AgentRunner 与 Narrator 编排，
无需修改 Armillae。顺序/关联、current Revision 重投影、每 Request 一个 Result、claim/fact 分离、
两级可配置预算、取消竞速、Continue round 和非跨 Tool 原子性已同步回 Active Spec。测试专用 Rust
类型不升级为公共 API。
