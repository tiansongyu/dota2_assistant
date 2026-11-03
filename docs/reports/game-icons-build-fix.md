# Windows 图标构建故障修复

日期：2026-11-03。

## 失败证据

提交 `9a76079` 的 [Windows 工作流 41603764173](https://github.com/tiansongyu/dota2_assistant/actions/runs/41603764173) 在图标下载步骤退出 1，未进入 Rust 编译与安装包构建。同提交的 Linux 工作流通过。

Windows 清单记录 `items/arcane_boots` 下载失败，原因是 `cross-host or insecure redirect rejected`。旧日志没有保存跳转目标，不能确定当时的完整跳转链；本机随后请求相同 URL 直接返回 200。

逐条比较两个平台的真实清单，还发现四张已下载图片的 SHA-256 不同。即使重试解决秘法鞋下载，原有资源锁仍会阻止构建：

| 图片 | 本地锁定字节数 | Windows 下载字节数 |
| --- | ---: | ---: |
| `abilities/pugna_decrepify.png` | 32,826 | 28,878 |
| `abilities/pugna_nether_ward.png` | 31,852 | 26,084 |
| `items/consecrated_wraps.png` | 16,920 | 15,050 |
| `items/prophets_pendulum.png` | 16,083 | 16,808 |

元数据固定到 Git commit，并不意味着 Valve CDN 图像 URL 的内容也不可变。本次没有用 Windows 的不同图片覆盖原锁，也没有关闭哈希检查。

## 修改

- 默认构建改为恢复已验证的资源 ZIP。归档 SHA-256、字节数和对应锁文件 SHA-256 写入 `assets/game-icons.snapshot.json`，Windows、Linux 与 `assets:sync` 共用。
- 恢复脚本先校验归档，再解包至暂存目录，检查全部元数据、图像、来源锁及文件集合后才替换缓存。完整缓存可离线使用；本地 ZIP 支持离线恢复。
- 原 CDN 下载器继续用于显式更新。图片仅允许四个已核实的 Valve CDN 主机间 HTTPS 同路径跳转，拒绝陌生域名、协议降级、用户信息和异常端口；元数据不继承图片域名规则。错误记录来源和目标 origin，不记录凭证或查询参数。
- 增加重复路径、目录逃逸、符号链接、归档损坏和锁不匹配等回归检查。权利声明随资源发布，图像仍不适用代码 MIT 许可。

四个图片主机为 `cdn.cloudflare.steamstatic.com`、`cdn.steamstatic.com`、`cdn.akamai.steamstatic.com` 和 `cdn.fastly.steamstatic.com`。依据是 [Valve 官方英雄页面](https://www.dota2.com/hero/axe) 的运行配置，以及其引用的 [官方 JavaScript](https://www.dota2.com/public/javascript/dota_react/main.js?v=K0KE9f72Ct4H&l=english&_cdn=akamai) 中的 CDN 列表。它们不是 Windows 原跳转链的复现证据。

固定快照保留 1,802 个图像文件、55,247,287 字节；ZIP 为 48,810,988 字节。图像与原资源锁一致，391 个 404 和 24 个跳过条目保持原状。

## 验证状态

本地验证结果：

- 71 项 Rust、70 项 Python、16 项 Node 测试通过；Python 新增 5 项 CDN 跳转和 24 项快照恢复测试。
- `cargo fmt`、严格 Clippy、TypeScript、ESLint、Prettier 与 Vite 构建通过。
- 真正的 48,810,988 字节 ZIP 在空目录离线恢复成功，1,802 个图像与旧锁一致；`assets:sync` 对已有缓存不发网络请求。
- 开发源与生产 `dist` 的 1,822 个公开文件逐文件校验通过；原图像锁文件没有改动。

真实 GitHub Release 网络冷恢复成功，随后缓存离线复验成功。新 Windows/Linux 工作流结果在修复提交验证后补录。Windows 游戏画面识别、FPS、清洁机器安装和 WebView 内容验收仍未完成，构建成功不代表这些项目通过。
