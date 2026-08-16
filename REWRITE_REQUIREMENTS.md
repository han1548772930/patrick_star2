# Patrick Star 2 独立重写要求

> 状态：执行基线  
> 日期：2026-08-16  
> 旧项目：`E:\rust_test\patrick_star`  
> 新项目：`E:\rust_test\patrick_star2`

## 1. 决策

`patrick_star2` 是独立重写，不再对 `patrick_star` 做渐进式渲染迁移。旧项目只承担三种角色：功能规格、交互对照和性能基线。

新项目不得：

- 依赖旧项目的任何 crate；
- 调用、转发或适配旧项目的 Direct2D/`windows-canvas` 绘制接口；
- 为了保留旧结构而建立没有实际职责的包装层；
- 先实现 Direct2D 后端再切换 OpenGL；
- 用“平台暂不支持”掩盖尚未实现的公共功能。

允许参考旧项目的行为、测试场景和算法，但实现必须围绕新的数据模型、渲染路径和平台边界重新设计。源码目录和依赖方向从第一次实现就固定，不把 Win32 代码写进公共模块后再搬迁。项目确定使用一个 Cargo package 和下述固定源码目录；这不是临时结构。模块通过 Rust 可见性和 trait 控制依赖，不用多个 crate 制造额外包装。

## 2. 固定技术方案

```text
全屏截图和滚动截图预览
  原生平台窗口 + 原生 OpenGL context
  OpenGL：截图纹理、FBO、像素效果、最终合成
  FemtoVG：选区、控制点、矢量标注、文字、工具栏

可编辑预览
  Slint：窗口、标题栏、工具栏、面板和普通控件
  OpenGL + FemtoVG：可编辑图像画布

设置和业务窗口
  Slint

图像算法
  纯 Rust 状态/几何 + OpenCV（ORB、图像处理）

平台能力
  小型能力接口 + Windows/macOS/Linux 原生实现
```

全屏捕获窗口不依赖 Slint，也不要求 `winit`。Windows 使用 Win32 窗口和 WGL；macOS 使用 AppKit 平台窗口；Linux 分别实现 X11 和 Wayland 路径。OpenGL 是公共渲染 API，但窗口、OpenGL context、抓屏、权限和系统集成仍由平台实现。

FemtoVG 不是另一套窗口或 GPU 后端。它运行在 OpenGL 上，只负责不值得手写三角化、抗锯齿和字形缓存的 2D 内容。大尺寸截图不经过 FemtoVG 图片 API反复上传，而由 OpenGL 静态纹理直接绘制。

OpenCV 不承担交互渲染。CUDA/OpenCL 仅在具体算法基准证明有收益且不破坏硬件兼容性时启用。

## 3. 功能完整性

下列功能全部是重写验收范围，不得缺失。

| 领域 | 必须保留的行为 |
|---|---|
| 启动与系统集成 | 托盘、全局快捷键、设置持久化、开机启动策略、单实例行为 |
| 桌面捕获 | 多显示器、负坐标、混合 DPI、光标位置、全屏冻结画面、捕获窗口排除 |
| 自动选择 | 窗口和控件识别、鼠标悬停高亮、手动拖选接管 |
| 选区交互 | 新建、移动、八方向缩放、最小尺寸、边界约束、全选、键盘微调、取消 |
| 辅助显示 | 放大镜、像素颜色、尺寸提示、选区外遮罩 |
| 标注 | 选择、矩形、圆形、箭头、画笔、马赛克、文字、表情；颜色、线宽、填充、字号和马赛克尺寸 |
| 编辑 | 命中测试、悬停、选中框、控制点缩放、移动、层级、文本插入/选择/光标、撤销和重做 |
| 工具栏 | 主工具栏、上下文选项、禁用态、悬停态、DPI 布局、屏幕边缘避让 |
| 输出 | 保存、复制图片、贴图、OCR、正确裁剪和标注合成、格式选择 |
| 滚动截图 | 滚轮捕获、帧去重、OpenCV ORB 匹配、拼接、实时预览、结束后继续编辑 |
| 可编辑预览 | 缩放、适应窗口、旋转、平移、标注编辑、自定义标题栏、保存和复制 |
| 平台覆盖 | Windows、macOS、Linux X11；Wayland 按能力报告并提供 Portal/PipeWire 工作流 |

