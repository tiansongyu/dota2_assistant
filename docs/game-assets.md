# DOTA2 游戏图标

图标原始来源是 Valve 的公开 CDN，目录与稳定 ID 来自固定版本的 dotaconstants。默认构建从 GitHub Release 恢复已验证的固定快照。下载不需要 Steam 登录，也不调用 OpenDota 战绩 API。英雄、技能、物品和命石目录不是“所有条目当前都能在普通比赛中使用”的证明；旧技能、内部条目和已移除物品可能仍保留在数据源中。

2026-11-03 已实际下载 1,802 个图像文件，合计 55,247,287 字节（52.69 MiB）：127 张英雄肖像、127 张英雄小图、1,002 张技能图、458 张物品图、82 张命石图和 6 张辅助图。详细覆盖与测试见 [验收报告](reports/game-icons.md)。

## 下载与启动

在仓库根目录执行。Linux 使用 Python 3，Windows 可将 `python3` 换成 `python`。

```bash
python3 scripts/fetch-locked-game-icons.py
python3 scripts/fetch-game-icons.py --verify
npm --prefix apps/desktop run assets:verify
```

图像和完整清单保存在 `assets/game-icons/`。默认从仓库发布页恢复固定快照：先核对归档大小、SHA-256 和当前资源锁，再安全解包并逐文件检查。完整且符合锁的缓存可直接离线复用；失败下载不会替换已有缓存。也可在安装前端依赖后执行 `npm --prefix apps/desktop run assets:sync`。

不能联网时，在另一台机器下载 [资源快照](https://github.com/tiansongyu/dota2_assistant/releases/tag/game-icons-20261103-v1) 的 ZIP，复制到本机后运行：

```bash
python3 scripts/fetch-locked-game-icons.py --archive /path/to/game-icons-50bd326d0c66.zip
```

安装前端依赖后，通过桌面入口启动：

```bash
npm ci --prefix apps/desktop
npm --prefix apps/desktop run tauri dev
```

只查看资源页面也可运行 `npm --prefix apps/desktop run dev`，打开终端显示的地址。浏览器预览可浏览本地图标；真实战绩操作仍需桌面程序。主导航的“游戏资源”支持分类、搜索、分页、来源和缺失记录查看。

总览、英雄池、历史、BP 和推荐中原有英雄标记使用本地图像。损坏或缺失时保留文字回退；应用不把缺图请求转发到外部网站。下载完成后，资源浏览与图像展示不需要网络或账号。

## 资源范围与解释

| 类别 | 依据与用途 |
| --- | --- |
| 英雄肖像 | 按稳定 `hero_id` 保存，适合英雄卡片 |
| 英雄小图标 | Valve 小图版本，适合紧凑列表 |
| 技能 | 技能目录与 ID 表合并；英雄关联来自 `hero_abilities`，不按名称前缀推测 |
| 物品 | 物品目录与 ID 表合并；配方可以共享 Valve 的配方卷轴图 |
| 命石 | 从英雄能力目录提取图标 key，保留来源中的废弃标记，不能视为当前可选命石清单 |
| 辅助图标 | 属性、先天能力与通用天赋图标 |

天赋使用官方通用天赋树图时标记 `shared`，不将其描述为每个天赋独立绘制的图标。配方共享图也保留各自物品 ID。目录记录数、下载成功映射数和实际图片文件数分别统计，不能混称为“下载了多少张图片”。

本次 4,164 条目录记录中，3,749 条能显示本地图像，391 条对应地址返回 404，24 条为无独立图像的占位或内部字段。1,811 条天赋映射到同一张通用树图，137 条配方映射到同一张卷轴图；均不重复计算图片数量。缺失项包含当前英雄关联的先天能力，不能统称为过时内容。

`manifest.json` 每条资源包含类别、稳定 ID 或 key、真实名称、来源 URL、相对路径及状态；成功文件另含 SHA-256、大小、尺寸和时间。状态为 `downloaded`、`not_found`、`skipped` 或 `failed`。缺图和不适用条目保留原因，不用随机图片或相似英雄顶替。

## 构建与更新

Vite 从仓库 `assets` 读取公开静态资源，将 `game-icons` 原样放入 `dist/game-icons`。Tauri 再随前端一起打包；不另加一份 `bundle.resources`，不要求最终用户联网逐张加载。CSP 只增加同源清单访问，不开放任意 CDN。

图片和原始目录不写入 Git；经过验证的 ZIP 保存在仓库的独立资源发布页。`assets/game-icons.lock.json` 锁定每张图片的来源、SHA-256 和大小，`assets/game-icons.snapshot.json` 锁定 ZIP、下载地址及对应锁文件。CI 与默认开发命令使用同一快照，不逐张访问可变的 Valve CDN。构建仍核对本地图片与锁，不能通过刷新锁自动接受远端变化。

首次构建需要下载快照；已有有效缓存后可离线构建。Valve CDN 可能在不同区域返回不同版本。2026-11-03 的 Windows 构建曾遇到跨域跳转和四张图片字节不同，因此新增固定快照恢复，见 [故障记录](reports/game-icons-build-fix.md)。

原始 CDN 下载器保留给资源维护者。它支持限流、重试、缓存和中断恢复；已确认的 404 只有 `--retry-missing` 才重新请求。Ctrl-C 取消未开始任务，保存未完成清单并以 130 退出。若要更新已缓存的成功图片，请使用新的暂存目录；仅加 `--retry-missing` 不会重新下载成功文件。

维护者在检查新目录和下载结果后更新资源锁，生成新的快照标签；下面的标签仅为命令示例，不应覆盖已发布快照：

```bash
python3 scripts/fetch-game-icons.py --output artifacts/new-game-icons --retry-missing
python3 scripts/fetch-game-icons.py --output artifacts/new-game-icons --verify
# 审阅覆盖变化并将完整暂存目录替换到 assets/game-icons 后：
python3 scripts/lock-game-icons.py
python3 scripts/pack-game-icons.py --release-tag game-icons-YYYYMMDD-vN
npm --prefix apps/desktop run assets:verify
```

`pack-game-icons.py` 先执行构建校验，再生成 ZIP 和新的快照描述文件；不自动上传或提交。将 ZIP、摘要及权利说明上传到描述文件所指的 GitHub Release，确认空目录恢复通过，再提交锁、描述文件、来源和变更记录。不得覆盖已有发布资源。`assets` 是公开静态目录，不得放入账号数据库、访问密钥、个人截图或日志。

## 来源与图像权利

- [dotaconstants](https://github.com/odota/dotaconstants) 提供元数据及 ID；其 MIT notice 保留在 `assets/licenses/dotaconstants-MIT.txt`。
- 英雄、技能、物品等图像来自 `https://cdn.cloudflare.steamstatic.com/apps/dota2/images/dota_react/`，每个实际 URL 见清单。
- DOTA2 图像权利属于 Valve 及相应权利人，不适用本项目原创代码的 MIT 许可。DPDA 与 Valve 无隶属或背书关系；尚未取得单独的商业图像再分发授权。

本地下载并接入应用不代表取得无限商业再分发权。公开服务或正式商业安装包使用前，需确认相应使用条件。现有开发构建记录应明确包含哪些图像，而不能继续声称安装包不含 Valve 美术。

这些网站图标用于 UI 展示。英雄肖像可作为视觉实验输入，但不是已标注的 DOTA2 选人截图，也不能证明锁定识别准确率；视觉验收仍见 [vision-system.md](vision-system.md)。
