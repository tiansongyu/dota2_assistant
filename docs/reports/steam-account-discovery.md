# 本机 Steam 账号发现验收

日期：2026-11-03。目标：减少首次使用时手工查找、复制 Steam ID 的步骤，同时不把本机记录视为身份认证。

## 已验证

| 检查 | 结果 | 证据边界 |
| --- | --- | --- |
| Rust Workspace | 92 项测试通过；fmt、严格 Clippy 通过 | 含 19 项 Linux 账号发现合成测试及 2 项 CLI 集成测试 |
| Windows 编译检查 | `steam-discovery` 的 Windows GNU all-targets 检查通过 | 未实际运行 Windows 注册表或文件共享逻辑 |
| Python 工具 | 70 项测试通过 | 原生脚本的实际执行另列 |
| 前端 | TypeScript、ESLint、Prettier、Vite 与 Tauri debug 构建通过 | 构建未替代交互检查 |
| 专项浏览器 | 6 组测试通过，外网请求与 JS 异常均为 0 | 显式合成 IPC，仅证明界面状态与交互 |
| 既有浏览器回归 | 15 个页面/视口组合与表单、设置、校准、悬浮窗检查通过 | 空数据浏览器，非真实 Steam 或 DOTA2 |
| Linux 本机 CLI | 实际读取本机 Steam 记录成功；一个去重候选，建议选择可用；VDF 内容未改动 | 报告只保留计数与状态，不提交账号、昵称或原 VDF |
| Linux 原生 Tauri | 33 项检查通过，包含真实 IPC 发现、精确 Steam64、发现不改账号、点击候选保存 | 在隔离应用数据库中选择实际候选，再切回明确公共测试账号；未调用战绩 API |
| 图标与许可 | 1,822 个公开资源逐文件验证；生产依赖许可原文收集缺口为 0 | 未改变已固定的图标锁与商业授权状态 |

合成测试覆盖 BOM、Unicode、转义、未知字段不输出、重复键、大小/深度/节点/账号限额、时间戳范围、安装路径去重、最近使用歧义、部分读取失败、文件链接与非普通文件。Linux 的安全读取测试不证明 Windows 分支已在 Steam 客户端中运行。

界面测试覆盖预选、无建议时不擅自选择、键盘操作、检测失败重试、手动输入、保存失败、已有账号切换，以及保存账号后不自动同步。1440、1024、375 宽度均无页面横向溢出，超长昵称与 HTML 字样按普通文本显示。

原生测试中，发现结果未自动写入配置；点击候选后保存生效。测试报告和截图不包含实际发现的账号或昵称，实际选择仅发生在隔离应用目录。发现模块未扫描 `userdata`、读取密码文件、初始化 Steamworks、访问游戏内存或发起 HTTP 请求。

## 复查

```bash
cargo test --locked -p dpda-steam-discovery -p dpda-debug
cargo check --locked -p dpda-steam-discovery --all-targets --target x86_64-pc-windows-gnu
npm --prefix apps/desktop run test:steam
node apps/desktop/tests/ui-smoke.mjs
```

实际本机检查使用 `target/debug/dpda-debug steam-accounts`。它会显示当前用户的本机候选，输出不应直接复制到公开 issue。原生检查在具备 Linux WebDriver 依赖的环境执行：

```bash
python3 scripts/native-smoke.py --application target/debug/dpda-desktop \
  --account PUBLIC_TEST_ACCOUNT --game-assets --steam-discovery
```

本次本地报告位于 `artifacts/steam-discovery/`，不提交 Git。专项界面脚本会写入该目录的 `ui-synthetic` 子目录，素材和昵称均明确标为合成测试。

## Windows 与 Linux CI

提交 `c427b02` 的 [Windows CI](https://github.com/tiansongyu/dota2_assistant/actions/runs/41614855078) 与 [Linux CI](https://github.com/tiansongyu/dota2_assistant/actions/runs/41614855026) 均通过。Linux CI 也实际执行了新增的六组账号界面回归。Windows 完成严格 Clippy、Rust/Python/Node 测试、NSIS 构建、静默安装和安装后进程启动检查。

已下载 Windows 安装包并与 CI 的摘要文件核对一致：

- 文件：`Dota Personal Draft Assistant_0.1.0_x64-setup.exe`
- 大小：62968539 字节
- SHA-256：`f7feecbff5161a845146cce99a15fb64b5044d06287578685960b1a832c70c59`

安装包位于该 Windows 工作流的 `dpda-windows-nsis-unsigned` artifact。旧 `v0.1.0-dev.1` 应用下载包未替换。

## 待验证

Windows 实机上 Steam 登录切换、退出后的记录更新、权限差异和并发写入仍待验证；Flatpak 安装目录也未做真实安装验收。`MostRecent` 只是本机记录提示，不能证明当前登录状态。此前 DOTA2 选人识别、性能与清洁机器验收边界不变。
