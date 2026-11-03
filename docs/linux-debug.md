# Linux 数据与界面调试

`dpda-debug` 在 Linux 命令行中验证公开历史接口、同步 SQLite、检查个人英雄统计和执行时间顺序回测。它调用与桌面相同的 Rust 业务模块，不依赖 GTK、WebKit、OpenCV、Node 或显示服务器。游戏画面捕获和悬浮窗仍属于 Windows 桌面应用。

## 打开已有数据的图形界面

当前 `main` 提供 Linux 图形调试启动脚本；它不包含 Linux 游戏捕获。先按 [README](../README.md#ubuntu-2204--2404-调试-ui) 安装 Tauri/Linux 依赖，并执行 `npm ci --prefix apps/desktop`。在 Linux 图形桌面终端、仓库根目录运行：

```bash
python3 scripts/fetch-game-icons.py
python3 scripts/start-linux-debug.py \
  --db ./local/dpda.db --account YOUR_STEAM32_OR_STEAM64 --build
```

`--db` 必须指向已导入的 DPDA SQLite。脚本使用 SQLite backup API，将数据库完整复制到 `~/.local/share/dpda-debug-sessions/` 下的新会话目录，选中账号并启动真实 Tauri 窗口。源库不被修改；默认位置为自动分析，已有手动设置则保留。输出的 `resume_command` 可直接复制执行，再次打开同一会话并保留设置；不要每次都创建新副本。

已构建最新桌面二进制时可省略 `--build`。没有显示服务器时用 `--prepare-only` 只准备副本，或使用下方 CLI。仅 `npm run dev` 是浏览器预览，不能读取本地数据库。

图标只需首次下载，后续复用本地缓存；“游戏资源”页无需账号即可浏览。已有隔离会话可用原 `--resume` 命令加 `--build` 更新程序，保留同一份个人数据。资源来源、缺失及打包说明见 [game-assets.md](game-assets.md)。下方无界面 CLI 不需要下载这些图标。

总览显示历史位置分布、证据覆盖和未知样本；“我的英雄池”展示自动候选与手动覆盖；“历史战绩”提供 25/50/100 场分页、页码跳转、逐场全部已保存字段、位置推断理由和 JSON。原始缺失字段仍为 `null`。界面与 CLI 数据集相同；不把未保存的上游字段伪装成已经获取。

## 自动分析位置

以下命令使用当前源码构建的 CLI；`v0.1.0-dev.1` 下载包不含 `roles` 子命令。当前数据库 Schema 为 2，升级前请保留备份，旧包不能读取新 Schema。

```bash
cargo build --locked -p dpda-debug
target/debug/dpda-debug roles --db ./local/dpda.db --account YOUR_STEAM_ID
target/debug/dpda-debug roles --db ./local/dpda.db --account YOUR_STEAM_ID \
  --refresh --max-matches 240
```

第一条 `roles` 只分析缓存；`--refresh` 才发请求补充缺少的比赛详情。采样优先近期比赛，再按英雄轮询，遵守 OpenDota 限流；每次上限 1—3000 条，可用 Ctrl-C 暂停。重新执行跳过已有缓存，`partial` 表示预算内采样完成，不表示所有比赛都有位置证据。程序不提交回放解析请求。

推断使用完整同队五人的早期经济/补刀与分路；缺早期数据时仅用有限可信的终局指标。四/五号位不能可靠区分时保留并列；缺分路的核心可保留一/二/三号位候选，每英雄至少 3 场符合条件的证据才自动用于候选过滤。该规则不是已校准位置模型，详见 [英雄池](hero-pool.md)。普通历史同步完成后自动补充最多 120 条详情，总览按钮每次最多 240 条；普通启动仅读缓存。

## 构建与自检

可直接下载 [v0.1.0-dev.1 的 Linux 调试包](https://github.com/tiansongyu/dota2_assistant/releases/tag/v0.1.0-dev.1)，将 `dpda-linux-debug-x86_64.tar.gz` 与 `SHA256SUMS-linux.txt` 放在同一目录：

```bash
sha256sum -c SHA256SUMS-linux.txt
tar -xzf dpda-linux-debug-x86_64.tar.gz
./dpda-linux-debug/dpda-debug doctor
./dpda-linux-debug/dpda-debug api-probe --account YOUR_STEAM32_OR_STEAM64
```

后续命令中的 `target/debug/dpda-debug` 可替换为解压后的 `./dpda-linux-debug/dpda-debug`。下载版不需要 Rust；构建版步骤如下。

本次验证环境为 Ubuntu 22.04 x86_64、Rust 1.97.1。从项目根目录执行：

```bash
cargo build --locked -p dpda-debug
target/debug/dpda-debug doctor
```

构建需要 Rust/Cargo 和 C 编译工具链；SQLite 使用 bundled 构建。`doctor` 检查内存数据库迁移、操作系统、架构、可选凭证是否存在，默认不访问网络，也不输出凭证内容。

发布构建命令：

```bash
cargo build --locked --release -p dpda-debug
target/release/dpda-debug --help
```

不要用 `cargo build --workspace` 代替上面的包级命令，否则会同时构建桌面和 OpenCV 模块。CLI 运行时不需要 Rust 或 Node，但 Linux 可执行文件仍受 glibc 兼容性约束。

## 验证真实 API

```bash
target/debug/dpda-debug api-probe --account YOUR_STEAM32_OR_STEAM64
```

探测 OpenDota 英雄目录和该公开账号的两条比赛，输出解析数量、完整阵容数量、额度响应头和实际请求耗时。输入 ID 只表示公开查询，不代表 Steam 登录或身份认证。Steam64 始终按字符串处理。

需要检查完整比赛响应时增加已知比赛 ID：

```bash
target/debug/dpda-debug api-probe \
  --account YOUR_STEAM32_OR_STEAM64 \
  --match-id KNOWN_PUBLIC_MATCH_ID
```

输出只保留比赛英雄结构，不输出或存储该场其他玩家的账号信息。接口拒绝、超时、结构变化、限流会显示失败并返回非零退出码。

可选环境变量为 `STRATZ_API_TOKEN` 和 `STEAM_WEB_API_KEY`。没有凭证时报告 `not_configured`；配置但未指定 `--probe-optional` 时报告 `configured_not_probed`。明确加入该选项后，才将凭证发给对应官方服务。

STRATZ 的可选探测只请求标准 GraphQL `__typename` 字段；Steam 只调用公开玩家摘要接口。它们验证连接，不证明 DOTA2 历史数据权限、覆盖率或可用克制统计；输出始终保留 `history_verified: false`。没有把两者接成历史导入备用源。接口依据：[STRATZ 官方入口](https://stratz.com/api)、[Steam ISteamUser 文档](https://partner.steamgames.com/doc/webapi/ISteamUser)。

不要将 token 放在命令参数、提交的脚本或报告中。程序只报告凭证是否存在；认证请求错误不打印可能带查询密钥的 URL，也不复制远端错误正文。

## 同步公开历史

先进行有明确请求预算的小规模验证：

```bash
target/debug/dpda-debug sync \
  --db ./local/dpda.db \
  --account YOUR_STEAM32_OR_STEAM64 \
  --from 2021-11-03 --to 2026-11-03 \
  --page-size 100 --max-pages 1
```

日期采用 UTC，包含起始日 00:00:00 和结束日 23:59:59。`--to` 省略时使用当前 UTC 日期。页大小为 1–500。`--max-pages` 是每次执行的请求页数预算，达到预算后状态为 `paused`，不能解释为完整导入。

正式扫描去掉 `--max-pages`。按 Ctrl-C 时中止等待中的请求，保存 `paused` 检查点；重启后相同范围可继续扫描并按 `(account_id, match_id)` 去重。OpenDota 的 offset 不是稳定快照游标，因此恢复会从头重扫已访问页，以避免新比赛插入造成遗漏。这意味着反复使用同一个很小的 `--max-pages` 不会越过此前页数；预算用于烟测，需要扩大或移除预算才能完成整个范围。

只有 `completed` 表示已扫描数据源当前可访问的结果。隐私设置、未解析比赛、接口覆盖不足都可能限制实际历史范围；成功扫描不承诺恢复三到五年的全部比赛。

标准输出是最终 JSON。同步进度以 NDJSON 写到标准错误，便于分别保存：

```bash
target/debug/dpda-debug sync \
  --db ./local/dpda.db --account YOUR_STEAM32_OR_STEAM64 \
  --from 2021-11-03 --to 2026-11-03 \
  > sync-result.json 2> sync-progress.ndjson
```

状态为 `paused` 的主动暂停或预算终止返回 0；网络、限流或数据错误返回 1，最后一次提交的检查点仍可从数据库查看。参数错误由命令解析器报告。

## 检查数据与统计

```bash
target/debug/dpda-debug inspect \
  --db ./local/dpda.db --account YOUR_STEAM32_OR_STEAM64 --limit 20

target/debug/dpda-debug stats \
  --db ./local/dpda.db --account YOUR_STEAM32_OR_STEAM64 \
  --half-life-days 180
```

`inspect` 输出数据库版本、比赛数量、缺失阵容数量、时间覆盖、同步检查点和最多指定条数的最近比赛。数据库不存在时会报错，不为只读检查创建空库。

`stats` 输出真实个人英雄统计和自动英雄池建议。没有比赛时返回空列表，不生成示例胜率。可以用 `--as-of YYYY-MM-DD` 限定统计时间，用 `--patch ID` 指定已知数据源版本 ID；不自动把空缺版本推断成当前版本。

## 时间顺序回测

```bash
target/debug/dpda-debug backtest \
  --db ./local/dpda.db --account YOUR_STEAM32_OR_STEAM64 \
  --role 2 --min-training 20 --pool ./local/pool.json
```

不提供 `--pool` 时读取桌面写入的 `pool:ACCOUNT_ID` 配置。位置不能由少量历史或 `lane_role` 强行推断。没有明确支持所选位置的英雄时报告空候选、跳过数量和 `null` 覆盖率。

`pool.json` 与桌面 `PoolConfig` 相同。以下示例只展示配置形状；英雄位置应由用户按实际英雄池确认：

```json
{
  "mode": "hybrid",
  "included": [],
  "excluded": [],
  "roles": {"13": [2]},
  "priorities": {}
}
```

回测按比赛时间排序，只使用目标比赛前已结束的记录；它报告历史选择覆盖、常用英雄基线、样本量与版本分布。由于没有真实历史 BP 时间线，回测使用空阵容，不能证明克制收益；覆盖比赛的观察胜率也不能解释为“采用建议会提升多少胜率”。固定当前英雄池配置会产生回顾性选择限制，报告保留该说明。

## 本次验证记录

2026-11-03 UTC 执行了以下真实验证，使用公开诊断账号 `70388657`，没有将其设置为用户账号：

| 检查 | 结果 |
| --- | --- |
| `cargo test -p dpda-debug` | 3 个库单元测试和 2 个 CLI 测试通过 |
| `cargo clippy -p dpda-debug --all-targets -- -D warnings` | 通过 |
| 无 DISPLAY/WAYLAND_DISPLAY 执行 `doctor` | SQLite 初始化成功 |
| OpenDota `/heroes` | 127 个有效英雄 |
| OpenDota 账号历史两条结构探测 | 2 条解析成功，2 条完整双方阵容 |
| 比赛 `9073857007` 详情 | HTTP 200，10 名英雄 |
| 单页实际导入 | 10 场、0 个失败、0 个缺失阵容；因预算暂停 |
| 请求等待时真实 SIGINT | 正常返回 `paused` 并保存检查点 |
| `inspect / stats / backtest` | 已对本地真实数据执行；未配置位置的回测如实跳过 |
| `ldd target/debug/dpda-debug` | 仅系统 C 运行库相关依赖，无 GTK/WebKit/OpenCV |
| STRATZ / Steam | 无凭证，未验证认证调用 |

本地诊断输出位于 `artifacts/debug-cli/`，该目录不进入源代码交付。以上是数据及命令行验证，不是 Windows 截图识别、悬浮窗或安装包验收。
