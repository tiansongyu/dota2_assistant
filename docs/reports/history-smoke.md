# 历史导入实测

环境：Ubuntu 22.04，Rust 1.97.1。执行时间：2026-11-02 America/Denver（响应元数据为 2026-11-03 UTC）。这是公开测试账号的 API/数据库冒烟测试，不是用户个人数据验收。

```bash
cargo run -p dpda-history --example sync -- 70388657 /tmp/dpda-network-smoke.sqlite 2 2
```

首次执行真实请求两页，每页两条，新增 4 条比赛，失败 0 条，缺少完整阵容 0 条，达到显式页预算后状态为 `paused`。再次执行相同页请求新增 0、实质更新 0，数据库仍为 4 条。

SQLite 检查结果：schema version 为 1，`PRAGMA quick_check` 返回 `ok`，4 条比赛均有完整阵容，`match_heroes` 共 36 行（每场 4 队友、5 敌方）。4 条比赛的 patch 均为 `NULL`，保留了玩家列表接口的实际缺省状态。

自动化数据模块检查：`cargo test -p dpda-data-provider -p dpda-persistence -p dpda-history` 共 12 个测试通过；同三个 crate 的 `cargo clippy --all-targets -- -D warnings` 通过。测试含真实响应解析，以及明确的合成 HTTP 429/503、取消、恢复、去重、空值、迁移、事务回滚案例。

这次测试没有扫描完整五年，没有证明当前账号隐私覆盖率，也不证明所有公共账号均可访问。真实 API 有速率、每日额度和服务可用性限制。

随后执行 `cargo run -p dpda-recommender --example backtest -- /tmp/dpda-network-smoke.sqlite 70388657 2 1`：4 条输入均为标准模式；1 条训练不足，3 条因没有配置英雄位置而跳过，评估行数为 0，覆盖率为 `null`。程序没有替测试账号配置位置，也没有从这组数据得出模型效果结论。
