# 屏幕捕获与识别技术预研

核验日期：2026-11-02。本文依据官方文档、固定提交的源码和本地命令结果。当前环境为 Linux，没有 DOTA2 窗口或 Windows GPU 捕获条件；本文不包含真实游戏识别准确率结论。

## 结论与实现边界

采用 Windows Graphics Capture（WGC）捕获指定游戏窗口，Rust `windows-capture` 封装窗口与帧生命周期，OpenCV Rust bindings 处理头像区域。先完成可移植的图像处理、状态机和回放工具，再在 Windows 上验证捕获、DPI、窗口模式和性能。所有总体性能目标、四种目标分辨率的识别率、独占全屏支持仍待实测。

头像匹配结果与“已锁定”证据必须分开。连续多帧出现同一英雄，只能证明图像稳定；玩家可能长时间预选某个英雄。没有经过真实样本验证的可见锁定标记时，结果保持 `CANDIDATE` 或 `UNCERTAIN`，不得送入推荐引擎。用户明确确认的手动纠错可作为独立来源的锁定记录。

## 依赖核验

| 组件 | 已检查版本与提交 | 许可证与条件 | 使用范围 |
| --- | --- | --- | --- |
| `windows-capture` | 2.0.1；`c7d106448eb9d9b251345c39047711e1cd408ae2`，2026-08-08 | MIT；Windows 专用，源码采用 Rust 2024 edition，依赖 `windows` 0.62.2 | WGC 单窗口捕获与局部帧复制 |
| `opencv` Rust crate | 0.101.1；`901d7a221b6172846f77d38c2beb868fab18ef2c`，2026-10-09 | MIT；声明 MSRV 1.88；需要系统 OpenCV 和 Clang | `imgproc`、`imgcodecs`，禁用不需要的默认模块 |
| OpenCV | 本次官方 4.x 文档重定向至 4.13.0 | 4.5.0 及以后为 Apache-2.0；与 Rust binding 的 MIT 分开保留通知 | 归一化、灰度转换、缩放、模板匹配 |

上述版本来自仓库清单与 docs.rs 页面。本环境查询 crates.io JSON API 返回 HTTP 403，因此不能据此宣称已核验所有发布渠道的最新版本。实际工程应在 lockfile 中固定成功编译的版本。

