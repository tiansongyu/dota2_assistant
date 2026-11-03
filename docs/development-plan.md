# 开发计划与验收门禁

按用户指定顺序推进；某阶段的硬件验收被外部条件阻塞时继续完成可独立验证的后续实现，但保留该阶段未通过的状态。

| 阶段 | 工作 | 验收证据 |
| --- | --- | --- |
| Phase 0 | 官方资料、API 真响应、参考仓库代码/许可证、架构 | 研究文档与带来源 fixture |
| Phase 1 | Workspace、Tauri2、React、SQLite、日志、检查工具 | Rust/TS 门禁通过、桌面启动 smoke |
| Phase 2 | Steam ID、分页同步、断点、个人池、历史页 | 真 API 导入与 SQLite 回读；失败/重试/去重测试 |
| Phase 3 | WGC、OpenCV、状态机、校准、人工修正 | 算法测试加真实截图精确率/召回率、Windows 捕获验收 |
| Phase 4 | 过滤、平滑、对抗/协同、解释、回测 | 单元测试和严格时间回测；不夸大反事实效果 |
| Phase 5 | 悬浮窗、快捷键、位置恢复、后台频率 | Windows 游戏实测、资源和 FPS 报告 |
| Phase 6 | 安装包、文档、隐私、回归 | Windows 安装包产物与清洁机验收 |

每轮执行 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`npm run typecheck`、`npm run lint`、`npm run build`。前端命令位于 `apps/desktop`。Windows/OpenCV 功能按平台另行检查。网络 smoke 必须显式运行，常规测试不消耗外部 API 配额。

不新增无关组件 skill、云服务、移动端、对手查询或游戏自动操作。详细实际进展见 [development-progress.md](development-progress.md)。
