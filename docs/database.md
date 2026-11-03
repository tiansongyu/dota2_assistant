# 本地数据库

实现：`crates/persistence`。Schema 当前版本为 2，DDL 位于 `crates/persistence/migrations/`，新增 `002_role_evidence.sql` 保存位置证据；`PRAGMA user_version` 保存版本号。打开数据库时检查完整性、拒绝高于当前支持版本的数据库，并按版本在事务内执行迁移。升级保留已有历史；旧二进制不能读取 Schema 2，调试前应保留备份或使用隔离启动脚本。

SQLite 使用 WAL、外键约束、5 秒 busy timeout。同步工作线程和界面查询各自打开一个连接，避免把 `Connection` 跨线程共享。数据库损坏时返回错误，软件不能自动删除或覆盖旧文件。

读取一个账号的比赛和阵容只使用两次查询：先读取按时间排序的比赛，再批量读取该账号的阵容并按 match_id 组装。不会为每一场比赛单独查询阵容。当前仍一次返回该账号全部本地记录，尚无大规模历史的内存与延迟验收报告。

## 表与职责

| 表 | 内容 | 主键 / 索引 |
| --- | --- | --- |
| `profiles` | 公开账号 ID、精确 Steam64 字符串、创建时间 | `account_id` |
| `matches` | 当前账号在一场比赛中的表现、胜负依据、来源和采集时间 | `(account_id, match_id)`；账号、英雄、比赛时间、版本索引 |
| `match_heroes` | 与当前账号同队的其他英雄及敌方英雄 | `(account_id, match_id, side, hero_id)`；英雄和阵营索引 |
| `settings` | 规模较小的用户配置 JSON，例如英雄池、位置、权重、校准区域 | `key` |
| `sync_jobs` | 固定日期范围、分页偏移、同步状态和实际计数 | `account_id` |
| `import_failures` | 无法解析的比赛标识、原因、发现时间 | `(account_id, record_key)` |
| `role_evidence` | 同队五人的最小位置证据 JSON、来源和采集时间；不含其他玩家账号 | `(account_id, match_id)`，关联个人比赛外键 |

账号输入只创建本地公开数据分析记录，不表示 Steam 身份认证。Steam64 与 match_id 使用 `TEXT`，跨 IPC 也使用字符串。英雄 ID 为稳定的整数，不使用目录下标。

## 空值和统计

KDA、GPM、XPM、patch、分路和比赛结果缺失时保存 `NULL`。胜负由有效 `player_slot` 和 `radiant_win` 共同确定，两者缺失时不计入已知结果分母。`lane_role` 是数据源分路标签，不自动转换为一至五号位。

`match_heroes.side=ally` 不包含账号本人英雄。`lineup_complete=true` 表示导入器已核对 10 个合法槽位、10 个不同英雄及本人槽位；推荐器应按完整度决定能否用于阵容分析。数据库不保存队友或对手的账号资料，不保存整个比赛原始 JSON。聚合统计由本地事实重新计算；尚未建立独立统计快照表，避免重复事实失去同步。

`role_evidence` 仅保留同队槽位、英雄、分路、GPM、补刀、10—12 分钟经济/补刀均值和上游推断标签。上游 `position_est` 不当作真实标签；本地规则版本和解释由分析器生成。`settings` 中 `role_sync:ACCOUNT_ID` 记录详情请求进度；预算完成且仍有未缓存比赛时为 `partial`。自动位置结果实时计算，用户 `pool.roles` 只保存显式覆盖；缺 key 表示自动，空数组表示禁止该英雄所有位置。

## 导入与恢复

一页记录、解析失败条目和下一页进度在同一个事务中提交。重复 `(account_id, match_id)` 执行更新：新的空值不擦掉已知值，不完整阵容不覆盖已有完整阵容。仅更新时间不计作“数据更新”；插入与实质更新分别计数。

OpenDota offset 不是快照游标。暂停后保留 checkpoint，继续时从 offset 0 重新检查当前列表，并依靠唯一键去重。这样会多请求已经导入的页，但不会因为头部新增比赛直接跳过这些记录。仍无法保证服务端在整个扫描期间任意删改记录时获得一致快照；可再次同步校正。

暂停状态为 `paused`，限流为 `rate_limited`，网络或解析错误为 `failed`，遍历结束为 `completed`。`completed` 仅表示当前可访问分页扫描结束，不代表用户五年历史完整。`failed` 计数表示已记录的异常行数；`missing_lineups` 是该账号本地缺少完整阵容的记录数。

## 开发验证

```bash
cargo test -p dpda-persistence -p dpda-history -p dpda-data-provider
cargo run -p dpda-history --example sync -- 70388657 /tmp/dpda-smoke.sqlite 2 2
```

第二条命令会访问真实 OpenDota 公共接口，只扫描两页并主动暂停，便于检查去重和恢复。账号是公开测试账号，不应被当作用户默认账号。恢复会从头重新核对，因此重复相同小预算只会复查前几页，不会自动跳到更旧的记录。省略最后两项将以每页 100 条扫描整个可访问历史，在本地保留近五年范围内记录。数据库仅保存在指定路径；实际个人数据库不应提交到代码仓库。