来源：[windows-capture 仓库](https://github.com/NiiightmareXD/windows-capture/tree/c7d106448eb9d9b251345c39047711e1cd408ae2)、[2.0.1 API](https://docs.rs/windows-capture/2.0.1/windows_capture/)、[opencv-rust 清单](https://github.com/twistedfall/opencv-rust/blob/901d7a221b6172846f77d38c2beb868fab18ef2c/Cargo.toml)、[OpenCV 许可证](https://opencv.org/license/)。

## Windows 捕获接口与生命周期

Microsoft 的 `IGraphicsCaptureItemInterop::CreateForWindow` 面向单个 `HWND`，最低支持 Windows 10 1903（18362）。`GraphicsCaptureSession::IsSupported` 仍须在运行时检查；操作系统版本满足要求不等于当前设备、驱动或目标窗口一定可捕获。[Microsoft 单窗口接口](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow)、[捕获流程](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)。

已在 `windows-capture` 2.0.1 源码核对以下入口：

- `Window::enumerate()`、`Window::from_name()`、`Window::title()`、`Window::process_id()`、`Window::as_raw_hwnd()`：发现窗口并建立来源身份。标题模糊匹配可能抓到浏览器或助手自身，应精确匹配并验证进程身份；只读进程路径不等于读取游戏内存。
- `GraphicsCaptureApiHandler` 的 `new`、`on_frame_arrived`、`on_closed` 与 `start_free_threaded`：专用捕获线程与停止句柄。回调只做频率判断、区域复制和投递，识别工作使用有界队列，积压时保留最新帧。
- `Settings::new(...)`：设置颜色格式、光标、边框、更新间隔等。优先 `Bgra8`，图像转换时明确通道次序。
- `Frame::buffer_crop(start_x, start_y, end_x, end_y)`：使用 D3D11 子区域复制减少 CPU 读取。其源码只检查起点小于终点；调用层必须先检查坐标是否在当前帧内。捕获源仍是窗口，不能把局部 CPU 复制描述成操作系统只采集局部窗口。
- `FrameBuffer::as_raw_buffer()` 等接口涉及 row pitch/padding，应按实际步长读取，不能假定 `width * height * 4` 与原始缓冲区布局相同。

`MinimumUpdateIntervalSettings::Custom(Duration)` 是 OS 端最小更新间隔，不保证恒定帧率。源码会检查 `MinUpdateInterval` 属性是否存在；不支持时使用 `Default`，在复制图像前自行限频。取消光标、关闭捕获边框等选项也有版本能力检查，不应因可选优化不可用而使整个功能启动失败。[间隔设置说明](https://docs.rs/windows-capture/2.0.1/windows_capture/settings/enum.MinimumUpdateIntervalSettings.html)、[捕获实现](https://github.com/NiiightmareXD/windows-capture/blob/c7d106448eb9d9b251345c39047711e1cd408ae2/src/graphics_capture_api.rs)。

应用层仍需处理：`IsIconic` 最小化检测、窗口关闭与重建、无帧超时、尺寸变化、DPI 与客户区坐标、切换显示器。`Window::is_valid()` 的检查不能替代最小化检测。帧尺寸变化后必须重新验证区域，不能继续使用旧坐标。HDR 可能导致色调差异；Microsoft 建议按 HDR 链路处理，本版应将未经测试的 HDR 明确标为待验证配置。

## 参考仓库 A：dota-counterpick-overlay

仓库：[gestevao04/dota-counterpick-overlay](https://github.com/gestevao04/dota-counterpick-overlay)。检查提交：`3a7a7daf7dac734a784a7467af5757c454292291`（2026-10-31）。

`LICENSE` 是 MIT 文本，但版权行仍为 `Copyright (c) 2026 [YOUR NAME]`。保留这项来源异常；如将来复制实质代码，需要保留原始通知并核实署名。当前 DPDA 不复制该仓库代码。

实际代码行为：

- `draft_assistant.py::grab` 使用 `mss` 捕获指定显示器；不是 WGC 指定窗口捕获。
- `slots_for` 将固定参考坐标按整张图的宽、高分别缩放。该方法不能证明超宽屏或不同 HUD 排版支持。
- `TemplateBank` 对小尺寸彩色图做零均值归一化向量匹配；每个英雄允许多个头像模板。匹配使用原始相关度阈值与第一、第二名差值，并降低在多个槽位都得到高分的模板影响。
- `read_draft` 通过全局唯一性去除重复英雄；`SlotMemory` 在连续识别失败后移除旧结果，但新匹配没有独立的多帧锁定证明。
- `draft_reader.py::fetch` 从 OpenDota `heroStats` 获得稳定英雄 ID 和图片路径，再下载 Valve CDN 头像。

可借鉴范围是候选差距、多个模板映射至同一英雄、重复检测、诊断输出等一般设计。固定阈值、固定坐标以及 README 的使用描述不能作为 DPDA 的验收证据。[识别与采集源码](https://github.com/gestevao04/dota-counterpick-overlay/blob/3a7a7daf7dac734a784a7467af5757c454292291/draft_assistant.py)、[模板下载源码](https://github.com/gestevao04/dota-counterpick-overlay/blob/3a7a7daf7dac734a784a7467af5757c454292291/draft_reader.py)。

本地执行：

```bash
DOTA_OVERLAY_HOME=/tmp/dpda-capture-research/overlay-test-data python3 -m unittest discover -s tests -t . -q
```

结果：47 个测试通过。已检查 `tests/helpers.py`：API、Tk UI 和头像素材使用替代实现或合成数据。这个结果仅证明该提交的自动测试在当前 Linux 环境通过，不证明真实截图准确率、Windows 捕获或游戏性能。

## 参考仓库 B：Dota_Draft_Assist

仓库：[bijankle/Dota_Draft_Assist](https://github.com/bijankle/Dota_Draft_Assist)。检查提交：`02f2fd25d6f2616ed9c5cb005b89677a48e43f4e`（2026-10-10）。

`git ls-files` 未发现项目代码的根许可证；GitHub `/repos/bijankle/Dota_Draft_Assist/license` 返回 HTTP 404。`assets/fonts/OFL.txt` 只覆盖字体，不能视作整个项目的许可证。当前仅分析行为和局限，不复制或移植该项目代码。

实际代码行为：

- `capture/session.py` 延迟导入 Python `windows_capture` 并绑定 Dota 窗口。捕获回调保存最新帧，UI 定时驱动约 1 Hz 的 gate 与约 2 Hz 的头像识别。
- `vision/recognize.py` 使用灰度 pHash、多个偏移裁剪、最佳匹配距离与不同英雄之间的差距；允许 `unknown` 和空槽。
- `SlotStabilizer` 通常需要连续三次相同结果；它解决抖动，不是锁定标记识别器。其未知结果保留旧英雄的策略也需要明确的会话重置与过期机制。
- `vision/layout.py` 与 `vision/measured.py` 分别处理 HUD 横向区域、纵向坐标和已测量的比例/分辨率。源码记录过“使用同一坐标公式生成合成图再验证公式”的循环验证问题。
- 当前整体产品已接入 `gsi/`，`CaptureSession` 也接收游戏状态提示。该路线不符合 DPDA 当前阵容只能来源于可见画面的要求，不能整体复用。
- 真截图、录屏、下载头像被 `.gitignore` 排除；代码注释中的作者测量结论没有随克隆提供原始样本。本次无法复验其真实游戏结果。

依赖包括 NumPy、OpenCV Python、requests、PyYAML、python-dotenv、openpyxl，Windows 运行另外需要 windows-capture、pywin32、PyQt6。DPDA 使用既定 Tauri/Rust 技术栈，不引入这一套 Python GUI 运行时。[捕获源码](https://github.com/bijankle/Dota_Draft_Assist/blob/02f2fd25d6f2616ed9c5cb005b89677a48e43f4e/draft_assist/capture/session.py)、[识别源码](https://github.com/bijankle/Dota_Draft_Assist/blob/02f2fd25d6f2616ed9c5cb005b89677a48e43f4e/draft_assist/vision/recognize.py)。

本地选择性执行：

```bash
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3 -m pytest --noconftest -q tests/test_vision.py tests/test_gate_session.py
```

结果：35 个测试通过，1 个测试因缺少 PyQt6 失败。最初普通 pytest 被当前系统安装的 `launch_testing` 插件兼容性错误阻止；禁用插件自动加载后得到上述结果。`--noconftest` 用于避开上游自动导入整个 Qt UI 的 fixture，因此这是有限的算法/状态测试，不是该仓库完整测试通过。`test_vision.py` 明确使用程序生成的头像和画面。

## 头像素材与模板版本

已实际访问 odota/dotaconstants 的 `build/heroes.json`，读取英雄 ID 1 的 `img` 路径，再请求 Valve CDN：HTTP 200，`image/png`，62,893 字节，OpenCV 解码为 256 × 144 彩色图片。SHA-256 为 `7e47a9f52f7f1cdcb77b38e1f3e26ce967a3ed0eceb4b0d808f5ea14d347ce39`。这是单张头像来源与可解码性的验证；没有验证完整英雄覆盖率，也没有证明它与选人界面头像裁剪一致。

来源：[dotaconstants 英雄数据](https://github.com/odota/dotaconstants/blob/master/build/heroes.json)、[已测试的 Anti-Mage 图片](https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/heroes/antimage.png)。

静态数据仓库的代码许可证不自动覆盖 Valve 英雄图片。Valve 的网站条款保留素材权利，不能据此推导商业再分发授权。个人本地缓存与对外安装包内附带完整图片库需要分别处理；商业图片再分发许可目前未确认。模板实现应支持本地导入和有来源记录的下载，发布前再核对素材授权。[Valve 条款](https://www.valvesoftware.com/en/legal)。

模板清单建议包含 `hero_id`、`hero_name`、`localized_name`、`image_path`、`template_version`、来源 URL、下载时间和校验和。使用英雄 ID 作为映射键；同一英雄可有默认、至宝、身心和用户采集的多个变体。把模板版本、校准版本、游戏 UI 版本与测试报告绑定，更新后重新跑回放集。

## DPDA 视觉模块的验收约束

1. 先验证选人画面的可见特征，再定位头像槽位；菜单、加载界面和游戏内 HUD 属于负样本。不能只靠“找到几张相似头像”确认游戏阶段。
2. 彩色/灰度归一化与 `matchTemplate` 使用同尺寸、同类型数据；常量图和极低方差图先拒绝，避免归一化相关度退化。第一、第二名必须按不同英雄 ID 比较。[OpenCV 模板匹配接口](https://docs.opencv.org/4.13.0/df/dfb/group__imgproc__object.html)。
3. `portrait_confidence`、`lock_evidence`、`screen_evidence` 和 `source_timestamp` 分别保存。稳定帧、唯一性、锁定证据全部满足后才确认英雄；相似度不是经过校准的识别概率。
4. 手动纠错有来源和会话范围；不能被下一帧低置信度结果立即覆盖。新局、窗口丢失或区域失效时必须清除或冻结不再可信的自动状态。
5. 校准绑定分辨率、显示比例、DPI、窗口模式和 UI 版本，提供可视化槽位与锁定标记预览。未经实测的新配置不能仅通过线性缩放标为受支持。
6. 真实截图集必须有独立标注、来源与授权记录。目标配置为 1920 × 1080、2560 × 1440、3840 × 2160、3440 × 1440，覆盖空槽、预选、悬停、锁定、灰色头像、相似英雄和 UI 缩放。
7. 分别报告逐槽位 accuracy、precision、recall、false-positive rate、误确认预选次数及识别延迟；按分辨率和场景分组。合成回归测试与真实截图评估分开呈现。
8. Windows 实机记录 CPU、内存、GPU、采集/识别耗时、DOTA2 FPS 对照、最小化恢复、跨显示器与长时间运行结果。在取得这些证据前，Phase 3 和 Phase 5 的实机验收保持未完成。