Wayland 不能静默伪装成与 Win32 相同的能力。协议不允许的窗口枚举、任意定位或全局输入必须通过明确的 capability 返回值告诉上层；上层提供可用的替代工作流，而不是让按钮无响应。

## 4. 数据和渲染原则

- 原始截图在一次捕获会话中只上传 GPU 一次；尺寸或内容未变时不得重新创建纹理。
- 标注采用非破坏对象模型。拖动和缩放只更新对象参数，不复制整张位图。
- 马赛克优先由 OpenGL shader/FBO 实时合成；CPU/OpenCV 路径只能更新脏区。
- 字体、字形、图标、shader、VAO/VBO、FBO 和图片句柄跨帧复用。
- 默认按需重绘。只有动画、拖动或新捕获帧到达时才请求下一帧。
- 交互路径禁止 `glReadPixels`；仅在保存、复制或算法确需 CPU 像素时读回。
- GPU 资源创建、整图上传、图像编码、OCR 和 ORB 匹配不得发生在高频鼠标事件中。
- 逻辑坐标与物理像素必须显式转换，并覆盖负桌面原点与混合 DPI。
- 导出与屏幕预览使用同一份文档模型，但可以使用不同渲染目标；不得维护两套标注语义。

## 5. 代码边界

从项目开始就固定这些源码目录，不采用“先写在一起，后面再拆”的方式：

```text
src/app/          生命周期、命令调度、后台任务
src/model/        帧、几何、选区、标注、编辑历史、设置
src/rendering/    OpenGL 纹理/FBO 与 FemtoVG 矢量绘制
src/platform/     原生能力 trait 和平台选择
  windows/        Win32、WGL、Windows Graphics Capture/GDI、Shell
  macos/          AppKit、ScreenCaptureKit/CoreGraphics
  linux/x11/      X11/XCB、EWMH、GLX/EGL
  linux/wayland/  Portal、PipeWire、Wayland/EGL
src/ui/           Slint 预览、设置和普通窗口
src/scroll/       帧匹配与拼接
src/ocr/          OCR 任务和结果
```

`platform` 不是一个只做名称转发的模块。公共层按能力定义小 trait，例如 `DesktopCapture`、`CaptureOverlay`、`WindowLocator`、`Clipboard`、`GlobalShortcut` 和 `TrayHost`；`platform/windows`、`platform/macos`、`platform/linux/x11`、`platform/linux/wayland` 分别实现。应用组合这些能力，不直接调用 Win32/AppKit/X11/Wayland API。

接口只放在平台或并发边界。不得为每个类型创建 `Manager`、`Service`、`Repository`，不得用一套自造 scene graph 再翻译到 FemtoVG。渲染器直接读取稳定的文档/视图状态并绘制。

平台接口按能力拆分，避免一个拥有大量空实现的总接口。公共数据类型不得包含 HWND、Win32 virtual-key、CoreGraphics、X11 或 Wayland 句柄。

## 6. 性能验收

“性能不降低”以同一台机器、同一分辨率、同一发布构建和同一操作脚本对比旧项目，不凭主观感觉判断。

至少记录以下指标的中位数和 P95：

| 场景 | 验收门槛 |
|---|---|
| 启动到可响应 | 不慢于旧项目 5% |
| 热键到冻结画面可见 | 不慢于旧项目 5%，且无可见黑帧 |
| 静止覆盖层 | 0 连续动画帧，CPU 占用不高于旧项目 |
| 拖动选区/标注 | P95 帧时间不高于旧项目 5%，无周期性卡顿 |
| 4K 截图纹理 | 每次会话完整上传最多一次 |
| 马赛克画笔 | 不发生逐点整图复制或整图纹理重建 |
| 导出 | 结果一致，耗时不高于旧项目 10% |
| 滚动匹配与拼接 | 成功率不得降低，耗时不高于旧项目 10% |
| 内存稳定性 | 连续 50 次捕获后无持续增长，峰值不高于旧项目 10% |
| 发布包体积 | 每个目标平台单独记录；新增依赖必须说明实际用途 |

