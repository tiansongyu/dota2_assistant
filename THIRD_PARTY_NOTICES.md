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
| Valve DOTA2 英雄、技能、物品及辅助图标 | 从官方 CDN 下载至生成目录，构建时随前端嵌入应用；经校验的 ZIP 另存资源发布页，图像不写入 Git | Valve 及相应权利人保留图像权利；来源和哈希见资源清单，不适用本项目 MIT，尚未取得单独商业再分发授权 |
| OpenDota API 响应 | 最小真实测试 fixture | 公开历史，保留来源/日期；不包含聊天、昵称、访问凭证 |

参考仓库仅用于研究。`bijankle/Dota_Draft_Assist` 未发现 LICENSE，不复制其代码；`gestevao04/dota-counterpick-overlay` 的 MIT 声明及局限见 [研究记录](docs/research-capture.md)。Outdraft 只作公开功能参考，不分发其安装包或页面素材。

图标下载、打包、共享天赋/配方、缺失状态与更新方法见 [游戏资源说明](docs/game-assets.md)。旧 `v0.1.0-dev.1` 发布包未包含本次新增的图标资源；新构建的内容应以对应提交与资源锁文件为准。

## 二进制随附许可文本

`scripts/collect-licenses.py --strict-normal --output artifacts/THIRD_PARTY_LICENSES.txt` 收集已安装 Cargo 依赖及完整 npm 生产依赖的原始许可文本，按 SHA-256 去重并保留包名、版本、源码下载地址。Windows 安装目录及 Linux 调试压缩包随附该文件和 DPDA 的 MIT LICENSE。

Cargo 清单包含构建、测试和其他平台依赖，是分发范围的超集。脚本分别核对 Windows 桌面与 Linux CLI 的 normal 依赖树；`--strict-normal` 在这些树或 npm 生产依赖缺少原文时失败。超集中的其他缺口仍显示在报告中，不能把清单存在当成法律审核结论。

上游发布包缺少许可文件的已审阅补充材料位于 `assets/licenses/supplements.json`，保留来源、精确源码 revision（如适用）和 SHA-256；MPL 依赖保留其声明、标准许可证文本及可获得的精确版本源码地址。没有修改第三方代码的许可证或版权声明。OpenCV 官方二进制另附自身 LICENSE 与官方第三方许可目录。
