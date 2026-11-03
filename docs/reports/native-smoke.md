# Linux 原生 Tauri 集成测试

执行时间：2026-11-03 03:17 UTC。最新前端和 Rust 代码先重新构建为真实 Tauri debug 应用，再通过 `tauri-driver` + WebKitWebDriver 操作原生窗口。共 25 项检查、26 次真实 IPC 调用通过，其中 5 次非法输入或不支持平台的调用按预期拒绝。没有替换或模拟 `invoke`，没有以普通浏览器预览代替桌面程序。

## 环境与复现

Ubuntu 22.04 x86_64，glibc 2.35，Rust 1.97.1，应用由 wry 0.57.1 / WebKitGTK 驱动。测试使用 Xvfb、独立 D-Bus 会话及隔离的 XDG 目录；原数据库以只读方式打开，使用 SQLite backup 复制到测试目录。

```bash
npm --prefix apps/desktop run tauri -- build --debug --no-bundle

python3 scripts/native-smoke.py \
  --application target/debug/dpda-desktop \
  --tauri-driver /path/to/tauri-driver \
  --native-driver /path/to/WebKitWebDriver \
  --account 70388657 \
  --fixture-db /tmp/dpda-network-smoke.sqlite \
  --output artifacts/native-smoke-compact-final
```

`--output` 必须是尚不存在的目录，防止覆盖测试证据。不提供它时创建独立临时目录。`--fixture-db` 可省略，此时只测试空数据库与手动配置；有真实历史时才能验证实际战绩回读。`--account` 必须显式提供，不是应用的默认账号。测试脚本仅依赖 Python 标准库，额外运行工具为 Xvfb、D-Bus、tauri-driver、WebKitWebDriver 和可运行的 Tauri 二进制。

本次数据来自此前真实 OpenDota 冒烟测试的 4 场公开比赛，见 [历史导入实测](history-smoke.md)。英雄位置和池名单是测试明确设置的输入，不代表推断出的玩家常用位置。测试过程中外部数据 API 请求数为 0。

本次应用 SHA-256：`978e83a2bf873054eef388d8e844bc6fda1f2e4f07716e77c37c54d9a9dbf252`。构建后若再修改代码或前端资源，需要重新构建并重跑，不能沿用这个结果。

## 已验证行为

| 范围 | 实际检查 |
| --- | --- |
| 启动与连接 | 原生页面来源为 `tauri://localhost`，真实 IPC 可调用；快照包含最新 `automatic_pool` 和 `pool_candidates` 字段 |
| 账号 | 非法 ID 被拒绝；有效 Steam64 字符串精确转换，保存后前端收到事件并刷新账号 |
| 数据 | 从隔离副本回读 4 场真实比赛；重启后数量不变 |
| 英雄池 | 保存手动池及位置；未知英雄被拒绝且原配置保持完整 |
| 阵容与推荐 | 敌方已选、明确排除和禁用约束生效；重复英雄被原子拒绝，原阵容不变 |
| 设置 | 主题、采样频率、Top-K 和紧凑标志保存；无效 Top-K 被拒绝且设置不变 |
| 平台边界 | Linux 启动 Windows Capture 返回明确错误，主程序继续响应 |
| 悬浮窗 | 两次打开只创建一个 overlay；原生主窗和悬浮窗共 2 个；悬浮窗显示真实推荐快照 |
| 窗口尺寸 | 紧凑窗初始高度 420；手动把宽度调至 480 后，展开高度变为 620，再切回紧凑高度 420，两次切换均保留宽度 480 |
| 点击穿透 | 本环境恢复快捷键已注册，可以启用穿透，再从主窗口 IPC 关闭；没有模拟游戏输入 |
| 重启 | 账号、英雄池、设置和历史持久化；临时阵容不错误保留到下一次启动 |
| 数据库 | 操作结束后 `PRAGMA quick_check` 为 `ok` |

主窗口 1360×900、紧凑悬浮窗 420×420 均保存了原生截图 `main.png`、`overlay.png`，并人工查看了截图。窗口调整后的 480×420 悬浮窗也通过页面横向溢出检查。完整 JSON、IPC 时序及驱动日志位于本地 `artifacts/native-smoke-compact-final/`；该目录被 Git 忽略，避免提交个人数据库或运行日志。

这次 26 次 IPC 的观测往返耗时为 1.124–97.310 ms，包括 WebDriver、渲染进程和命令处理。样本规模很小、数据库只有 4 场；这些数值不是性能基准，也不能证明 Windows 游戏中的开销。

## 排障与未验收事项

最初直接使用宿主共享 runtime 目录启动，D-Bus 激活 `org.freedesktop.secrets` 等服务时等待，客户端建立会话超时。改用独立 `XDG_RUNTIME_DIR`（权限 0700）、独立 data/config/cache、X11、内存 GSettings、本地 VFS 和 WebKit 软件渲染环境后，会话建立和完整测试均通过。没有更改用户全局环境或系统包；未逐个隔离变量，因此不把恢复归因于单一环境变量。

日志仍有 headless 环境的 PipeWire 连接提示和 `g_dbus_proxy_new` GLib 警告；本次命令、窗口和重启检查没有因此失败。它们被保留在驱动日志，不能据此宣称所有 Linux 桌面环境都没有问题。

首轮截图发现紧凑模式仍保留 620 高度，产生大块空白背景。应用已改为紧凑高度 420、展开高度 620，并在切换时保留用户设置的宽度；本次重新构建后验证通过。这是固定高度模式，尚未实现按推荐条数自动收缩窗口。

一次中间运行在连续调整窗口后执行 WebDriver 截图超时，之前的 20 项检查均已通过。最终脚本在首次紧凑窗尺寸稳定后截图，再执行尺寸切换检查，完整运行通过。中间报告保留于本地 `artifacts/native-smoke-compact/`，不将截图驱动超时隐去或计作通过。

Windows 全局快捷键实际按键、点击穿透对游戏操作的影响、多显示器/DPI、独占全屏、屏幕捕获、真实截图识别精度、FPS、长时间运行和安装包均不在本次 Linux 测试的已验证范围。

WebDriver 接入依据：[Tauri 官方说明](https://v2.tauri.app/develop/tests/webdriver/)和[官方 Selenium 示例](https://v2.tauri.app/develop/tests/webdriver/example/selenium/)。本项目脚本直接使用 W3C HTTP 协议，没有增加 Selenium 运行依赖。
