# 第三方代码与素材

DPDA 原创代码见根目录 MIT LICENSE。以下许可仅覆盖各自项目，不意味着服务数据或 Valve 美术可以自由再分发。

| 来源 | 使用方式 | 许可/说明 |
| --- | --- | --- |
| Tauri / plugins | 桌面与窗口管理 | MIT OR Apache-2.0，详见依赖包 |
| React、Vite、TypeScript | 前端构建 | React/Vite MIT；TypeScript Apache-2.0 |
| Rust crates | HTTP、SQLite、日志、统计支持 | 精确版本见 Cargo.lock；依赖许可证以各包 metadata 为准 |
| windows-capture 2.0.1 | Windows Graphics Capture 包装 | MIT；仅使用窗口捕获与只读窗口元信息 |
| OpenCV 与 Rust binding | 图像匹配 | OpenCV 4.x Apache-2.0；Rust binding MIT；Windows 包附 OpenCV license |
| lucide-react | UI 图标 | ISC |
| odota/dotaconstants | 英雄与 patch 静态目录 | MIT；原 notice 位于 assets/licenses/dotaconstants-MIT.txt |
| Valve DOTA2 肖像 | 用户主动下载到本地缓存 | 不随 Git/安装包发布，不声明取得再分发许可 |
| OpenDota API 响应 | 最小真实测试 fixture | 公开历史，保留来源/日期；不包含聊天、昵称、访问凭证 |

参考仓库仅用于研究。`bijankle/Dota_Draft_Assist` 未发现 LICENSE，不复制其代码；`gestevao04/dota-counterpick-overlay` 的 MIT 声明及局限见 [研究记录](docs/research-capture.md)。Outdraft 只作公开功能参考，不分发其安装包或页面素材。
