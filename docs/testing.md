# 测试与验收

记录日期：2026-11-03。自动测试、网络 smoke、真实截图评估和游戏实机验收分别报告，不能互相替代。

## 已执行

| 检查 | 环境与结果 | 边界 |
| --- | --- | --- |
| Rust Workspace | Ubuntu 22.04 / Rust 1.97.1，50 项单元及集成测试通过 | 测试数据来源按目录标注；包含合成算法测试 |
| 格式与静态检查 | `cargo fmt --all --check`；`cargo clippy --workspace --all-targets --locked -- -D warnings` 通过 | 不证明实机性能 |
| 前端 | TypeScript、ESLint、Vite build 通过 | 原生 WebView 与浏览器行为还需分别检查 |
| 浏览器 UI | Chromium，1440 / 1024 / 375 宽的五页与 360 宽悬浮界面，无页面横向溢出和 JS 异常 | 空数据真实渲染，不使用假战绩或 Mock IPC |
| 可视化校准 | 绘制 ROI、数值校验、ZIP 下载、profile 与 PNG 内容检查通过 | 测试图是应用自身截图，非 DOTA2，不用于测识别准确率 |
| OpenDota 网络 | 公开研究账号真实分页、阵容、重复导入检查通过 | 2 页 4 场；详细见 [历史 smoke](reports/history-smoke.md) |
| Linux CLI 网络 | 127 英雄、2 条玩家历史、单场 10 个英雄结构通过；另导入 10 场 | 小样本接口验证，非完整历史覆盖证明 |
| CLI 取消 | 实际 SIGINT 后返回 `paused`，检查点落库 | 继续扫描需遵守安全重扫语义 |
| Windows 捕获编译 | `x86_64-pc-windows-gnu` 交叉 check 与严格 clippy 通过 | 没有执行 Windows 捕获 |
| Tauri Linux 构建 | `npm run tauri -- build --debug --no-bundle` 通过 | Windows 发布包另由 Windows workflow 验证 |
| 前端依赖审计 | npm audit 包括开发依赖为 0 条已报告漏洞 | 是该时间点的数据库结果，不保证没有未知漏洞 |

浏览器脚本：[ui-smoke.mjs](../apps/desktop/tests/ui-smoke.mjs)；结构化结果：[browser-smoke-result.json](../tests/ui/browser-smoke-result.json)。

## 运行质量门禁

```bash
npm ci --prefix apps/desktop
scripts/check.sh
```

仅验证无界面数据模块：

```bash
cargo test -p dpda-domain -p dpda-data-provider -p dpda-persistence -p dpda-history \
  -p dpda-statistics -p dpda-hero-pool -p dpda-recommender -p dpda-debug
```

网络请求只在显式运行 CLI 或 example 时发生，不作为普通单元测试隐式访问外网。API smoke 可用 [Linux 调试指南](linux-debug.md) 中的命令，凭证缺失必须显示 `not_configured`。

## 真实识别数据集

格式与评估命令见 [tests/vision/README.md](../tests/vision/README.md)。需要标注：游戏模式、UI 版本、分辨率、DPI、窗口模式、槽位英雄、是否已锁定、空槽与图像来源。真实与合成素材不能合并后对外报告准确率。

当前真实截图数量为 **0**，所以 `accuracy`、`precision`、`recall`、`false_positive_rate` 和真实截图 latency 均为 **未测量**。不得用代码阈值、合成测试通过或下载头像数量填充这些指标。

## Windows 游戏验收矩阵

以下各项全部待实机执行：

| 配置 | 捕获 | 锁定识别 | 空/灰/预选 | 悬浮/快捷键 | CPU/GPU/内存/FPS |
| --- | --- | --- | --- | --- | --- |
| 1920×1080，窗口/无边框 | 待测 | 待测 | 待测 | 待测 | 待测 |
| 2560×1440，窗口/无边框 | 待测 | 待测 | 待测 | 待测 | 待测 |
| 3840×2160，100%/150%/200% DPI | 待测 | 待测 | 待测 | 待测 | 待测 |
| 3440×1440，超宽屏 | 待测 | 待测 | 待测 | 待测 | 待测 |
| 跨显示器/DPI/分辨率变化 | 待测 | 待测 | 待测 | 待测 | 待测 |
| 最小化、退出、重开与跨局 | 待测 | 待测 | 待测 | 待测 | 待测 |
| 独占全屏 | 待测 | 待测 | 待测 | 待测 | 待测 |

每条结果记录 OS build、GPU/驱动、游戏版本、commit、配置和截图 manifest。性能比较用相同场景、画质和采样区间，比较关闭助手与开启助手的 FPS 分布和资源占用。至少覆盖完整选人流程和持续运行，报告异常及恢复过程。

## 推荐模型

规则测试检查排除/角色/已选/禁用过滤、平滑、去重、标准模式、未来数据和重叠比赛泄漏。时间回测报告实际英雄 Top-K 覆盖与同条件常用英雄基线；没有候选时为不可评估。当前没有策略收益、分版本稳定性或已校准概率的实证结论，见 [推荐模型](recommendation-model.md)。
