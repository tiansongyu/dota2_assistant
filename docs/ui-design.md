# 界面设计

状态：设计与实现分开验收。目标为 Windows 桌面应用；浏览器预览只用于检查界面，不提供虚构的账号或战绩。

## 设计方向

DPDA 是选人工作台。界面优先回答三个问题：个人数据是否可用、当前阵容是否已确认、推荐依据是否充分。主要操作左对齐，固定左栏提供总览、英雄池、历史战绩、BP 推荐和设置。BP 页将阵容与推荐并列，避免推荐卡片占用整屏。

采用石墨蓝背景、暖金主操作、青绿天辉与暗红夜魇阵营标记。颜色同时配有文字或图标，不作为唯一状态线索。卡片只用于有独立职责的数据区域；历史记录采用表格。没有统计结果时显示引导和缺失原因，不展示示例胜率。

本次使用 ui-ux-pro-max 和 frontend-design。设计检索中的数据密集仪表盘、可见焦点、对比度和低动效建议与本项目匹配；营销入口、滚动叙事、在线字体和装饰性光效不适用于选人工作台，未采用。

## Design System

| Token | 深色值 | 用途 |
| --- | --- | --- |
| `--bg` | `#10151d` | 页面底色 |
| `--surface` | `#181f2a` | 内容区 |
| `--surface-raised` | `#222c39` | 输入与强调层 |
| `--text` | `#edf1f6` | 主要文字 |
| `--muted` | `#a4b1c3` | 次要文字 |
| `--accent` | `#e7ba72` | 主要操作与当前页 |
| `--radiant` | `#78ccb3` | 天辉与成功 |
| `--dire` | `#ed9898` | 夜魇与错误 |

字体使用 `Segoe UI`、`Microsoft YaHei UI`、系统无衬线回退，不依赖在线字体。正文 14–16px，页标题 28px，区域标题 18px；数据采用 tabular-nums。间距采用 4px 基础，常用 8、12、16、24、32px。输入/按钮圆角 6px，面板圆角 12px。

浅色主题独立配置语义 token，不通过反色处理。焦点框使用高对比色并保留轮廓。页面在紧凑宽度下收缩侧栏、堆叠主要区域；表格在自身容器内滚动，不推动整个窗口横向滚动。

## 页面与状态

- 总览：账号初始化、真实同步进度、历史覆盖、常用英雄、最近比赛和进入 BP 的操作。
- 英雄池：自动、手动、混合模式；位置选择；明确排除英雄；历史样本与胜率；手动位置和优先级配置。
- 历史战绩：日期、英雄、位置、版本筛选；胜负、KDA、阵容完整度。缺失字段显示“—”。
- BP 推荐：己方阵营、目标位置、双方已选和禁用英雄；手动更正；推荐评分与历史统计分栏，样本不足时提示不确定性。
- 设置：账号、半衰期、推荐权重、悬浮窗、外观、捕获诊断与区域校准。

Loading 使用真实请求状态。Empty 给出下一步操作。Error 保留上次成功数据并显示可重试信息。Offline 保留已同步数据；浏览器预览明确提示需要桌面应用，不能把浏览器无 IPC 状态伪装成成功的离线账号。

## 前后端边界

UI 不调用外部 DOTA2 API，所有业务操作经 `services` 层调用 Tauri `invoke`。组件接收来自本地数据库与核心模块的快照。同步状态和阵容变化采用事件刷新；监听注册是异步操作，卸载时等待返回清理函数后再注销，避免事件泄漏。

Steam64 输入始终保留字符串。百分数只展示已知历史胜率；推荐评分使用“分”，不使用百分号。没有通过校准评估的模型时，不出现“本局预测胜率”。

## 悬浮窗

悬浮窗使用独立 Tauri 窗口，提供紧凑与展开模式。拖动区只位于标题栏。透明度在可读范围内调整；点击穿透必须同时具备主窗口解除入口和全局快捷键恢复，避免无法操作。窗口位置保存、多个显示器切换和独占全屏兼容性需在 Windows 实机验收。

## 验收

自动检查覆盖 TypeScript、ESLint 和生产构建。界面检查覆盖空数据、错误、首次设置、键盘焦点、窄窗口、深浅主题与长文本。真实同步、窗口控制和游戏内悬浮结果必须另外检查，前端构建通过不构成这些能力的验收。

## 官方参考

- [Tauri 2 调用 Rust](https://v2.tauri.app/develop/calling-rust/)
- [Tauri 2 前端事件](https://v2.tauri.app/develop/calling-frontend/)
- [Tauri 2 Window API](https://v2.tauri.app/reference/javascript/api/namespacewindow/)
- [Global Shortcut 插件](https://v2.tauri.app/plugin/global-shortcut/)
- [Window State 插件](https://v2.tauri.app/plugin/window-state/)

核验日期：2026-11-02。最终安装版本以锁文件为准。TypeScript 选择兼容 ESLint 工具链的 5.9 版本线。

## 当前实现与验证（2026-11-03）

五个主要页面、首次账号引导、三种英雄池模式、明确排除、位置和优先级编辑、历史筛选与月度图表、手动阵容修正、推荐依据、悬浮窗页面及识别诊断已连接 Tauri command 层。Hero/MatchRecord/统计 DTO 位于 `apps/desktop/src/types/index.ts`；数据库和算法不在组件中重复实现。

识别诊断仅在展开且捕获正在运行时每秒读取诊断对象，不轮询完整快照。事件到达正在读取的快照时，会排队补读，避免丢失最后一次更新。UI 通过外部 store 订阅数据，保留上次成功读取的快照，错误提示提供重试入口。

可视化校准支持本地图片、鼠标绘制 ROI、键盘数值精调、10 个头像槽位、选人标记及可选锁定/空槽标记。导出 ZIP 包含 profile.json 和截图实际裁剪 PNG；配置标记 `not_validated: true`。未提供锁定标记时不会生成自动锁定证据。图像不上传。

已执行 TypeScript 类型检查、ESLint、Vite 生产构建和 Chrome 浏览器检查。浏览器检查覆盖 1440、1024、375 像素宽度下的五个页面、空状态、输入错误、浏览器缺少 IPC 的提示、英雄纳入与排除互斥、设置校验、360 像素悬浮窗及校准包下载结构。没有注入虚构战绩。校准导出测试使用应用自身的截图，仅验证文件和坐标交互，不是 DOTA2 识别准确率证据。

- [真实空数据界面截图](images/ui-empty.png)
- 自动化脚本：`apps/desktop/tests/ui-smoke.mjs`
- 执行结果：`tests/ui/browser-smoke-result.json`

复现界面检查：

```bash
cd apps/desktop
npm ci
npx playwright install chromium
npm run test:ui
```

脚本自动启动本地 Vite；已有 `DPDA_UI_URL` 时使用指定服务器。可用 `DPDA_CHROME_PATH` 指定 Chromium 可执行文件。浏览器检查不替代 Tauri 原生命令与 Windows 游戏实测，PNG 下载在 Windows WebView2 中仍需验证。

## 前端依赖维护

TypeScript 5.9.3 与 typescript-eslint 8.72.1 的兼容范围一致。ESLint 使用 10.14.0。开发依赖链通过 `overrides` 固定 minimatch 11.0.1，以处理 [GHSA-g3hv-75wg-m249](https://github.com/advisories/GHSA-g3hv-75wg-m249)；安装后需保持 lint 和类型检查通过，再更新锁文件。当前 `npm audit` 未报告漏洞。此结果仅描述本次依赖扫描，不代表对所有代码的安全保证。
