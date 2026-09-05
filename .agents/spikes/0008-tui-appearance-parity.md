# P0 Spike 0008：DoL 外观语义与 Goose 式双视图

> 状态：Completed
> 开始日期：2026-09-03
> 完成日期：2026-09-03
> 规范来源：项目方授权的范围化视觉 Spike；不冻结产品 API、Mod Schema、依赖或持久化格式

## 目标

在不提交或运行第三方美术资源的前提下，用原创、确定性生成的像素图层验证两件事：

1. DoL Canvas renderer 中对动态纸娃娃最关键的局部 mask、动态染色、代表性混合模式和同步动画帧
   可以在 Rust 离屏 RGBA compositor 中表达；
2. Goose 的“同一帧同时包含小比例全身与大比例特写”可以作为普通分层美术布局工作，并在两种
   视图中同步眼色、衣色、表情和眨眼。

## 边界

- 只修改 `loreloom-tui` 的开发期 example，并继续使用 dev-dependency；
- 不接入 ECS、Runtime、`UiSnapshot` 或正式 TUI；
- 不创建 `appearance/` Schema、公共 Rust API、Mod capability 或缓存/线程产品契约；
- 自动化验收、测试 fixture、CI 和提交内容不加载 Goose/DoL 第三方图片；经项目方明确要求后，
  example 另提供显式 `--goose` 本地演示入口，只读取用户已放在 Git 忽略目录中的素材，不自动下载、
  不提交、不进入发布包；
- 只验证足以暴露语义与终端表现风险的代表性子集，不宣称完成 DoL 全部领域模型或美术覆盖。

## 验收

- [x] 左右虹膜通过独立 mask 染成不同颜色，mask 不得覆盖眼白、眼线或脸部；
- [x] 睁眼与闭眼帧在全身、特写两种视图中同步；
- [x] SourceOver、Multiply、Screen、HardLight 的确定性像素测试通过；
- [x] 两种外观状态产生可直接审查的 PNG，且包含全身与特写；
- [x] half-block 路径仍可确定性输出，但明确只作为兼容降级；
- [x] format、check、test 与 Clippy 通过。

## 实现与视觉结果

实现继续位于 `crates/loreloom-tui/examples/appearance_spike.rs`，通过 `--original` 启用完全由代码
生成的原创素材。它先在 128 x 128 逻辑画布上绘制 12 个透明图层，再使用 nearest-neighbor 放大到
256 x 256；每个图层都同时包含左侧小比例全身与右侧特写，因此同一外观配方天然同步两个视图。

新增的代表性 compositor 支持稳定 z-order、图层 opacity、局部 alpha mask、颜色 tint，以及
SourceOver、Multiply、Screen、HardLight。左右虹膜从同一灰度源层生成，分别经过独立 mask 和颜色
参数；睁眼与闭眼由同一动画状态同时控制两个视图。终端模式预先合成睁眼和闭眼关键帧，以 950 ms
睁眼、150 ms 闭眼的周期切换，draw loop 每 50 ms 保持输入响应。

视觉输出位于 Git 忽略的 `.local/appearance-spike/output/`：

- `original-review.png`：800 x 272，依次展示睁眼、闭眼和替换整套发色/衣色/左右眼色；
- `original-animation-strip.png`：512 x 256 的睁眼/闭眼双帧；
- `original-halfblocks.png`：42 x 21 cells、269 个着色字符格的降级预览；
- 独立的 `original-awake.png`、`original-blink.png` 和 `original-alt-palette.png`。

当前 PTY 自动降级到 Halfblocks，交互 smoke test 能持续切换 awake/blink，并在按键后正常恢复终端。
静态视觉输出包含全身和特写，异色瞳没有覆盖眼白或脸部；half-block 可以辨认人物与眨眼，但仍明显
模糊，继续只适合作为 fallback。一次 debug 运行合成三种状态、编码 half-block 并写入全部验收 PNG
约 407 ms，该数字包含 PNG I/O，不代表缓存后动画成本。

旧的本地 DoL Plus 适配也随本 Spike 修正：其版本同时提供预切 `iris_left.png`/`iris_right.png`，应
直接使用，不能把整幅虹膜资源交给仅有简单 tint 的 0007 路径；修正后此前的大块眼色伪影消失。

## 自动化验证

```sh
cargo fmt --all -- --check
cargo check -p loreloom-tui --all-targets
cargo test -p loreloom-tui --all-targets
cargo clippy -p loreloom-tui --all-targets -- -D warnings
```

结果：29 tests passed，check 与 Clippy 无警告。

## 结论与限制

Spike 通过。Goose 式双视图不需要专用渲染算法：它是同一组图层内同时绘制两个尺度的美术布局；
mask、染色、混合与同步关键帧负责保持动态外观一致。Loreloom 能在终端路径复现这套能力边界。

本 Spike 只实现并验证代表性 Canvas 语义，不是 DoL 全部 29 种混合/合成操作、渐变、图案、多 mask、
offset、完整动画属性或三级缓存的实现，也不代表已达到 Goose 的美术精细度。产品化前仍需单独冻结
资源 Schema、compositor 所有权和依赖、工作线程、动画时钟、缓存上限与 `UiSnapshot` wire，并用
浏览器 Canvas golden image 覆盖完整像素一致性矩阵。本次没有引入 `tiny-skia` 或其它新依赖。

## 本地 Goose 实素材演示

项目方在查看原创程序化占位图后明确要求直接使用 DoL/Goose 素材演示。example 因此新增显式
`--goose` 模式和可选 `--goose-source`，默认只查找 `.local/appearance-spike/source/Goose/` 下已经
解包的 goosefem imagepack；目录继续由现有 `.gitignore` 覆盖，程序不会下载或复制素材。

本地配方使用 18 层 Goose 图片，包含身体、左右手臂、左右虹膜、眼白、眼睑、睫毛、嘴型、头发前后
层、上衣袖子、上衣和下装。512 x 256 图层跟随 animation frame 选择左右两个 256 x 256 帧，静态
256 x 256 图层保持 frame 0。输出 `goose-review.png` 依次展示正常、眨眼和换色/破损服装，实际画面
确认全身与特写同步且遮挡正确；`goose-halfblocks.png` 在 52 x 26 cells 中仍明显模糊。

当前 PTY 的交互演示自动选择 Halfblocks；`--goose --terminal` 已验证 awake/blink 周期切换、输入响应
和终端恢复。一次 debug 静态演示读取并合成三种状态、编码 half-block 和写入 PNG 约 834 ms。Goose
素材自身的使用限制不因本地 Spike 改变：这些图片和派生视觉证据不得提交、进入 CI 或随 Loreloom
分发，正式资源必须原创或另行取得授权。

人工验收发现 `ratatui-image` 在终端 capability 查询拿不到像素字号时会回到 Halfblocks，即使调用者
知道当前终端支持原生图片。example 因此增加 `--protocol auto|kitty|iterm2|sixel|halfblocks`；显式值
在查询后覆盖 picker 的协议类型。界面检测到 Halfblocks 时会直接显示低画质提示。该参数只用于
Spike 诊断，不冻结正式配置或 capability policy；用户强制终端不支持的协议时可能看到空白或转义码。
