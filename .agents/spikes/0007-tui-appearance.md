# P0 Spike 0007：动态纸娃娃与终端图片

> 状态：Completed
> 开始日期：2026-09-02
> 完成日期：2026-09-02
> 规范来源：项目方授权的范围化视觉 Spike；不冻结产品 API、Mod Schema 或持久化格式

## 目标

验证 Loreloom 可以把角色外观表达为有序透明图层，在内存中按外观变化重新合成，并通过终端原生
图片协议或 Unicode half-block 降级渲染，同时保持现有 Ratatui/Crossterm 事件循环和确定性测试边界。

## 候选输入

- `image` 只作为 `loreloom-tui` 的开发依赖，用于 PNG 解码、RGBA alpha composition、缩放和
  视觉验收图输出；
- `ratatui-image` 只作为开发依赖，候选支持 Kitty、iTerm2、Sixel 与 half-block fallback；
- 本地视觉验证可使用 DoL/DoL Plus 社区图片包，但素材、解包结果和派生图片只放在
  `.local/appearance-spike/`，不得提交、进入 CI、测试 fixture、发布包或 Loreloom Mod；
- 自动化测试必须使用程序生成的小型 RGBA 图层，不依赖外部素材、网络或真实 TTY。

## 候选模型

Spike 只定义测试/示例内部的 `LayerRecipe`、compositor、cache key 和 terminal preview，不增加
`UiSnapshot` 字段。输入是按 `z_index` 稳定排序、画布尺寸一致的 RGBA 图层；外观 revision 或图层
集合变化才重新合成，普通 TUI redraw 复用缓存。

正式设计的候选方向是 Mod 根目录可选 `appearance/`，但本 Spike 不创建或接受
`appearance/pack.toml` Schema，不扩展 `mod.toml` capability，也不决定 Runtime、Content 与 TUI
之间最终由哪一种公共 wire 传递外观配方。

## 验收

- [x] 多个透明 PNG 图层按稳定 z-order 正确 alpha composition；
- [x] 衣物/状态图层变化产生不同合成结果，相同 cache key 不重复合成；
- [x] half-block 路径能在 `TestBackend` 中确定性渲染并输出可视验收图；
- [x] 支持真实 TTY 时可自动探测 Kitty/iTerm2/Sixel，否则降级为 half-block；
- [x] resize/编码不进入 Loreloom 产品 renderer，本 Spike 不阻塞现有 50ms 事件循环；
- [x] 外部素材缺失时给出明确本地操作提示，自动化测试仍独立通过；
- [x] 完成格式、check、test 与 Clippy 验证，并记录依赖、性能、终端差异和最终结论。

## 非目标

- 不冻结 Appearance Pack 或 Mod 资源格式；
- 不接入 ECS、Runtime、`UiSnapshot`、正式 TUI 布局或存档；
- 不提交、再分发或自动下载第三方美术资源；
- 不验证高帧率动画、远程图片、任意 URL、视频或 GPU 合成。

## 实现与本地视觉证据

Spike 实现在 `crates/loreloom-tui/examples/appearance_spike.rs`，没有进入 library module 或产品
renderer。它包含：

- DoL sidebar 双帧图层适配：从 256/512 x 256 PNG 中选择指定 256 x 256 frame；
- 稳定 `z_index` 排序、RGBA alpha composition、简单 mask tint、透明边界裁剪和 nearest-neighbor
  pixel-art 放大；
- calm/intact 的 18 层配方，以及 tired/damaged 的 20 层配方；后者替换嘴型、上衣/短裤完整度并
  增加 blush/tear 图层；
- 旧 DoL Plus 包优先使用其预切的左右虹膜图，避免把只适合完整 Canvas filter/cutout 流水线的
  整幅虹膜图交给 Spike 的简化 tint 后产生大色块；
- `(source, appearance state, ordered recipe)` cache key；相同 key 不重复合成，变化后只重建一次；
- `Picker::from_query_stdio()` 真实 TTY 协议探测，以及确定性的 `Picker::halfblocks()`/`TestBackend`
  路径；
- 将实际 Ratatui half-block Buffer 栅格化成 PNG，便于无需目标终端也能直接审查降级效果。

本地视觉样本使用 DoL Plus Release 65 的 BEEESSS 0.5.4.9 包，只解包在被 Git 忽略的
`.local/appearance-spike/`。素材、原 HTML、解包结果和四张派生 PNG 均未进入 Git。生成结果为：

| 状态 | 图层 | 裁剪后合成图 | half-block 区域 | 非空字符格 |
|---|---:|---:|---:|---:|
| calm / intact | 18 | 75 x 185 px | 15 x 19 cells | 85 |
| tired / damaged | 20 | 75 x 185 px | 15 x 19 cells | 88 |

在当前 PTY 中运行 `--terminal --state calm` 时，协议查询安全降级为 `Halfblocks`，按键退出为 0，
raw mode、alternate screen、mouse、paste 和 cursor 均由既有 `TerminalSession` 逆序恢复。当前环境
没有可截图的 Kitty/iTerm2/Sixel 终端，因此三个原生协议只验证了 picker/encoding 代码路径与依赖
兼容性，没有声称完成对应终端的人工视觉 smoke test。

## 性能与视觉结论

debug binary 在本机冷读取 36 个社区 PNG 图层、生成两张合成图、两次 half-block 编码并写出四张
PNG 的总墙钟时间为 0.76 秒。这个数字包含磁盘解码和验收文件 I/O，不代表单次缓存命中；自动化测试
确认相同 cache key 的 redraw 不调用 compositor。正式产品若允许运行时 resize/重编码，仍应放在
后台线程，不能搬进当前 50ms draw callback。

合成图在视觉上正确保留了发型前后层、身体、脸、手臂、上衣袖子、下装、表情和受损状态变化。
half-block 在 15 x 19 cells 内能辨认人物轮廓、发色和完整/受损服装差异，但脸部和像素画线条明显
失真；这来自只有约 15 x 38 个独立色块的表示上限。它适合作为兼容 fallback，不应成为 Loreloom
动态头像的目标画质。Kitty/iTerm2/Sixel 原生像素协议才是保留 DoL 风格细节的主路径。

Spike 的 tint 只用于快速视觉验证，没有复制 DoL 的左右眼 mask、复杂 blend mode、服装 pattern、
腹部/帽子遮罩或完整动画规则；这些能力属于未来 `appearance/` Schema 与 compositor RFC，而不是
从 DoL 历史实现反向冻结的协议。

## 自动化验证

```sh
cargo fmt --all -- --check
cargo check -p loreloom-tui --all-targets
cargo test -p loreloom-tui
cargo clippy -p loreloom-tui --all-targets -- -D warnings
cargo deny check licenses bans sources
```

结果：23 tests passed，check 与 Clippy 无警告；依赖 bans、licenses 与 sources 均通过（仓库既有
重复版本和 SurrealDB 缺少 license 字段保留 warning）。

## 最终结论

Spike 通过。动态纸娃娃可在 Loreloom 中离屏合成并交给 Ratatui 图片 widget；外观变化可由稳定
recipe/cache key 增量触发，终端图片能力可以渐进增强并保留确定性 half-block fallback。该结论不
接受正式 `appearance/pack.toml`、`UiSnapshot` wire、Mod capability、资源限额、缓存所有权或动画
时序，产品化前仍须按设计流程独立冻结。
