# Loreloom Agent 协作约定

本文件适用于 Loreloom 仓库中的全部目录和任务。

## 1. 权威文档与阶段门禁

- 设计入口：[.agents/DESIGN.md](.agents/DESIGN.md)
- 架构提案：[.agents/rfcs/0001-loreloom-architecture.md](.agents/rfcs/0001-loreloom-architecture.md)
- 候选规范：[.agents/specs/runtime.md](.agents/specs/runtime.md)
- RFC 工作流：[.agents/rfcs/README.md](.agents/rfcs/README.md)
- 实施状态入口：[.agents/TODO.md](.agents/TODO.md)

`.agents/DESIGN.md` 只负责生态分层、权威文档路由和跨子系统边界；RFC 负责尚未确认的架构
决定；Spec 负责接受后持续约束实现的契约；TODO 只记录已确认设计与实现之间的差异。

当前 RFC 状态为 Draft，Runtime Spec 状态为 Proposed。用户明确接受 RFC、Spec 转为 Active
并建立实施清单前，不得创建 Cargo workspace、产品 crate、公共 Rust API、持久化格式或实现
代码。讨论、审查和文档修改不等同于实现授权。

## 2. 设计与同步顺序

发生新的跨层责任、公共协议、持久化语义、Tool 副作用、Agent 调度、依赖选择、安全边界或
范围变化时，按以下顺序处理：

1. 更新 `.agents/DESIGN.md` 中受影响的分层与责任；
2. 未确认的决定先更新 Draft RFC；
3. 用户接受后更新 Active Spec；
4. 建立或更新对应实施清单；
5. 最后修改代码、配置、测试、示例和用户文档。

不得把重要设计决定只隐藏在代码或测试中，也不得从 Proposed Spec 推断实现授权。

## 3. 核心架构边界

- ECS 工作世界是运行中游戏事实的权威数据面；影响未来模拟或 Agent 决策的状态不得只存在于
  Prompt、聊天历史、模型隐藏状态或 TUI View Model。
- 持久化必须保存拥有所有权、带版本且使用稳定逻辑 ID 的领域数据；不得把 Bevy `Entity`、
  `World` 内存布局或 Schedule 内部状态当作长期格式。
- LLM 只能通过 Loreloom Runtime 注册并授权的 Tool 查询或请求修改世界；模型文本本身不能
  直接修改 ECS。
- Loreloom 负责 Agent Loop、上下文投影、Tool 策略、世界命令、持久化提交与 UI；不得要求
  Armillae `LlmBridge` 自动执行 Tool 或维护 Memory。
- `LlmBridge` 仍只负责一次 Model Call；`ToolExecutor` 仍只负责一次 `ToolCall -> ToolResult`；
  是否继续下一次 Model Call 由 Loreloom Runtime 显式决定。
- UI 和 LLM 调用不得长期持有 ECS 可变访问；它们只消费不可变 Observation 或 UiSnapshot。

## 4. Rust 与依赖

- 使用最新 stable Rust 与 Rust 2024 edition。
- Loreloom 不设置 MSRV，不在 Cargo manifest 中声明 `rust-version`，也不为旧编译器维护兼容
  分支。CI 只验证执行时的最新 stable。
- Armillae 或其它依赖自身声明的最低编译器要求仍必须满足，但不得复制为 Loreloom 的兼容承诺。
- 除非用户明确要求直接编辑，否则 Cargo manifest 的 crate、workspace member 和依赖变更使用
  `cargo new`、`cargo add`、`cargo remove` 等 Cargo CLI 完成，并在执行后检查 manifest 与
  lockfile。
- 提交的依赖必须能从 Loreloom 的干净 checkout 解析：使用 registry 版本，或公开 Git URL 与
  明确的 tag/revision。项目文档、manifest、lockfile、示例和测试不得依赖未提交的外部状态。
- Bevy 依赖版本必须与 `armillae-simulate-bevy` 的兼容发布线一致，不能由 Loreloom 单独升级。
- 生产代码不得对可恢复错误使用 `unwrap()`；外部输入、存档、Provider、Tool、I/O 和配置失败
  必须转换为结构化错误。

## 5. Secret、内容与日志

- Provider Secret 不得进入存档、ECS Component、`Debug`、错误文本、日志、Fixture 或快照。
- 默认不得记录完整 Prompt、玩家输入、模型响应、Tool 参数或 ToolResult。
- 需要诊断时使用稳定 ID、阶段、耗时、结果分类和经过脱敏的摘要。
- 外部副作用 Tool 默认禁用；启用时必须由独立能力策略显式授权。

## 6. 验证

实现阶段的变更至少执行与范围相称的格式检查、`cargo check`、测试和 Clippy。世界协议与
持久化变化必须覆盖 round-trip、版本迁移、稳定 ID、重建、崩溃恢复和未知字段策略；Agent
流程必须使用 Mock Bridge 覆盖 Tool 顺序、预算、取消和错误恢复；TUI 使用确定性 View Model
与快照测试，不在渲染测试中调用真实模型。

## 7. 交付

- 每次交付必须说明实际修改、验证、未决事项，以及技术方案相对任务开始时发生的变化；无变化
  时明确写出“本次技术方案相对既有设计无变动”。
