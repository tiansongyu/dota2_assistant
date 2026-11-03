# 开发进展

更新：2026-11-03。已发布 [v0.1.0-dev.1](https://github.com/tiansongyu/dota2_assistant/releases/tag/v0.1.0-dev.1)，提供 Windows 安装包与 Linux 调试工具；代码完成、自动化通过和游戏验收分别标记。

## Phase 0：技术预研

已完成：工具链、官方依赖、参考代码与许可证、OpenDota 真响应、架构与风险文档。

新增/修改：`architecture.md`、`data-sources.md`、`technical-risks.md`、`development-plan.md`、`research-capture.md`。

测试：真实英雄/历史/详情请求成功，分页投影获得十人阵容。

尚未验证：第三方商业数据授权、Valve 素材再分发权、Windows 真实 UI。

问题：部分官方旧配额文档与响应不同，按实际 response header/429 处理。

下一阶段：已进入工程实现。

## Phase 1：基础工程

已完成：模块化 Rust Workspace、Tauri 2、React/TS/Vite、SQLite 迁移、日志、配置和 CI。

新增/修改：`Cargo.toml`、`apps/desktop`、`crates/domain`、`crates/persistence`、构建与检查脚本。

测试：全 Workspace 编译、50 项 Rust 测试、严格 clippy/fmt 和前端门禁通过；Linux Tauri debug 产物可构建，25 项原生检查 / 26 次真实 IPC 通过。

尚未验证：全部 Windows UI 原生行为及安装环境。

问题：已有 10k/50k 合成模块性能报告；完整桌面快照、IPC 和渲染仍需端到端测量。

下一阶段：已接入个人历史。

## Phase 2：个人历史系统

已完成：精确 Steam ID、真实 OpenDota 分页、暂停恢复、限流、去重、来源/缺失、统计窗口和英雄池。

新增/修改：`data-provider`、`history`、`hero-pool`、`statistics`、历史与英雄池页面、真实 fixture。

测试：真实 4 场分页及重复导入；独立 CLI 10 场导入和真实 Ctrl-C 暂停；SQL 账号隔离与两次查询组装阵容测试。

尚未验证：用户本人 3—5 年公开历史完整度与长期大数据性能。

问题：列表缺失 patch；未推断五位置；统计缓存暂未物化。

下一阶段：已完成视觉模块代码，等待真实数据验收。

## Phase 3：屏幕捕获与识别

已完成：WGC 指定窗口/ROI、OpenCV 模板缓存、严格 profile、多帧、锁定证据、冲突/空槽、人工修正、可视化校准与评估 CLI。

新增/修改：`capture`、`vision`、`draft-state`、校准 UI、`tests/vision`、模板下载脚本。

测试：捕获 Windows GNU 交叉检查；合成管线/状态测试；校准 ZIP 实际导出。127 官方肖像缓存下载成功。

尚未验证：真实 DOTA2 截图数量为 0，准确率/FPS 均未测；四种分辨率和所有游戏模式未验收。

问题：没有可默认启用的已验证 profile；可靠自动禁用识别未完成。

下一阶段：继续保守候选流程，并等待真实截图和 Windows 实机。

## Phase 4：推荐引擎

已完成：硬过滤、平滑、衰减、去重对抗/协同、解释、权重、严格时间前缀与基线回测。

新增/修改：`recommender`、回测 example、模型与英雄池文档。

测试：模式混用、未来数据、重叠比赛、数值溢出和不一致权重分母等回归通过。真实小样本无位置池时报告不可评估，没有虚构效果。

尚未验证：大样本效果、权重优化、当前版本总体统计、概率训练/校准。

问题：外部总体统计仅保留协议，尚未接入。当前分数不是胜率。

下一阶段：收集足量已授权个人历史和可靠总体证据后评估。

## Phase 5：悬浮窗与性能

已完成：独立悬浮窗、紧凑/透明/穿透、拖动/位置记忆、恢复快捷键、后台任务、变化事件更新。

新增/修改：Tauri 服务、悬浮页面、诊断面板、捕获生命周期。

测试：浏览器悬浮布局、快捷键恢复的后端约束、Linux 原生构建和双窗口实际测试通过；25 项原生检查 / 26 次真实 IPC 包含紧凑窗口高度、宽度保留和重启。另完成 10k/50k 合成历史的读取、聚合与推荐测量。

尚未验证：实际 DOTA2 窗口遮挡、Windows 多显示器、长期资源占用和 FPS。

问题：未做游戏性能承诺；CPU/GPU及端到端捕获延迟仍需外部实测。

下一阶段：Windows 开发安装包已发布；真实游戏回归仍需 Windows 硬件与截图。

## Phase 6：开源仓库与开发打包

已完成：MIT 公共仓库、中文 README、隐私/贡献/安全说明、变更记录、第三方声明与许可收集、GitHub 工作流。发布 `v0.1.0-dev.1` Windows NSIS 安装包、独立 Linux API 调试工具和 SHA-256 文件。

新增/修改：根目录开源文件、`.github`、`debug-cli`、`scripts/collect-licenses.py`、`linux-debug.md`、`windows-build.md`、Outdraft 功能对照、`reports/release-v0.1.0-dev.1.md`。

测试：提交 `d276485` 的 Linux/Windows CI 均通过 50 项 Rust 测试、6 项许可收集器测试及格式/静态检查/前端构建。Windows NSIS 在 CI 静默安装并启动，进程存活 8 秒。两个包的摘要和许可内容已核对；Linux 下载包实际执行 `doctor` 成功，无 GTK/OpenCV 动态依赖。

尚未验证：清洁 Windows 安装与全部游戏验收；尚未签名。

问题：Windows 启动 smoke 未验证 WebView 渲染/IPC，runner 有开发 PATH 和预装运行库；VC++ 前置依赖尚未自动安装。当前发布为开发预览，不能标为生产就绪。

下一阶段：在用户的 Linux 环境验证本人公开账号；收集已授权真实选人截图，在 Windows DOTA2 实机执行识别、悬浮窗和 FPS 矩阵，再进行清洁机器安装验收。外部硬件与真实素材缺失的项目保持未验收。
