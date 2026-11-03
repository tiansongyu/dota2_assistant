# 游戏图标下载与集成验收

日期：2026-11-03。环境：Ubuntu 22.04、Python 3、Node 24、Chromium、Linux WebKitGTK / Tauri。素材来自真实 Valve CDN，测试没有用生成图或假英雄图替代资源。

## 来源与数量

固定元数据：`odota/dotaconstants` commit `5c0242df9057848fa2036ca3fafe9f2bd4508163`，版本 10.8.0。读取英雄、技能、物品、ID 和英雄能力关系六份目录及 package 元数据，逐文件保存 SHA-256。Valve 官网英雄列表实测为 127，与现有稳定英雄 ID 集合一致。

处理 4,164 条资源映射、2,193 个去重图像任务。取得 1,802 个物理图像文件，共 55,247,287 字节，约 52.69 MiB。

| 文件目录 | 物理文件数 | 说明 |
| --- | ---: | --- |
| `heroes` | 127 | 全部目录英雄的 256×144 肖像 |
| `hero-icons` | 127 | 全部目录英雄的 32×32 小图 |
| `abilities` | 1,002 | 技能 PNG，不含下面的共用天赋 SVG |
| `items` | 458 | 含一张共用配方卷轴 |
| `facets` | 82 | 目录中的命石图案，不能视为当前可选命石全集 |
| `ui` | 6 | 四种属性、先天能力通用标记、通用天赋树 SVG |

3,749 条映射有本地图像，391 条返回 404，24 条明确跳过，传输或图片验证失败为 0。1,811 条天赋共享同一个 SVG，137 条配方共享一张 PNG，因此映射数不能用作图片数。

缺失包括 390 个技能 key 和物品 `claddish_spyglass`。其中 57 个缺图技能在 `hero_abilities` 中具有关联；仅凭关联不判断当前游戏可用性。对反法师、斧王和孽主的官方数据抽检确认，一些先天能力仍然存在，但未取得对应独立图片。现代目录与少量有依据的旧目录补查都未提供这些图，未改用其它技能图片冒充。

组合技能 ID `3060,1617` 被保存为 `ids: [1617, 3060]`，单一 `id` 保留空值。原始命石废弃标记包含混合类型，未强行改写为“当前可选”清单。

## 自动化和实际运行

| 检查 | 实际结果 |
| --- | --- |
| Python 下载器与已有工具 | 41 项测试通过，其中新增 23 项覆盖真实组合 ID、PNG CRC/解压/截断、SVG、429、404、HTML 假图片、缓存损坏和原子写入 |
| Node 构建门禁 | 12 项测试通过；不完整清单、损坏、路径冲突/逃逸、锁不符、未声明和敏感文件均被拒绝 |
| 实际下载恢复 | 在保存 650 个任务后停止进程，再启动；452 个成功文件和 198 个 404 从缓存恢复，继续完成全部任务 |
| Ctrl-C 取消 | 真实子进程 SIGINT 与本地 HTTP 测试中，6 个任务只有已开始的 2 个发出请求，其余 4 个取消；退出 130、保存未完成清单，重启不重复下载已成功文件 |
| 文件验证 | 下载器离线校验通过；Pillow 完全解码全部 1,801 张 PNG；SVG 经静态校验及浏览器实际解码 |
| 资源锁 | 1,802 个去重路径的来源、SHA-256、大小与本地文件一致 |
| Rust 门禁 | 71 项测试、fmt、严格 Clippy 通过 |
| 前端门禁 | TypeScript、ESLint、Prettier、Vite build 通过 |
| 开发与生产浏览器 | 六类资源首尾分页、图片解码、搜索/组合 ID、英雄技能关联、缺失和坏图回退通过；1440/1024/375 宽度无溢出；禁止外网请求后仍通过，外网请求与 JS 异常均为 0 |
| 静态打包 | `dist/game-icons` 图片与锁一致；公开静态目录共 1,821 个文件通过逐文件检查，无私有数据库或日志 |
| Linux 原生 Tauri | `tauri build --debug --no-bundle` 成功；29 项实际 WebDriver 检查通过，包括 `tauri://localhost` 下清单、CSP、六类 PNG/SVG、无账号资源页和英雄标记 |

故障测试的合成 PNG、HTTP 错误和损坏响应明确仅用于验证错误处理；资源验收读取真实下载清单。原生测试使用独立空 SQLite 与明确的公开测试账号配置，没有调用战绩 API，也没有打开用户个人数据库。

## 复查入口

```bash
python3 scripts/fetch-game-icons.py --verify
npm --prefix apps/desktop run assets:verify -- --require-lock --check-dist
npm --prefix apps/desktop run test:assets
node apps/desktop/tests/game-assets-smoke.mjs
```

真实下载报告位于本机 `artifacts/game-icons/download-report.json`；浏览器报告与截图在 `artifacts/game-icons/ui/`；原生报告在 `artifacts/game-icons/native/`。这些路径不提交 Git。可公开重建的资源锁为 [game-icons.lock.json](../../assets/game-icons.lock.json)，操作说明见 [游戏资源](../game-assets.md)。

随后 Windows 构建发现 CDN 重定向和跨平台字节差异，修复及验证见 [构建故障记录](game-icons-build-fix.md)。本次没有验证 Windows WebView 的图像渲染；Windows CI 构建与安装结果需按对应提交另查。旧开发预览安装包未更新。图标下载和 UI 显示不证明 DOTA2 选人识别准确率、锁定识别或游戏 FPS；原有真实游戏验收状态不变。图像版权仍归 Valve 及相应权利人，不因打包或源码 MIT 获得单独商业授权。
