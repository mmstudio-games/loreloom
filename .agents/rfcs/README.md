# Loreloom RFC 工作流

RFC 用于记录尚未确认、需要讨论或会改变架构边界的工程提案。持续约束实现的契约放在
`../specs/`；实施差异放在 `../todos/`；RFC 不代替 Spec 或实施清单。

## 状态

- `Draft`：正在探索或等待确认，不构成实现授权，也不得产生实施 TODO。
- `Accepted`：核心决策、范围、风险和验收标准已由用户确认；必须同步 Active Spec 后才能实施。
- `Active`：RFC 本身需要作为长期架构决策持续维护；具体可执行契约仍落入 Spec。
- `Rejected`：提案未采用，保留背景和原因。
- `Superseded`：提案被后续 RFC 替代，必须链接替代文档。

Spec 可在 RFC 讨论期以 `Proposed Spec` 形式形成可审查的精确契约，但不得作为实现依据。
RFC 接受后应复核 Proposed Spec，将确认内容转为 `Active Spec`，再建立 TODO。

## 编号与推进

RFC 文件使用四位递增编号和简短主题，例如
`0001-loreloom-architecture.md`。Draft 至少说明背景、目标、非目标、术语、候选决策、依赖
方向、数据所有权、失败与安全边界、取舍、验收场景和待决问题。

