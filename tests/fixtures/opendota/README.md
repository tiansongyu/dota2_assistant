# OpenDota 真实响应样本

这些 JSON 来自 2026-11-02 America/Denver（2026-11-03 UTC）对 OpenDota 公共接口的实际请求，非随机模拟战绩。每份响应配有 `.meta.json`，记录完整请求 URL、采集时间、HTTP 状态及限流响应头。

- `player-matches.json`：公开测试账号 `70388657` 的默认第一页，两条比赛。
- `player-page2.json`：同一账号 offset 2 的两条比赛，用于核对分页。
- `player-projected.json`：显式投影双方阵容、GPM、XPM 和分路的两条比赛。
- `match-detail.json`：比赛 `9073857007` 的详情字段投影。保留与本项目解析有关的字段，没有保留整个大型响应。
- `heroes.json`：英雄目录响应，用于稳定 ID 检查。

公开测试账号不是当前用户账号。这些文件只供自动化解析测试使用；生产界面不得自动展示为用户数据。`match-detail.json` 与 `player-projected.json` 已删除非目标玩家的 `account_id` 字段，元数据 `redactions` 记录删除数量；英雄 ID、槽位、胜负和统计字段未修改。响应头仅保留日期、内容类型和额度信息，不保存 IP 或 Cookie。

数据库导入同样丢弃其他玩家账号，仅保留英雄与阵营。后续新采样应另存文件或更新采集信息，不得手工修改比赛结果来迎合测试。