若某项未达到门槛，相关阶段不能标记完成。先用 profiler 确认 CPU、GPU、驱动同步或分配热点，再决定优化；不得通过删除效果、降低采样质量或减少功能达标。

## 7. 测试与交付门槛

- 旧项目现有 385 个通过测试是行为规格来源；新项目应按新模型重写对应测试，而不是依赖旧测试代码。
- 几何、选区、标注、撤销/重做、裁剪坐标和滚动匹配必须有平台无关单元测试。
- 每个平台必须有真实窗口/context 创建、截图像素格式、DPI 和剪贴板集成测试。
- OpenGL 渲染使用离屏像素基准或截图回归，覆盖普通 DPI、150% DPI、4K 和负桌面原点。
- Windows 首条垂直链路必须真实完成：抓取桌面、创建原生全屏窗口、创建 OpenGL context、上传一次截图纹理、FemtoVG 绘制选区/工具栏、按需 Present。
- 每完成一个功能域，都与旧项目执行相同用户流程并更新功能矩阵和基准结果。

## 8. 实施顺序

1. 公共帧格式、坐标和选区状态；Windows 原生抓屏、WGL 窗口和 OpenGL/FemtoVG 首帧。
2. 自动高亮、放大镜、完整选区交互和全屏工具栏。
3. 标注文档、命中测试、文字编辑、撤销/重做和 GPU 马赛克。
4. FBO 导出、保存、剪贴板、贴图和 OCR。
5. 滚动捕获、ORB 匹配、拼接预览和转编辑。
6. Slint 可编辑预览、自定义标题栏和设置窗口。
7. 托盘、快捷键、权限及其他系统集成。
8. macOS、Linux X11、Linux Wayland 原生实现与能力测试。
9. 全功能回归、性能对比、包体积记录和发布构建。

顺序可以因测试暴露的问题调整，但不得用临时旧后端完成任一阶段。每一阶段都必须可运行、可测试，并保留前面阶段的性能性质。

## 9. 当前实现状态

> 本节只记录已经验证的事实，不代表整个重写已经完成。

2026-08-16 已完成第一条 Windows 垂直链路：

