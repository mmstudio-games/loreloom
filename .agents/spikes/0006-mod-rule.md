# P0 Spike 0006：Mod Package 与声明式 Rule

> 状态：Completed
> 开始日期：2026-08-30
> 完成日期：2026-08-30
> 规范来源：[Runtime Active Spec §6.5](../specs/runtime.md#65-模组参数事件与声明式规则)、
> [§10.4](../specs/runtime.md#104-mod-加载冲突与扩展边界)

## 目标

验证内置内容与显式配置的外部目录模组可以通过同一受限加载管线，形成可锁定、可恢复的 Definition/
Rule Registry；数据模组能够声明类型化 Parameter、Event Option、Gameplay Action 与白名单 Rule，
但不能加载本机代码、注册 Tool/System、访问包外路径或越过执行预算。

## 固定候选格式

第一阶段候选只支持目录包，不支持 zip/tar、Rust/C 动态库、WASM 或脚本：

```text
mod.toml
content/*.json
rules/*.json
locales/*.json       # 可选、只作展示数据
assets/**            # 可选、有界 opaque resources
```

- `mod.toml` 使用 UTF-8 TOML，声明 schema version、Mod ID、SemVer version、Engine/Content Schema
  compatibility、required/optional dependencies、capabilities、payload hash 与显式 Patch；
- JSON 数据必须是拥有所有权、tagged、版本化且拒绝未知控制字段的 Schema；
- payload hash 为规范化 manifest（清空 hash 字段）与按相对路径 byte-order 排序后的所有 payload
  文件的 SHA-256；路径和原始 bytes 都进入摘要；
- 所有路径使用 `/` 相对路径，拒绝 absolute、`.`、`..`、空 segment、反斜杠、NUL 和 symlink；
- Host 配置提供不可被 manifest 扩大的文件数、单文件、总 bytes、深度与规则预算。

候选默认资源上限与最终字段只有在 Spike 通过并回写 Active Spec 后冻结。

## 验收

- [x] 内置与外部 Virtual/Directory source 进入同一 manifest/path/hash/schema pipeline；
- [x] path traversal、absolute/backslash/NUL、symlink、文件数量/大小/总量/深度越界在解析前拒绝；
- [x] SHA-256 对 manifest、路径和 bytes 敏感，hash mismatch 不发布包；
- [x] required/optional dependency、SemVer requirement、缺失、不兼容和 cycle 得到确定性拓扑结果；
- [x] duplicate Definition 默认失败；显式 Patch 仅在 target mod/definition/version 精确匹配时按
  依赖拓扑与 Patch ID 顺序应用；
- [x] ModLock 保存 Mod ID/version/hash、解析后的依赖、Patch 与 Schema 版本，精确重开；
- [x] Parameter Bool/Fixed/Counter/Enum/TagSet/ObjectRef 按 Definition 校验，不能退化为任意 JSON bag；
- [x] Event Option 使用 Instance ID + Option ID + expected Revision，在 current Revision 重新校验
  visible/enabled，并把 Effect + node advance 作为单一领域结果；
- [x] Gameplay Action 只引用已编译的参数 Schema/Effect plan，不注册动态 Tool Handler；
- [x] Rule Trigger/Predicate/Effect 白名单、规范排序、静态 cycle 检测、节点/effect/级联预算和可信
  initiator/provenance 有确定性测试；
- [x] 数据包无法声明 Provider/Secret/network/Shell/native/WASM/script/Tool/System capability；
- [x] 失败加载不改变已发布 Registry/ModLock，测试不访问网络、真实 Store 或 Provider；
- [x] 记录物理格式、默认资源限制、声明式执行边界与最终结论。

## 禁止提前冻结

Spike 使用 virtual package 与测试专用 Schema；它可以冻结第一阶段目录布局、Manifest/ModLock 逻辑
字段和 Rule 边界，但 Stable ID 编码、Fixed 数值、各领域 Definition 的完整长期格式仍受各自门禁。

## 自动化证据

测试文件：`crates/loreloom-content/tests/mod_rule_spike.rs`。

| 测试 | 证据 |
|---|---|
| `builtin_and_directory_packages_share_path_hash_and_resource_checks` | builtin/directory 得到相同 manifest/hash/files；拒绝 `..`、absolute、backslash、`.`、空 segment、NUL、symlink、file/single/total/depth/manifest limit 和篡改 bytes |
| `dependencies_have_deterministic_semver_order_cycles_and_exact_lock` | 输入逆序仍按 dependency -> lexical Mod ID；required/optional、missing、incompatible、cycle；exact lock reopen，版本变化不替换 published state |
| `duplicate_definitions_fail_and_explicit_versioned_patch_is_ordered` | 普通同 ID Definition 失败；direct dependency + exact target mod/version/definition 的显式 Patch 成功并进入 lock |
| `all_parameter_variants_are_typed_and_object_refs_are_world_checked` | Bool/Fixed/Counter/Enum/TagSet/ObjectRef default 校验；Fixed scale mismatch、missing ObjectRef 和 untyped JSON 拒绝 |
| `event_option_rechecks_revision_and_commits_effect_with_node_atomically` | stale expected Revision 不变；current Revision 同时调整 Parameter、推进 node 和 Revision |
| `gameplay_action_uses_generic_entry_and_typed_effect_plan` | Action ID 不成为 Tool 名；参数越界不变；generic action entry 执行已编译 Effect |
| `rules_are_ordered_provenanced_cycle_checked_and_budgeted_atomically` | `priority -> rule ID` 顺序、system principal/source event provenance、budget failure 全回滚、静态 event cycle 拒绝 |
| `manifest_rejects_unknown_native_or_service_capabilities` | TOML unknown shell capability 与 JSON `tool_handler` 控制字段在 Schema 层拒绝 |

验证命令：

```sh
cargo test -p loreloom-content --test mod_rule_spike
cargo clippy -p loreloom-content --all-targets -- -D warnings
```

结果：8 passed，Clippy 无警告。

## 依赖、默认限制与边界

- Spike 使用 registry `semver 1`、`sha2 0.10`、`toml 1`、Serde/JSON；不使用本地路径；
- 默认 package limit：256 files、1 MiB/file、16 MiB total、depth 8、256 KiB manifest；
- 默认 Rule limit：64 predicates/rule、32 effects/rule、128 triggered rules、1,024 evaluated
  predicates、512 applied effects、cascade depth 8；
- 第一阶段只支持目录包；archive、signature、WASM/native/script 和热加载不在此协议；
- SHA-256 是完整性与 reproducibility，不证明发布者身份；外部目录是否可信仍是用户配置；
- 测试使用 virtual file source 验证纯 pipeline。产品 filesystem adapter 仍必须用防 symlink race 的
  打开方式做真实目录 smoke test，但不能改变上述路径/资源语义。

## 最终结论

Spike 通过。目录布局、Manifest/ModLock 逻辑字段、hash、依赖/Patch、Parameter/Event/Gameplay
Action、Rule 白名单/预算和失败原子性已同步回 Active Spec。各领域完整 Definition JSON 与 Fixed/
Stable ID 底层编码仍保持独立门禁，测试类型不升级为公共 API。
