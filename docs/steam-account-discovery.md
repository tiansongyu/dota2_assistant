# 自动发现本机 Steam 账号

首次打开桌面程序时，账号区域会读取当前系统用户的 Steam 本地账号列表。无需输入 Steam ID；选择候选并点击“使用此账号”，再按需同步历史。保存后仍使用本机 SQLite，后续启动不要求重复关联。

## 选择与回退

- 只有一个去重候选，或有唯一的最近使用标记时，程序预选该账号。
- 多个候选没有明确依据、不同安装目录的最近使用标记冲突，或部分目录读取失败时，不预选账号。用户直接从列表选择。
- 已保存的 DPDA 账号不会随 Steam 本地记录变化自动切换；在设置中点击“切换账号”后重新检测。
- 未安装 Steam、没有记住账号、权限不足或配置格式改变时，界面给出相应提示，可重试或手动填写公开 ID。
- 检测与保存账号均不发网络请求；只有启动历史同步后才访问数据平台。

本机记录可以过期，也可以包含曾在这台电脑上使用过的其他账号。“最近使用”不代表当前在线或身份已认证。本功能不解除战绩可见性限制，也不能作为未来公开服务的登录凭据。

## 实现边界

`crates/steam-discovery` 负责路径发现、有界 KeyValues 解析和候选合并。桌面通过异步 `discover_steam_accounts` IPC 调用，避免文件读取阻塞界面；Linux 调试工具复用同一模块。

读取范围限于已定位的 Steam 安装目录下 `config/loginusers.vdf`。只提取个人 Steam64、昵称、最近使用标记和可用时间；Steam64 在 JSON 与 TypeScript 中始终为字符串。Steam 登录名、未知字段和原始文件内容不进入 IPC、数据库或日志，不读取凭证存储、`config.vdf`、`localconfig.vdf`、游戏内存或其他玩家资料。

Windows 使用安装路径注册表信息和已知默认目录；Linux 检查当前用户的 Steam 标准目录及已支持的包装目录。合法 Steam 根目录符号链接会按真实路径去重，不递归扫描主目录或 `userdata`。具体路径与错误规则以模块代码为准。

解析限制包括文件大小、层级、节点、字符串长度及候选数量；重复键、异常 Steam64、无法支持的 KeyValues 指令和结构损坏会被拒绝。读取错误返回固定来源标签与说明，不回显路径、原始 VDF 或未知字段。

## Linux 调试

```bash
cargo build --locked -p dpda-debug
target/debug/dpda-debug steam-accounts
```

输出候选、建议选择、检测状态与诊断，不联网、不写数据库，也不修改 Steam 文件。输出包含本机账号 ID 和昵称，分享排障信息时可只保留状态及候选数量。

非标准安装或明确的测试目录可单独指定根目录；此时不再扫描默认路径：

```bash
target/debug/dpda-debug steam-accounts --steam-root /path/to/Steam
```

目录参数指向 Steam 根目录，不是 VDF 文件。`found`、`not_installed`、`no_records` 表示检测正常结束；部分失败、权限、格式或 I/O 错误以及不支持的平台返回非零退出码，JSON 仍保留诊断。

## 来源与验证

[Steam 官方启动器源码](https://repo.steampowered.com/steam/archive/stable/steam_1.0.0.87.tar.gz) 使用 `~/.steam/steam`、`~/.steam/root` 与 XDG 下的 Steam 数据目录。`loginusers.vdf` 的字段和 Windows 安装路径信息属于本地兼容读取，未发现 Valve 承诺稳定的公开账号发现 API；Steam 更新后需要继续验证。

Valve 的 [GetSteamID](https://partner.steamgames.com/doc/api/ISteamUser#GetSteamID) 属于 Steamworks，其[初始化要求](https://partner.steamgames.com/doc/api/steam_api#SteamAPI_Init)包括有效的本应用 App ID 及运行环境。当前实现不借用 DOTA2 或测试游戏的 App ID 初始化 Steamworks，也不操作 Steam 会话。

自动化测试使用明确的合成 VDF 记录，覆盖候选选择、重复安装、格式错误、数据大小限制及字段最小化。真实本机与 Windows 验证结果另见 [检测验收记录](reports/steam-account-discovery.md)。
