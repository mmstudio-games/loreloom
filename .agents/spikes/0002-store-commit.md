# P0 Spike 0002：Store Commit 与恢复

> 状态：Completed
> 开始日期：2026-08-30
> 完成日期：2026-08-30
> 规范来源：[Runtime Active Spec §16.1](../specs/runtime.md#161-p0-spikes)

## 目标

验证 Loreloom durable unit、Revision CAS、ActionId 幂等、JSON、崩溃恢复和备份要求是否能由
公开发布的 SurrealDB/SurrealKV Toasty driver 满足，并以 SQLite 作为对照。

## 固定候选输入

- `toasty = 0.10`
- `toasty-driver-surreal`：公开 Git
  `https://github.com/noctisynth/toasty-driver-surreal`，revision
  `0a7c87408e0daae0d6f5ed9f2b9d1ebf01d08549`（package version 仍为
  `0.1.0-alpha.0`，AGPL-3.0-only）
- `toasty-driver-sqlite = 0.10.0`（registry，MIT）
- SurrealDB SDK `3.2.4` 与 SurrealKV 由公开 driver 的依赖图解析

不得使用本地路径或未发布提交。普通 Toasty batch 不视为原子事务；durable unit 必须显式使用
transaction handle。

crates.io 上同版本号的 alpha 包早于候选 revision，缺少显式事务、SurrealKV、原生 JSON 和
migration tracking，因此不能用于本 Spike。固定 revision 已与其公开 `origin/main` 对齐；本地
checkout 只用于核对，不是 Loreloom 的构建输入。

## 验收

- [x] Record、WorldEvent、Transcript、Action/Commit 和 Save Head 在显式事务中同成同败；
- [x] 从同一 expected Revision 竞争的两个提交恰好一个成功，另一方分类为 Conflict；
- [x] 重复 ActionId 不产生第二组记录或事件；
- [x] 强制错误与 rollback 不留下部分 durable unit；
- [x] SurrealKV handle drop 后经有界等待重开，只观察到完整 Revision；
- [x] JSON 嵌套对象、数组、大整数策略、未知字段、database empty 与 JSON null 有 round-trip 证据；
- [x] 一致备份可恢复，存档路径切换不会串扰数据；
- [x] 至少 10,000 条 Record 的提交/加载/checkpoint 指标被记录；
- [x] SQLite 运行相同的逻辑提交契约并记录差异；
- [x] 记录依赖来源、许可证、构建体积、故障注入与最终后端建议。

## 禁止提前冻结

Spike 使用测试专用模型，不构成长期 Store Schema。提交协议与后端只有在证据同步回 Active Spec
后才能冻结和进入 `loreloom-store` 公共 API。

## 自动化证据

测试文件：`crates/loreloom-store/tests/store_commit_spike.rs`。全部测试串行运行，避免多个
SurrealKV 文件引擎测试互相争用临时目录。

| 证据 | 结果 |
|---|---|
| 显式事务 durable unit | RecordOp、WorldEvent、Transcript、ActionCommit、Save Head 同事务提交 |
| 故障注入 | Record/Event/Transcript/Action/Head 五个阶段逐一 rollback，Head 保持 0 且无部分行 |
| ActionId | 重复提交返回 `AlreadyCommitted`，五类 durable row 数量均不增加 |
| Revision 竞争 | 两个 SurrealKV 连接同读 Revision 0 并写 Head；第一个 commit，第二个为 serialization failure |
| 崩溃恢复 | 子进程在提交前/事务中/commit 后直接 exit；重开只观察完整 Revision 0/0/1 |
| JSON | 嵌套对象、数组、Unicode、未知字段、`u64::MAX` 和 JSON null 无损；`Option::None` 保持 database empty |
| 文件恢复 | handle drop 后等待 100 ms 再复制 SurrealKV 目录，备份重开为完整 Revision 1；另一存档目录保持 Revision 0 |
| Migration | public Driver contract 的 migration ID apply/tracking 可读取 |
| SQLite | 显式 transaction commit/rollback 通过；原生 JSON 能力关闭，需使用规范化 JSON text |

验证命令：

```sh
cargo test -p loreloom-store --test store_commit_spike -- --test-threads=1
```

结果：11 passed（其中 crash parent 会启动三个预期以非零状态退出的子测试进程）。

## 规模与构建证据

SurrealKV 10,000 Record 测试在当前 macOS arm64、最新 stable、test profile 且移除 debuginfo 后的
单次结果：

| 操作 | 实测 |
|---|---:|
| 10,000 Record 显式事务提交 | 1,390 ms |
| handle drop、等待后重开 | 177 ms |
| 全量加载 10,000 Record | 193 ms |

Driver 不暴露手动 KV checkpoint；本 Spike 证明事务 commit 后经 handle drop 与有界等待可以重开，
但固定等待不能证明产品级确定性关闭。物理备份/切换仍需要可等待的 shutdown API；领域
Snapshot/compaction 作为后续 Store transaction。当前 Store Spike test executable 为约 161 MiB，
包含完整测试 harness、SurrealDB 与 SQLite；对应低调试信息 `target/debug` 缓存约 1.5 GiB。这些是
开发构建代理值，不是最终 release binary 门槛，产品纵向切片建立后必须重新测量 release 体积。

## 依赖、许可证与缺口

- `toasty 0.10.0`、`toasty-core 0.10.0` 和 `toasty-driver-sqlite 0.10.0` 来自 registry；
- SurrealDB driver 来自固定公开 Git revision，解析到 SurrealDB `3.2.4` 与 SurrealKV `0.21.4`；
- `toasty-driver-surreal` 为 `AGPL-3.0-only`，SQLite driver 为 MIT。第一阶段分发必须与 AGPL 兼容，
  或在发布前取得 driver 的兼容重新许可；
- crates.io 的同版本 alpha 不含本 Spike 所需能力，不可替换固定 Git revision；
- 顶层事务、read-your-writes、commit/rollback、native JSON、migration tracking 与 SurrealKV
  write conflict 均可用；nested transaction/savepoint、remote engine、live backup 和手动 KV
  checkpoint 不属于第一阶段依赖面；
- 普通 Toasty batch 对 KV backend 不自动形成所需原子性，Loreloom 必须始终显式开启事务。
- 当前 Toasty `Db`/Surreal driver 没有可等待的 close/shutdown；产品备份、恢复和存档切换在新 driver
  revision 提供该能力前保持门禁，不能把测试中的 100 ms sleep 复制到产品代码。

## ECS/Store 策略比较与结论

Spike 比较了两个可落地方向：

1. 先生成纯领域变更、Store commit 后再应用 ECS：崩溃窗口最小，但会在第一阶段把 World System
   拆成 prepare/apply 两套规则路径，且 apply 失败仍需 durable reload；
2. 在唯一世界槽内先执行 mutation-oriented Armillae System，屏蔽 candidate Observation，Store
   commit 后才发布：复用单一规则实现；已知失败或不确定结果时丢弃 World 并从 Store 重建。

第一阶段选择第 2 项。它不依赖任意 Component/Resource 可克隆，也不要求 Armillae 提供 ECS
transaction。Action ToolResult、Transcript committed 状态与 UiSnapshot 必须晚于 durable commit；
Store 失败到重建完成期间 Runtime 禁止新的世界观察和行动。

最终建议：SurrealDB + SurrealKV 在技术上符合 Loreloom 的单机嵌入式存档场景，并固定为第一阶段
后端；SQLite 仅保留为逻辑提交契约对照。测试模型不构成生产 Store Schema。
