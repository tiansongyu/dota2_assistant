# 数据源与验证记录

验证日期：2026-11-02。历史同步首选 OpenDota，离线静态数据采用固定版本的 dotaconstants。STRATZ 和 Steam 保留为可选适配器；目前没有凭证，不能宣称已验证它们的认证请求。

## OpenDota

实际发出了公开 HTTP GET 请求，未登录、未提供 API key。测试账号 `70388657` 是公开职业玩家账号，仅用于接口验证，并非当前用户的已关联账号。响应及来源时间保存在 `tests/fixtures/opendota/`。

| 请求 | 实际结果 | 用途 |
| --- | --- | --- |
| `/api/heroes` | HTTP 200，英雄使用稳定 `id` | 静态目录检查 |
| `/api/players/70388657/matches?limit=2&offset=0` | HTTP 200，2 条真实比赛 | 默认字段解析 |
| 同端点 `offset=2` | HTTP 200，2 条不同比赛 | 分页验证 |
| 同端点显式 `project=heroes` 等列 | HTTP 200，天辉槽位 0–4、夜魇 128–132 | 每页同时导入阵容 |
| `/api/matches/9073857007` | HTTP 200，10 位玩家、patch 60 | 详情结构核对 |
| `/api/status` | HTTP 404 | 不作为健康检查端点 |

源码核对基于 [odota/core commit 8d774ab](https://github.com/odota/core/tree/8d774abda8542390fbd215487d961f1b5c3a3ef3)。`svc/api/requests/playerParams.ts` 定义 `limit`、`offset`、`date`（过去多少天）和 `project`；`svc/util/buildPlayer.ts` 默认按 `match_id` 降序排序后切片。玩家比赛端点不支持比赛 ID 游标；其他端点出现的 `less_than_match_id` 不能套用到这里。

显式传 `project` 会替换默认附加列，因此客户端逐一请求所需列。`heroes` 是以 `player_slot` 为键的对象，不是英雄 ID 数组。导入时只保留英雄及阵营，不保存队友、对手的账号资料。未请求或未返回的字段保存为 `NULL`。`lane_role` 表示分路类型，不等于一至五号位；缺少可靠位置证据时使用手动位置配置。详情返回 `patch`，但实际玩家投影未返回该字段；不把缺省 patch 编造为当前版本。

### 配额、恢复与覆盖

本次响应包含 `X-Rate-Limit-Remaining-Minute: 59`、`X-Rate-Limit-Remaining-Day: 2999`。当前仓库 `config.ts` 的无 key 默认值是 60 次/分钟和 3,000 次/日；部署可以覆盖默认值。旧 [API 文档](https://docs.opendota.com/) 及 [2018 年公告](https://blog.opendota.com/2018/04/17/changes-to-the-api/) 写的每月 50,000 次与本次每日响应头不同，软件应优先遵守响应头和 429，不能把旧配额写成服务保证。

客户端使用串行请求、至少 1.1 秒间隔、有限重试和 `Retry-After`。遇到每日额度耗尽应保存进度并暂停，不能持续重试。分页期间新增或补入比赛可能移动 offset；恢复时从首屏重新扫描并去重，以减少跳过记录的风险。已存储记录可离线使用。日期范围在本地按 UTC 时间严格过滤，避免相对 `date` 随暂停时间漂移。

[OpenDota FAQ](https://blog.opendota.com/2014/08/01/faq/) 说明公开比赛设置和回放解析影响覆盖范围，过期回放无法补齐高级统计。客户端只承诺同步接口实际可访问的记录。空结果可能是隐私设置、账号没有记录或服务端覆盖不足，不自动判定为“无比赛”。不能保证恢复近五年全部记录，也没有可确认的更新延迟 SLA。

### 使用与商业许可

OpenDota core 代码使用 MIT；官方 FAQ 明确代码可用于个人及商业项目。这个代码许可不等于 Valve 游戏素材或第三方数据的独立再授权。服务按尽力而为方式提供。本项目保留来源标识和 OpenDota 链接；如果日后作为商业产品发布，应再核对当时的数据使用条件、付费配额与品牌要求。

## STRATZ

[官方 API 页面](https://stratz.com/api) 提供 GraphQL 入口，但本环境没有用户 token，未执行认证数据查询。[官方知识库](https://github.com/STRATZ-Esports/knowledge-base/issues/15) 列过默认 token 的秒、分钟、小时、每日限制；它发布于 2020 年，不能当作 2026 年当前 token 的保证。应以用户 API 控制台和实际响应为准。[官方说明](https://github.com/STRATZ-Esports/knowledge-base/issues/31) 要求来源链接，升级 token 另有引流要求。当前商业分发授权、实际历史覆盖和字段可靠性仍未验证。DPDA 不借用别人 token，也不伪装身份。

## Steam Web API

[官方概览](https://partner.steamgames.com/doc/webapi_overview) 和 [使用条款](https://steamcommunity.com/dev/apiterms) 要求保护 key，说明数据存储和隐私方式，并允许服务变更或终止。条款含调用上限及多人游戏公平性要求。本项目当前不实现 Steam 历史 provider，也不将公开账号输入视为 Steam 登录。没有 key，因此 `GetMatchHistory` 与 `GetMatchDetails` 当前行为未实测；不能依赖旧论坛参数、历史 500 场说法或第三方报告作为真实覆盖保证。

## 英雄静态数据与头像

[dotaconstants](https://github.com/odota/dotaconstants) 当前核验版本为 `10.8.0`，固定 commit `5c0242df9057848fa2036ca3fafe9f2bd4508163`。仓库 MIT 文本位于 `assets/licenses/dotaconstants-MIT.txt`。`assets/heroes.json` 是 127 位英雄的字段投影，`assets/patches.json` 保留上游版本日期表，来源写入 `assets/data-provenance.json`。这是一次目录快照，并不保证未来游戏更新后仍完整。

上游英雄 JSON 含 Valve CDN 的头像相对路径，网站头像与实际选人槽位可能裁剪、灰度、边框不同，不能直接宣称匹配精度。MIT 覆盖仓库软件的许可声明不能替代 Valve 美术资源权利确认；本项目不把图片标为 MIT。模板下载、版本、来源和哈希应独立登记；真实截图测试还需要用户授权采集或有明确来源的素材。

更新时先获取新的目录到临时文件，校验唯一且非零的英雄 ID、保留已有 ID 映射，记录来源 commit 后再替换。不要把数组下标当作英雄 ID，也不要从 Carry/Support 标签推断确定的五号位位置。
