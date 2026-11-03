# v0.1.0-dev.1 发布核验

日期：2026-11-03。发布地址：[GitHub 开发预览版](https://github.com/tiansongyu/dota2_assistant/releases/tag/v0.1.0-dev.1)。

源码 tag 指向 `d276485cb1569024e105d9a4201d76cfbbddae69`，与两个构建工作流的提交一致。应用内部版本为 `0.1.0`；GitHub 标记为 prerelease。

## 构建与下载产物

| 文件 | 字节数 | 来源 |
| --- | ---: | --- |
| `dpda-windows-x64-setup.exe` | 19,321,775 | [Windows build 41573643582](https://github.com/tiansongyu/dota2_assistant/actions/runs/41573643582) |
| `dpda-linux-debug-x86_64.tar.gz` | 3,824,090 | [Quality gates 41573643644](https://github.com/tiansongyu/dota2_assistant/actions/runs/41573643644) |

SHA-256：

```text
4b6c4373c822697a8270f37952e3095cb4f0907da1c1cc2ec7c3d5734a1628da  dpda-windows-x64-setup.exe
306bc4262c4370bda1e3698f669780b725c2bc55238e3bb9710e2a24868c3829  dpda-linux-debug-x86_64.tar.gz
```

Windows CI 文件原名为 `Dota Personal Draft Assistant_0.1.0_x64-setup.exe`。发布时仅改名，二进制字节与 CI 摘要不变；发布用校验文件采用新名称。Linux 包与 CI 产物一致。

## 执行结果

- 两个平台均通过 50 项 Rust 测试、6 项 Python 许可收集测试、Rust fmt/严格 Clippy、TypeScript、ESLint 和前端构建。
- Windows CI 的 NSIS 静默安装退出码为 0，安装目录有 `opencv_world4110.dll`；随后从安装目录启动 `dpda-desktop.exe`，8 秒后进程仍存活。
- 下载 Windows 产物后核对 SHA-256，并通过 NSIS 文件清单确认 EXE、OpenCV DLL、OpenCV 许可目录、`DPDA-LICENSE.txt` 和 `THIRD_PARTY_LICENSES.txt` 均存在。
- 下载 Linux 产物后，`sha256sum -c` 通过。归档包含 CLI、Linux 使用说明、项目 MIT 和第三方许可；实际解压执行 `doctor`，SQLite schema 1 初始化与自检通过。
- Linux 二进制动态依赖仅有系统 C 运行库相关库；没有 GTK、WebKit 或 OpenCV。

## 不能由本次结果证明的事项

Windows smoke 仅确认安装退出状态、DLL 文件存在和进程存活，没有检查 Windows WebView 渲染、IPC、数据库或界面交互。CI runner 预装 VC++/WebView2，并仍有开发 OpenCV 目录在 PATH 中，因此不属于清洁用户环境测试。Linux 原生 IPC 是另一次独立测试，不能替代 Windows IPC 验证。

安装包未签名，VC++ x64 Runtime 不会自动安装。OpenCV 导入 `MFPlat.dll`、`MF.dll`、`MFReadWrite.dll`，Windows N 或精简系统的媒体组件依赖未验证。WebView2 缺失时需网络下载官方 bootstrapper。

真实 DOTA2 标注截图为 0，游戏识别准确率、四种分辨率、DPI、多显示器、独占全屏与 FPS 均未验收。开发预览版先用于真实数据同步、手动 BP、校准和工程调试，不作为生产验收通过的声明。

更完整的证据和限制见 [测试记录](../testing.md)、[原生测试](native-smoke.md)、[合成性能](synthetic-performance.md) 和 [已知问题](../known-issues.md)。
