# RFC 0003：动态角色外观与终端图片管线

> 状态：Accepted
> 接受日期：2026-09-03
> 决策方：项目方
> 影响范围：Core、Content、Appearance、World、Runtime、Store、TUI、应用装配与 Mod 资源
> 验证来源：[Spike 0007](../spikes/0007-tui-appearance.md)、[Spike 0008](../spikes/0008-tui-appearance-parity.md)

## 1. 背景

两个 P0 Spike 已验证透明 PNG 稳定排序、局部 alpha mask、动态染色、SourceOver/Multiply/Screen/
HardLight、同步关键帧、离屏合成、终端图片协议探测与 half-block 降级。DoL/Goose 资源只用于 Git
忽略目录中的本地人工对照；正式产品不能提交、下载或再分发这些第三方素材。

Spike 没有冻结资源格式、公共 API、存档字段、线程与缓存所有权。项目方于 2026-09-03 接受本 RFC
所述产品化方案，并确认资源目录直接使用 `appearance/`，不使用 `presentation/appearance`，整个
World/Mod 仍只分别使用 `world.toml`/`mod.toml`，不增加 `package.toml`。

## 2. 目标与非目标

目标：

- 让角色外观随持久角色选择、装备、Condition 和其它权威世界事实确定性变化；
- 让同一配方支持有序图层、sprite sheet 帧、mask、tint、opacity 和受限 blend mode；
- 在不阻塞 50ms TUI 事件循环的前提下，以终端原生图片协议显示缓存帧；
- 让根世界和声明了 Appearance capability 的 Mod 通过同一校验、哈希和锁定管线提供素材；
- 允许 Goose 式“全身 + 特写”作为普通画布美术布局，不建立专用领域协议。

非目标：

- DoL Mod 或 Canvas API 的二进制/Schema 兼容；
- 加载远程 URL、视频、任意脚本、自定义 shader 或 Mod 原生代码；
- 把第三方 DoL/Goose 图片纳入仓库、CI、fixture、默认 World 或发布包；
- 在第一版实现 DoL 的全部合成操作、渐变、图案或无限帧动画。

## 3. 模块和依赖决策

新增 `loreloom-appearance` library crate。它依赖 `loreloom-core`、Serde/TOML、SHA-256 与只启用 PNG
解码的 `image`，拥有 Appearance Pack wire、编译后 Catalog、受限解码、确定性 compositor 和
render key；不得依赖 Content、World、Runtime、Store、Ratatui 或 Crossterm。

`loreloom-content` 依赖 Appearance，继续拥有 World/Mod package 边界、Capability、资源路径、总量、
哈希和跨包引用校验，并把 package resources 交给 Appearance 编译。`loreloom-tui` 依赖 Appearance，
只消费 Runtime 发布的 `UiSnapshot` 和应用装配层提供的不可变 Catalog。TUI 不得借此查询 ECS 或
Content Registry。Runtime 不传 PNG 字节，不执行图片解码或终端编码。

## 4. 资源与 Mod 协议

每个根世界或 Mod 最多包含一个固定入口 `appearance/pack.toml`，图片只允许位于
`appearance/images/**.png`。根世界必须在 `world.toml.resources` 中显式列出入口和全部引用图片；
目录 Mod 仍由现有安全递归读取发现 payload。

提供 `appearance/` 的 Mod 必须在 `mod.toml.capabilities` 中声明 `appearance`；未声明 capability 的
包包含该目录，或声明后缺少 `appearance/pack.toml`，均拒绝完整包。`appearance/` 的原始 bytes
进入既有 ContentHash、WorldLock/ModLock，不增加独立 package manifest 或 lock。

Pack v1 包含：schema version、pack ID、一个或多个 Model；每个 Model 声明有限画布、typed 默认
Parameter、一个或多个带持续时间的 Frame，以及按声明次序稳定打破相同 z-index 的 Layer。Layer 可
引用同包 PNG source/mask、sprite frame、opacity、固定/Parameter tint、受限 predicate 和
SourceOver/Multiply/Screen/HardLight。

