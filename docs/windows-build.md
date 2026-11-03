# Windows 构建与安装包

适用：Windows 10 1903 或更新版本的开发机，推荐 Windows 11 x64。WGC API 的最低版本不表示所有系统和显示模式已经验证。

## 开发依赖

- Rust stable MSVC，至少 1.90；Visual Studio C++ Build Tools 和 Windows SDK。
- Node.js 24；Edge WebView2 Runtime；Microsoft Visual C++ 2015—2022 x64 Redistributable（OpenCV 动态库的运行依赖）。
- LLVM/libclang 和 OpenCV 4.x。CI 固定使用官方 OpenCV 4.11.0 Windows 包，链接 `opencv_world4110`。

依赖依据：[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)、[opencv-rust 安装指南](https://github.com/twistedfall/opencv-rust/blob/v0.101.1/INSTALL.md)。

## 配置 OpenCV

从 [OpenCV 4.11.0 官方 release](https://github.com/opencv/opencv/releases/tag/4.11.0) 下载并解压 Windows 包。以下示例假设目录为 `C:\opencv`：

```powershell
$env:OPENCV_INCLUDE_PATHS = 'C:\opencv\build\include'
$env:OPENCV_LINK_PATHS = 'C:\opencv\build\x64\vc16\lib'
$env:OPENCV_LINK_LIBS = 'opencv_world4110'
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'
$env:PATH = 'C:\opencv\build\x64\vc16\bin;' + $env:PATH
npm ci --prefix apps/desktop
cd apps/desktop
npm run tauri dev
```

路径应指向实际安装目录。版本不同则修改 lib/dll 名称，不能只改一个版本字符串后假定兼容。

## 打包

安装包需要带上 OpenCV DLL，并保留其 license，不能只在开发机 PATH 中找到 DLL 就认为新机器可运行。`.github/workflows/windows.yml` 展示获取官方 OpenCV、执行全部检查、生成 `bundle-windows.json` 资源映射和构建 NSIS 的完整步骤。

```powershell
# 在 apps/desktop 下，按工作流准备 src-tauri/runtime 和资源配置后：
npm run tauri -- build --config src-tauri/bundle-windows.json
```

输出位于仓库 `target/release/bundle/nsis/`。Tauri 为当前用户安装，WebView2 缺失时使用官方 bootstrapper。当前包不自动安装 VC++ Redistributable，目标机器需先安装微软官方 x64 运行库；其许可及安装方式见 [Microsoft 部署说明](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170)。开发包未签名；不应关闭安全软件或把安全提示当作质量证明。

## 必须追加的人工验收

在没有 Rust、OpenCV 开发目录和系统 PATH 配置的 Windows 用户环境安装，检查应用能启动、WebView2 安装、数据库写入、真实公开战绩导入、手动 BP、关闭再打开、卸载行为。随后在真实 DOTA2 中验证 WGC、校准、锁定识别、悬浮窗和资源占用。

自动构建只证明相应 runner 的编译和打包；不代表这些人工项已经通过。结果记录到 [testing.md](testing.md)，包含 OS/build、显卡/驱动、DPI、窗口模式、分辨率和 commit。