- 固定 `app/model/rendering/platform/windows|macos|linux/x11|linux/wayland/ui/scroll/ocr` 目录；
- `app` 通过 `DesktopCapture`、`CaptureOverlay`、`ImageClipboard`、`PlatformCapabilities`/`PlatformBackend` trait 使用平台能力；
- GDI 一次性抓取带负原点信息的虚拟桌面 BGRA 帧；
- Win32 原生全屏顶层窗口和按需 `WM_PAINT`；
- WGL 创建 OpenGL 3.3 core context，无 Direct2D、`windows-canvas`、winit、wgpu；
- OpenGL 静态纹理一次上传并绘制桌面；
- `WindowLocator` trait 和 Windows 原生窗口/控件命中实现，支持高亮后单击选取或拖动改为手动选区；
- FemtoVG 绘制遮罩、选区边框、控制点、十字线、15x15 像素放大镜、颜色信息和 15 命令工具栏；图标使用统一 24 单位网格的 FemtoVG 原生缓存路径和随按钮缩放的 1.75 单位描边；
- 新建、移动、八方向缩放、边界约束、全选、键盘微调和最小尺寸恢复；工具栏具备悬停、按下、禁用、激活和按下/释放匹配状态；保存和贴图由显式 `OverlayFeatures` 能力开启，未接入的 OCR、滚动截图和语言命令明确禁用，不保留可点击的空处理；
- Rectangle、Circle、Arrow、Pen、Text、Mosaic、Emotion 已有上下文选项；颜色、线宽、填充、字号、马赛克块尺寸和 40 个表情通过公共 `Editor` 状态直接作用于新标注或选中标注；
- OpenGL shader 实时合成 GPU 马赛克，路径只上传动态 capsule 网格，笔刷半径随 10/16/24 像素块尺寸对应为 5/8/12 像素，截图纹理不重传；
- 导出使用可按尺寸复用的 OpenGL framebuffer/颜色纹理：OpenGL 在选区坐标系内绘制截图和 GPU 马赛克，FemtoVG 把同一纹理作为 image render target 并追加已提交文档标注；预览与导出共用 `paint_document`，导出不包含 draft、hover、选中框、控制点、caret、遮罩和工具栏；
- 导出结束只执行一次 `glReadPixels`，在单个 RGBA 缓冲区内原地翻转为顶向下行序；Windows 默认确认结果通过独立 `ImageClipboard` trait 写入带 alpha 的 `CF_DIBV5`，RGBA 到标准 BGRA 的转换直接写入剪贴板全局内存；
- 保存命令在应用层使用纯 Rust `png`/`jpeg-encoder` 流式编码，在 Windows 通过小型 `ImageSaveDialog` trait 调用原生保存对话框并根据扩展名/筛选器确定格式；覆盖层事件回调不执行图像编码；
- 贴图命令通过独立 `PinnedImageHost` trait 创建 Win32 顶层窗口和 WGL/OpenGL 静态纹理，纹理只上传一次，窗口按需重绘；该窗口不依赖全屏 overlay、Slint、winit 或 FemtoVG；
- Windows capability 只对已经实现的桌面捕获、窗口检测、图像剪贴板、图像保存和贴图报告 `Native`，尚未实现的全局快捷键、托盘和捕获排除报告 `Unavailable`；
- 71 个平台无关或无窗口测试通过；严格 Clippy 零警告；真实窗口/context/上传/FemtoVG/Present smoke test 已在首条链路完成时通过；后续开发按当前约定只通过代码测试、编译和 Clippy 验证，不再运行或生成界面截图，由用户自行检查实际界面；
- 加入 FBO 导出、PNG/JPEG 编码和贴图窗口后的 Windows release 二进制为 1,500,160 字节；先前 373,248 字节数据只代表未包含这些功能的阶段。10 次单帧 smoke test 的历史基线为中位数 281.12 ms、最小 279.67 ms、最大 293.08 ms，本阶段按约定未重新运行窗口基准。

尚未完成的功能仍按第 3 节矩阵验收，不能标记为可发布。旧项目还没有同条件的 release 单帧基准，因此当前性能数据只是新实现基线，不能据此宣称已经达到“不降低”。

2026-08-16 后续实现状态：

- Windows OCR 使用系统 `Windows.Media.Ocr`，输入直接取 FBO 最终合成的顶向下 RGBA 帧；WinRT 初始化、可用语言枚举、指定语言、图像尺寸上限、Gray8 转换、行/词边界和文本角度均在 `platform/windows/ocr.rs` 内完成。识别文本通过独立 `TextClipboard` trait 写入 `CF_UNICODETEXT`，不重新截图，也不在覆盖层事件回调中执行 OCR。语言选择命令在选择 UI 接通前继续禁用。
- OpenCV 4.12 的 x64/VC17 静态 SDK 已迁入新项目自己的 `third_party/opencv-4.12-static`；Cargo 使用项目相对路径发现 `core`、`imgproc`、`features2d` 和 `zlib`，默认启用真实 `opencv` crate ORB 绑定，不依赖旧项目目录、系统 OpenCV 安装或运行时 DLL。
- 滚动截图公共层固定为“采样指纹帧去重 -> OpenCV ORB 特征与描述子 -> Hamming KNN/Lowe ratio -> 位移中位数/MAD 内点 -> 重叠行定位 -> 仅追加新底部行”。上一张已接受帧的 ORB 特征会跨帧缓存，拒绝帧不会污染缓存；算法没有替换成模板匹配或单纯像素差。
- `ScrollCaptureSource`/`ActiveScrollCapture` 与 `ScrollPreviewHost`/`ScrollPreview` 是两组独立的小型平台接口。公共 `scroll` 只持有 RGBA 帧、匹配结果、拼接文档和预览脏区，不包含 HWND、WGL、AppKit、X11 或 Wayland 句柄；macOS、X11 和 Wayland 各自保留明确实现位置并报告真实 capability。
- Windows 滚动采集源在会话开始时只创建一次 GDI DC/DIB，通过低级鼠标/键盘 hook 观察选区内滚轮，等待滚动稳定后抓取区域帧；Enter 结束、Escape 取消。右侧预览是独立 Win32 + WGL/OpenGL 宿主，使用最多 2048 行的分块纹理和 `glTexSubImage2D` 增量上传，不因长图增长反复上传全部拼接结果，也不受单张超长纹理高度限制。
- 滚动捕获结束后必须进入 Slint + OpenGL/FemtoVG 可编辑预览；该窗口尚未接通，因此滚动工具栏命令继续通过 `OverlayFeatures` 禁用。采集源、ORB、拼接或右侧预览的局部完成不代表第 5 阶段完成，也不得用直接复制或贴图窗口替代“继续编辑”。
- 加入 Windows 系统 OCR 和静态 OpenCV ORB 后，87 个无窗口测试通过，包含合成长页面的真实 ORB 垂直位移测试；`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` 和 `cargo build --release` 通过。当前 Windows release 可执行文件为 5,603,840 字节；该数字尚未包含 Slint 可编辑预览，且仍需与旧项目做同条件性能基准后才能判断“不降低”。

