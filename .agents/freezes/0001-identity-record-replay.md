# Freeze 0001：Identity、Record 与 Replay

> 状态：Frozen
> 日期：2026-08-30
> 权威契约：[Runtime Active Spec](../specs/runtime.md)

## 范围

本记录解除 Stable ID、record envelope、领域 migration 和重建事实源的实施门禁。它不冻结具体
Character/Item/Skill payload；这些 Schema 在后续领域冻结中完成。

## 已冻结决定

- 运行 ID 是带三字母类型前缀的 canonical lowercase UUIDv7；ActorId 复用 ObjectId wire identity。
- ModId 是 bounded reverse-DNS lowercase ID；Definition ID 是
  `mod-id:kind/local-key`，版本/hash 由 ModLock 固定。
- Envelope 拒绝未知控制字段；当前 payload codec 拒绝未知字段和 float，数据库 `NONE` 不能冒充
  JSON value。
- 旧版本只通过连续、纯确定性 migration 链升级；未知 type、新版本、缺口和 downgrade 明确失败。
- checkpoint + ordered RecordOp 是唯一状态重建事实源；Command 用于输入/幂等，Event 用于已发生
  事实/provenance，两者均不在 Load 时重新执行。

## 实施证据

`loreloom-core` 必须覆盖每类 ID 的生成/解析/Serde、错误前缀与非 canonical UUID、Definition ID
边界、Revision overflow、Envelope unknown/null/float 约束、migration gap 和 RecordOp 顺序验证。
Store/World 后续阶段必须覆盖 checkpoint + ops 重建与 ECS record round-trip。
