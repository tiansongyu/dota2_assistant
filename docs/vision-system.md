# 画面捕获与英雄识别

状态：捕获、校准、模板匹配、状态机与评估工具已有实现；尚未取得本项目真实 DOTA2 截图测试集，也没有执行 Windows 游戏实机验收。本文不报告识别准确率或 FPS 影响。

## 预研证据

2026-11-02 检查了两个参考仓库的实际代码：

| 仓库与固定提交 | 实际实现 | 本项目处理 |
| --- | --- | --- |
| [dota-counterpick-overlay](https://github.com/gestevao04/dota-counterpick-overlay/tree/3a7a7daf7dac734a784a7467af5757c454292291) | `draft_reader.py` 使用 64×36 头像归一化和相关性；`draft_assistant.py` 使用 MSS 显示器捕获、模板差距、唯一性筛选与短期记忆，槽位按分辨率线性缩放 | 仅研究算法思路，独立实现。仓库 MIT 文件的版权人仍为 `[YOUR NAME]`，没有复制其代码或图片 |
| [Dota_Draft_Assist](https://github.com/bijankle/Dota_Draft_Assist/tree/02f2fd25d6f2616ed9c5cb005b89677a48e43f4e) | `vision/recognize.py` 使用 pHash、位移搜索与不同英雄之间的距离差；`capture/session.py` 有时间稳定和阶段门控，当前代码使用 GSI 判断阶段 | 检查的工作树没有 LICENSE；不复制代码或资产。GSI 路径不符合本项目实时数据约束，不采用 |

参考项目自己的测试或 README 声明不构成本项目验收证据。没有从它们取得可再分发、完整标注的真实截图测试集。

依赖验证：`windows-capture 2.0.1`（MIT）、`windows 0.62.2`、`opencv 0.101.1`（Rust binding）已锁定在 Cargo 依赖中。开发机原生 OpenCV 为 4.5.4。已完成捕获 crate 的 `x86_64-pc-windows-gnu` 编译检查和 Linux OpenCV 测试。Windows MSVC、运行时 DLL 打包和实机表现仍需验证。

Windows 实现遵循 [WGC 官方工作流](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture)和 [CreateForWindow 文档](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow)。按 HWND 创建捕获项目的 API 要求 Windows 10 1903 或更新系统。库接口以 [windows-capture 2.0.1](https://docs.rs/windows-capture/2.0.1/windows_capture/) 和 [opencv 0.101.1](https://docs.rs/opencv/0.101.1/opencv/) 为依据。

## 捕获边界

`dpda-capture` 只自动选择窗口标题精确等于 `Dota 2`、可查询的可执行文件名为 `dota2.exe` 的窗口。进程元数据查询仅申请 `PROCESS_QUERY_LIMITED_INFORMATION`，不调用申请 `PROCESS_VM_READ` 的 `Window::process_name()`。不读取游戏内存，不注入进程，不拦截网络。

图像由 Windows Graphics Capture 获取，使用 `Frame::buffer_crop` 将配置的 ROI 从 GPU 复制到 CPU。帧为紧密排列的 RGBA8，包含捕获表面尺寸、ROI 原点、DPI 与时间戳。若没有配置 ROI，可以捕获整个游戏窗口供初次校准；不会自动捕获整个显示器。

软件在 GPU 回读前限速，允许运行时调整 250–10000 ms 的采样间隔；建议初始值为 500–1000 ms。这是工程初始值，不是性能测试结论。最多缓存两帧，队列满时丢弃新帧；不会无限积压。窗口退出、最小化或区域越界时停止捕获。恢复后重新发现窗口并启动会话。

WGC 自身帧池可能仍随画面更新；软件节流不能证明 GPU 开销已经足够低。当前使用默认 OS 更新间隔以兼容旧系统，未启用只在较新 Windows 可用的原生最小更新间隔。独占全屏、HDR、不同显卡、跨显示器切换均未实测。

## 校准与模板

不提供未经验证的默认槽位坐标，也不把 16:9 坐标直接缩放到超宽屏。每个 profile 固定记录 WGC 捕获表面尺寸、Windows DPI 与游戏 UI 版本。运行时尺寸或 DPI 不符会要求重新校准。窗口边框也可能属于 WGC 捕获表面；校准截图必须与实际捕获来源一致。

`CalibrationProfile` 的 JSON 形状如下。这里的数值仅解释格式，不能当作可用 DOTA2 配置；实际文件必须包含 0–9 共十个槽位。

```json
{
  "schema_version": 1,
  "profile_name": "用户实际校准的名称",
  "ui_version": "实际游戏 UI 版本及窗口模式",
  "surface_width": 1920,
  "surface_height": 1080,
  "dpi": 96,
  "draft_anchor": {
    "region": {"x": 10, "y": 10, "width": 80, "height": 30},
    "image_path": "draft-anchor.png",
    "min_similarity": 0.9
  },
  "slots": [
    {
      "slot_id": 0,
      "portrait": {"x": 100, "y": 50, "width": 80, "height": 45},
      "locked_marker": null,
      "empty_marker": null
    }
  ]
}
```

0–4 为天辉，5–9 为夜魇。`draft_anchor` 是选人界面的独立可辨认图案。`locked_marker` 必须来自能区分已锁定与预选的视觉区域，格式与 `draft_anchor` 相同；不能用整个英雄头像自身作为锁定证据。`empty_marker` 可选，用于明确的空槽图案。图片路径相对于 profile 所在目录，拒绝绝对路径和 `..`。纯色参考图会被拒绝。

如果尚未确认游戏 UI 中可靠的锁定指示，应将 `locked_marker` 留为 `null`，使用手动确认。连续多帧显示同一头像不证明玩家已经锁定英雄。profile 的锁定语义还需要用真实悬停、预选、锁定序列验证。

英雄模板清单：

```json
{
  "schema_version": 1,
  "source": "实际资产来源与采集方式",
  "retrieved_at": "实际采集时间",
  "templates": [
    {
      "hero_id": 13,
      "hero_name": "npc_dota_hero_puck",
      "localized_name": "Puck",
      "image_path": "13.png",
      "template_version": "实际模板版本"
    }
  ]
}
```

模板使用稳定英雄 ID，允许同一英雄有多种画面变体。候选差距按不同英雄计算，不把同一英雄的两个模板互相比较。只加载一个英雄的模板时无法建立有效差距，不能自动确认。未内置未经许可核对的英雄肖像；游戏资产版权与静态数据仓库代码许可证分开处理。

## 识别与状态流

```text
指定窗口 → 配置 ROI → 校验尺寸/DPI → RGBA 转灰度
         → 独立选人界面标记 → 十个头像槽位
         → 64×36 归一化 → OpenCV TM_CCOEFF_NORMED
         → 不同英雄候选排序 → 独立锁定标记 → 连续帧与唯一性
         → DraftTracker → 仅 CONFIRMED 进入推荐
```

当前阈值是待回测的工程初始值：英雄相关性至少 0.80、第一与第二不同英雄的差距至少 0.08、至少三个独立时间戳的连续帧，同时具备锁定标记。近乎纯色图像不会返回有效相关性。灰度归一化可减弱饱和度变化的影响，但不能据此保证灰色头像识别准确。

状态机输出 `EMPTY / CANDIDATE / CONFIRMED / UNCERTAIN / INVALID`。一帧缺失某槽位、界面门控失败、重复英雄、时间戳不递增等情况不会默默保留为可靠自动结果。两个槽位自动读到同一英雄时，两者都无效，不按分数猜测一个胜者。手动修正保持权威，直到明确释放或重置比赛。

相同像素复用已缓存的模板匹配结果，仍更新帧时间戳。`DraftTracker` 只有语义状态改变才增加 revision、发出事件；普通置信度微调不会反复触发推荐。游戏窗口丢失和“新一局”操作必须调用 `window_lost()` 或 `reset()`，防止跨局残留。

曾检测到选人界面后，如果连续三个不同时间戳的帧均未检测到选人界面，状态机清空本局英雄、禁用与手动修正，保留己方阵营。初始从未识别到选人界面时不触发这一自动清空，以便手动模式独立使用。

## 集成接口

```rust,ignore
let mut recognizer = dpda_vision::Recognizer::from_files(profile_path, manifest_path)?;
let config = dpda_capture::CaptureConfig {
    window_id: detected_window.id,
    region: Some(recognizer.profile().capture_region()),
    interval_ms: 500,
};
let session = dpda_capture::CaptureSession::start(config)?;
let mut tracker = dpda_draft_state::DraftTracker::default();
// 在后台工作线程中消费 CaptureEvent；Frame 分支：
let analysis = recognizer.analyze_frame(&frame)?;
let events = tracker.apply_frame(
    analysis.timestamp_ms,
    analysis.draft_screen_detected,
    &analysis.observations,
)?;
let confirmed = tracker.snapshot().selected_heroes();
```

`FrameAnalysis` 包含每槽位前三候选、相似度、锁定/空槽标记结果、处理耗时和缓存命中标记。相似度不是分类概率；界面不得显示为“识别正确率”。

`CaptureFrame.capture_ms` 是实际 GPU ROI 回读与紧密缓冲复制的耗时，不包括系统采集/合成排队，不应称为端到端捕获延迟。从文件读取的截图该值为 `null`。

## 真实测试与发布门禁

单张截图检查：

先将官方公开头像下载到个人本地目录：

```bash
python3 scripts/fetch-hero-templates.py --output /path/to/local/hero-templates
```

脚本校验 PNG、记录来源与内容哈希、重试临时网络/限流错误，并保留失败列表。2026-11-02 已用真实 CDN 下载验证两幅头像；这不构成选人画面识别验证，也不授予商业再分发权。使用 GUI 中的区域校准工具从真实 WGC 截图导出 profile 与标记裁剪，再运行下面的检查。

```bash
cargo run -p dpda-vision -- inspect /path/profile.json /path/templates.json /path/screenshot.png
```

按真实序列评估：

```bash
cargo run -p dpda-vision -- evaluate /path/profile.json /path/templates.json /path/dataset.json
```

数据集格式及验收矩阵见 [tests/vision/README.md](../tests/vision/README.md)。没有样本时工具报错，不输出零误差或 100% 准确率。合成素材必须声明 `synthetic`，其结果只能验证代码路径。

发布前至少需要标注并独立评估 1920×1080、2560×1440、3840×2160、3440×1440；包括窗口/无边框、不同 DPI、部分/全部已选、灰色头像、相似头像、长时间悬停但未锁定、最小化/恢复、窗口大小和 UI 版本变化。记录误确认案例、precision、recall、false positive rate、延迟，另用 Windows 工具测 CPU、内存、GPU、DOTA2 FPS 与长时间运行。当前没有这些实测结果，Phase 3 与 Phase 5 实机验收未通过。