V1 限制：画布边长不超过 512、Model 不超过 64、每 Model Frame 不超过 16、每 Frame Layer 不超过
128、sprite frame 不超过 16；解码宽高和内存另受严格上限。路径必须是规范相对路径，禁止反斜杠、
空段、`.`、`..`、symlink、URL 和跨包隐式查找。

## 5. 权威状态与持久化

角色拥有可选 `CharacterAppearance`：稳定 `model_id` 与按稳定 Parameter Definition ID 排序的 tagged
值。第一版值只允许 RGB color、Definition variant 和 boolean。Character Definition/SpawnSpec 可以
提供初值，物化后进入 CharacterRecord/ECS；装备、耐久、Condition、姿态等已有事实不复制进自由
参数袋，后续由明确的绑定规则投影。

Loreloom 尚未发布首个版本，因此外观字段直接属于 Character record schema v1 的可选字段，不分配
v2，也不为开发期数据注册 migration。缺少 `appearance` 时按可选字段读取为 `None`；未知字段继续
拒绝，旧开发存档不形成兼容承诺。
存档永不保存 PNG/RGBA、合成缓存、终端协议、cell size、宿主探测结果或眨眼时钟。

## 6. Runtime 与 UiSnapshot

`CharacterContext` 增加可选 `AppearanceView`，包含角色当前 Revision、Model ID 和 typed Parameter。
它是权威状态的不可变展示投影，不包含资源路径、图片字节、Ratatui 类型或进程内句柄。

Appearance Catalog 根据内容 hash、Model、默认值与 View 参数生成确定性 render key，并解析有序
配方。TUI 只有在 render key 或目标 cell area 改变时才提交工作；普通 Snapshot 发布和 redraw 不
重复合成。

## 7. 动画、线程与缓存

世界语义上的睡眠、受伤、装备和表情必须来自 ECS/WorldCommand。普通眨眼/idle 是非持久展示时钟，
不得推进 World Clock/Revision 或进入 Agent Context；所有关键帧共享同一外观参数。

TUI 在进入 alternate screen 后、开始读取事件前探测一次协议。合成、缩放和协议编码在后台 worker
完成；draw callback 只选择并渲染 ready protocol。新请求合并为 last-write-wins，过期结果不得覆盖
新 Revision。缓存至少以 source hash、render key/frame、render key/frame/protocol/cell area 分层，
并采用有界容量；第一版可只保留当前角色的有限关键帧。

正式配置提供 `auto|kitty|iterm2|sixel|halfblocks|disabled`。Auto 探测失败明确降级；half-block 是兼容
路径而非目标画质。宿主终端（例如 HerdR）改变 stdio/capability 时允许用户显式覆盖，但强制不受支持
协议可能无输出。

## 8. 失败、安全与许可证

无效 Pack、重复 ID、未知 Parameter、缺失资源、错误尺寸、越界帧、mask 不匹配、解码超限和未知
blend mode 在 World/Mod 激活前整体失败。运行期合成失败只产生脱敏 UI notice 并显示文本占位，不
修改世界、不泄露路径或原始 payload。后台线程必须可确定性停止，不能阻塞终端恢复。

自动测试只使用程序生成或 Loreloom 原创/明确授权素材。DoL/Goose 本地适配继续位于 Git 忽略范围，
不得成为干净 checkout、CI 或发行运行的依赖。

## 9. 第一版验收

1. Pack 能表达肤色/发色/左右眼色、衣物完整/破损条件和 awake/blink 同步关键帧；
2. Character appearance 能从 v1 Definition 物化、按 v1 保存、重开并投影到 UiSnapshot；
3. 相同 key redraw 不合成，状态或内容变化失效，过期后台结果不覆盖新结果；
4. TUI 宽屏左栏显示原生图片，窄屏和 disabled/失败路径保持现有文本 UI 可用；
5. Kitty/iTerm2/Sixel/half-block 选择与显式覆盖可测，Rio 直连可人工 smoke test；
6. traversal、超限图片、缺失 mask、未知参数和 capability 不匹配均在加载期拒绝；
7. 格式、check、test、Clippy、依赖许可与干净 checkout 解析通过。
