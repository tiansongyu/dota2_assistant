# 开发进展

更新：2026-11-03。已发布 [v0.1.0-dev.1](https://github.com/tiansongyu/dota2_assistant/releases/tag/v0.1.0-dev.1)，提供 Windows 安装包与 Linux 调试工具；代码完成、自动化通过和游戏验收分别标记。

## Windows 图标构建修复

已完成：定位失败清单中的秘法鞋重定向拒绝，以及另外四张图片与资源锁的字节差异；默认下载改为有归档、清单和资源锁 SHA-256 的固定快照，保留独立 CDN 更新工具。快照已放到 [资源发布页](https://github.com/tiansongyu/dota2_assistant/releases/tag/game-icons-20261103-v1)，不修改旧应用安装包。

新增/修改文件：`fetch-locked-game-icons.py`、`pack-game-icons.py`、`game-icons.snapshot.json`、原下载器、Node 门禁、Windows/Linux 工作流、测试与 [故障记录](reports/game-icons-build-fix.md)。

测试结果：本地 71 项 Rust、70 项 Python、16 项 Node 测试通过；真实网络/离线恢复与 1,822 个公开文件校验通过。提交 `6b1d25e` 的 Windows/Linux CI 均通过，Windows NSIS 安装及启动检查成功；两个平台的资源清单与快照逐字节一致。详见故障记录。

尚未验证：Windows 实际游戏捕获、WebView 内容与清洁机器安装，仍沿用原验收边界。

存在问题：原 CDN 缺失的 391 个条目和 24 个跳过条目仍如实保留；旧 Windows 日志没有重定向 Location，不能声称复现其目标域名。

下一阶段：继续原有 Windows WebView、清洁机器与真实游戏验收。

## 游戏图标下载与程序接入

已完成：实际下载 1,802 张英雄、技能、物品、命石和辅助图像；来源/哈希锁、断点恢复、共享图和缺失清单；替换英雄文字标记；增加无账号可用的资源页；开发、生产与 Tauri 离线打包集成。

新增/修改文件：`scripts/fetch-game-icons.py`、`lock-game-icons.py`、`verify-game-icons.mjs`、`assets/game-icons.lock.json`、GameAssets 页面与公共英雄组件、Vite/CSP、CI、测试与 [资源文档](game-assets.md)。实际图像保存在生成目录，不写入 Git。

测试结果：71 项 Rust、41 项 Python、12 项 Node 测试及全部格式/静态/前端构建检查通过；1,801 张 PNG 完全解码；开发/生产浏览器与 Linux 原生 Tauri 29 项检查通过。详见 [报告](reports/game-icons.md)。

尚未验证：新资源包在 Windows WebView 中的实际渲染、图像商业再分发授权，以及原有真实选人识别矩阵。

存在问题：目录对应地址中 390 个技能和 1 个物品返回 404，含部分英雄关联先天能力；另有 24 个内部/占位条目。没有把天赋共用图、配方共用图或缺失条目计成独立图片。旧发布包没有此更新。

下一阶段：在 Windows 实机核验新增资源显示；公开服务可复用本次稳定 ID、图像清单和来源记录，仍需落实其数据使用与部署条件。

## 公开服务方案：100–1,000 位用户

已完成：按用户确认规模形成 [公开服务提案](public-service-proposal.md)，明确自有 API/数据库、按需数据同步、账号隔离、自动位置分析、Windows 本地捕获边界及按请求数计算的费用模型。状态为 proposed。

新增/修改文件：`public-service-proposal.md`、README 与架构导航、本进展记录。

验证范围：核对现有源码、Steam 网站登录文档、OpenDota 公开配额与计费代码、Axum/SQLx/PostgreSQL 官方资料；文档相对链接、代码围栏、费用演算和差异空白检查通过。未运行公开服务、付费请求或容量测试。

尚未验证：商业数据使用范围、正式登录回调、付费账户、服务器配置与总成本、多人鉴权和负载。未购买资源、部署网站或上传个人数据库。

存在问题：桌面全局活动账号与全量快照不能直接用于多人网站；需新增用户隔离、分页、持久化任务和全站配额控制。

下一阶段：实现可测试的共享应用服务和服务端基础，再按提案逐步验收。原有桌面实机验收继续独立记录。

## 后续更新：自动位置分析与 Linux 数据查看

已完成：默认自动位置模式、基于同队经济/补刀/分路的可解释多位置候选、详情请求预算和缓存、可选手动覆盖、逐场全部保存字段与 JSON、分页跳转、隔离 Linux 图形启动脚本。更新目前在 `main`，不包含在旧下载包中。

新增/修改文件：`domain/roles.rs`、`hero-pool/role_analysis.rs`、`history/role_enrichment.rs`、Provider/CLI/Tauri、`002_role_evidence.sql`、位置分析组件和历史页、`scripts/start-linux-debug.py` 及相关测试文档。

测试结果：71 项 Rust 和 18 项 Linux Python 测试、全部格式/静态检查/前端构建通过；真实个人数据库副本已通过 launcher/原生 IPC 与完整分页、字段及位置展示检查。公开仓库不含个人数据。

尚未验证：位置推断相对于真实位置标签的准确率，以及原有 Windows 游戏实机矩阵。

存在问题：详情为有预算的采样，部分比赛没有早期经济/补刀或分路；终局指标仅作有限证据，辅助保持四/五号位候选，不强制用户先判断位置。当前回测未合入按时间重建的自动位置池。

下一阶段：通过实际使用反馈补充证据与标注，在保持未知/多位置的前提下评估推断质量。

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

问题：列表缺失 patch；现已提供五位置规则候选，但没有真实位置准确率验收；统计缓存暂未物化。

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