2026-08-16 托盘、快捷键与截图交互修正：

- Windows 启动流程不再立即截图。应用先安装不依赖 winit 的原生 Slint platform，创建 Slint `SystemTrayIcon`，并通过平台无关的 `GlobalShortcutHost`/`Shortcut` trait 注册 `Ctrl+Alt+S`；托盘“截图”和热键共用同一个防重入截图命令，托盘“退出”显式终止事件循环。
- Windows Slint platform 使用 Win32 消息泵驱动 Slint timer、托盘隐藏窗口和 `WM_HOTKEY`，并提供线程安全的事件循环唤醒代理；`create_window_adapter` 在普通 Slint 窗口适配器接通前明确返回不支持，不伪装可编辑预览已经完成。Windows 对托盘和全局快捷键报告 `Native`，其他平台仍按真实实现状态报告能力。
- 贴图窗口改为独立原生 UI 线程，并在窗口、WGL context 和首帧初始化完成后通过一次性通道向调用方报告结果。贴图不再独占主托盘消息循环，可保留多个贴图并继续使用全局截图热键。
- 全屏截图工具栏不再维护手绘图标或裸数组位置约定。16 个 action 通过强类型 `ToolbarIcon` 显式映射到原项目 SVG，初始化时以 96px 栅格化并只上传一次，缩小采样启用 mipmap，屏幕绘制恢复为最大 18px，避免部分 2px SVG 描边显得过粗；禁用状态优先于 hover、pressed 和 active。
- Arrow 选中框只使用真实 Start/End 端点手柄；截图选区手柄恢复白色填充、黑色边框和 Windows 蓝选区线，标注手柄使用中性灰。已提交文本单击只选择/移动，双击重新进入编辑并把 caret 放到文本末尾，编辑时使用 I-beam 光标和绿色 caret。
- 本轮 104 个无窗口测试通过；`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 和 `cargo build --release` 通过。Windows release 可执行文件为 8,751,616 字节。未运行窗口或截图流程；Slint 可编辑预览、macOS/Linux 原生 UI runtime、设置持久化、开机启动和单实例仍未完成，因此当前状态不能标记为完整跨平台或性能不降低。

2026-08-16 可编辑预览、设置与交互回归修正：

- Windows 普通 Slint 窗口已接入自定义 Win32 HWND/WGL/FemtoVG adapter。Slint 负责预览和设置窗口的控件、布局与自定义标题栏，大图与标注文档仍走 OpenGL/FemtoVG 渲染，不引入 Direct2D、winit、glutin、wgpu 或软件预览路径。
- 滚动截图命令已通过 `OverlayFeatures` 启用，既有 OpenCV ORB 匹配与拼接算法保持不变。拼接完成的 RGBA 文档会进入可编辑预览，支持工具、平移、缩放、旋转、撤销/重做、文字输入和双击编辑、复制与保存。
- 预览的 BGRA 桌面输入与 RGBA 拼接输入在 `DocumentPass` 中显式区分；复制和保存先在渲染阶段完成 GPU 合成，再调用平台剪贴板和保存能力，不从事件回调另外走一套软件绘制。
- 表情使用 Windows `Segoe UI Emoji` 的 COLR/CPAL 数据由 `swash` 一次栅格化为彩色预乘 alpha 纹理，并在 OpenGL 侧缓存和生成 mipmap；拖动四角手柄时只缩放已有 GPU 纹理，保持正方形比例、对角锚点、彩色像素和正确选中边框。
- 设置窗口只保留旧项目第二个“系统设置”tab 的截图热键、保存路径和 OCR 语言。设置持久化、原生目录选择、热键即时重新注册、OCR 语言和默认保存目录均已接通，托盘设置命令打开同一窗口。
- 自动高亮区域的单击选择改为在按下时原子提交最终矩形，同时清除高亮和拖动态；不再先创建 0x0 临时手动选区，因此首个重绘帧不会出现选区由小变大闪烁。超过 click slop 的拖动仍切换为原有手动选区行为。
- 本轮 121 个测试通过，包含彩色系统表情、四角等比缩放、可编辑预览、设置持久化以及高亮按下/释放矩形稳定性的回归测试；`cargo fmt --all --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 和 `cargo build --release` 全部通过。当前 Windows release 可执行文件为 14,866,432 字节。按约定未启动窗口或执行截图；macOS、Linux X11/Wayland 原生实现和同条件性能基准仍未完成，因此不能据此宣称整个项目已完整跨平台或性能不降低。

2026-08-16 OCR 预览与滚动覆盖层回归修正：

- “提取文字”不再只同步识别并复制文本。覆盖层导出的同一张 RGBA 图像交给 `TextRecognizer` worker，识别完成后通过 Slint 事件循环打开标题为“截图预览”的普通窗口；识别成功仍写入系统文本剪贴板，空结果和识别失败也会打开窗口并显示明确状态，不再表现为点击后无窗口。OCR、ORB、图像编码均不进入高频指针事件。
- OCR 与普通可编辑预览共用同一个 `PreviewSession`、OpenGL `DocumentPass` 和 FemtoVG 标注文档，没有增加软件图片预览或第二套编辑语义。OCR 模式把现有画布放在左侧，右侧使用 Slint 只读 `TextEdit` 显示可选择、可滚动的识别文本；`Ctrl+A`、`Ctrl+C` 和右键复制由 Slint 文本控件处理，Windows Slint platform 将其剪贴板出口接到现有 `CF_UNICODETEXT` 实现。
- OCR 窗口使用 `no-frame: true`。顶部 36px 标题行、其下 50px 命令行以及最小化、最大化/还原、关闭按钮共同组成应用自绘标题区域，图片和 OCR 文本内容从 86px 以下开始。查看命令补齐放大、缩小、原始大小、适应窗口和旋转，标题栏与标注按钮继续使用原项目 SVG 资产；Win32 adapter 对标题栏按钮、命令行、图片画布和 OCR 文本区分别提供 hand、crosshair 和 I-beam 系统光标，不依赖 Slint 内部 API。
- 滚动模式先注册 `ActiveScrollCapture`，再显示全屏冻结桌面覆盖层和右侧增量预览，消除工具栏可见但结束消息接收端尚未存在的启动竞态。全屏覆盖层继续通过窗口 region 挖出选区以让下层页面接收滚轮，工具栏保持编辑、保存、取消、完成的原顺序；OpenCV ORB 匹配、拒绝规则、重叠定位和仅追加新行算法没有改动。
- 本轮 130 个无窗口测试通过，新增原始大小、滚动工具栏顺序/命中/边缘避让和 100%/150% DPI OCR 光标区域回归测试；当前代码验收只执行 `cargo check --all-targets`、`cargo fmt --all --check`、`cargo test` 和 `cargo clippy --all-targets -- -D warnings`，后续不再执行 `cargo build --release`，也不以发布二进制体积作为本阶段验收项。按约定不启动窗口或执行截图；macOS、Linux X11/Wayland 原生实现和同条件性能基准仍未完成，因此不能据此宣称整个项目已完整跨平台或性能不降低。
