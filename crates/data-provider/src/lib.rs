//! Public historical data only. No live game state or opponent profile queries.
use chrono::Utc;
use dpda_domain::{Hero, MatchRecord, MatchRoleEvidence, PlayerRoleEvidence};
use reqwest::{header::HeaderMap, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::Mutex;

const PROJECTIONS: &[&str] = &[
    "hero_id",
    "start_time",
    "duration",
    "game_mode",
    "patch",
    "kills",
    "deaths",
    "assists",
    "last_hits",
    "denies",
    "gold_per_min",
    "xp_per_min",
    "lane_role",
    "heroes",
];

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("网络请求失败：{0}")]
    Network(#[from] reqwest::Error),
    #[error("数据源 HTTP {status}：{message}")]
    Http { status: u16, message: String },
    #[error("OpenDota 请求额度已用尽，请在 {retry_after_seconds} 秒后继续")]
    RateLimited {
        retry_after_seconds: u64,
        daily: bool,
    },
    #[error("数据源返回结构错误：{0}")]
    InvalidData(String),
    #[error("无效的数据源配置：{0}")]
    InvalidConfiguration(String),
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub base_url: String,
    pub min_interval: Duration,
    pub timeout: Duration,
    pub max_retries: u32,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.opendota.com/api/".into(),
            min_interval: Duration::from_millis(1100),
            timeout: Duration::from_secs(30),
            max_retries: 3,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    pub remaining_minute: Option<u32>,
    pub remaining_day: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedRecord {
    pub match_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerPage {
    pub matches: Vec<MatchRecord>,
    /// Number returned by the API, including malformed rows. Used for pagination.
    pub received: usize,
    pub rejected: Vec<RejectedRecord>,
    pub rate_limit: RateLimitInfo,
}

struct RequestState {
    next_request: tokio::time::Instant,
    daily_blocked_until: Option<SystemTime>,
}

#[derive(Clone)]
pub struct OpenDotaClient {
    http: reqwest::Client,
    base: Url,
    config: ProviderConfig,
    state: Arc<Mutex<RequestState>>,
}

impl OpenDotaClient {
    pub fn new() -> Result<Self, ProviderError> {
        Self::with_config(ProviderConfig::default())
    }

    pub fn with_config(config: ProviderConfig) -> Result<Self, ProviderError> {
        let base = Url::parse(&config.base_url)
            .map_err(|e| ProviderError::InvalidConfiguration(e.to_string()))?;
        if !matches!(base.scheme(), "https" | "http")
            || !base.path().ends_with('/')
            || config.max_retries > 8
        {
            return Err(ProviderError::InvalidConfiguration(
                "base_url 必须是以 / 结尾的 HTTP(S) URL".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .user_agent("DPDA/0.1 (public match history)")
            .timeout(config.timeout)
            .build()?;
        Ok(Self {
            http,
            base,
            config,
            state: Arc::new(Mutex::new(RequestState {
                next_request: tokio::time::Instant::now(),
                daily_blocked_until: None,
            })),
        })
    }

    async fn request(&self, url: Url) -> Result<(serde_json::Value, RateLimitInfo), ProviderError> {
        for attempt in 0..=self.config.max_retries {
            {
                let mut state = self.state.lock().await;
                if let Some(until) = state.daily_blocked_until {
                    if let Ok(remaining) = until.duration_since(SystemTime::now()) {
                        return Err(ProviderError::RateLimited {
                            retry_after_seconds: remaining.as_secs().saturating_add(1),
                            daily: true,
                        });
                    }
                    state.daily_blocked_until = None;
                }
                tokio::time::sleep_until(state.next_request).await;
                state.next_request = tokio::time::Instant::now() + self.config.min_interval;
            }
            let mut response = match self.http.get(url.clone()).send().await {
                Ok(response) => response,
                Err(error)
                    if attempt < self.config.max_retries
                        && (error.is_timeout() || error.is_connect()) =>
                {
                    tokio::time::sleep(Duration::from_secs(1 << attempt.min(5))).await;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let status = response.status();
            let headers = response.headers().clone();
            let rate = RateLimitInfo {
                remaining_minute: header_number(&headers, "x-rate-limit-remaining-minute"),
                remaining_day: header_number(&headers, "x-rate-limit-remaining-day"),
            };
            if rate.remaining_day == Some(0) {
                self.state.lock().await.daily_blocked_until = Some(next_utc_day());
            }
            if status == StatusCode::TOO_MANY_REQUESTS {
                let daily = rate.remaining_day == Some(0);
                let seconds =
                    retry_after(&headers).unwrap_or(if daily { seconds_to_next_day() } else { 60 });
                // Long waits become resumable UI states rather than blocking a worker indefinitely.
                if daily || seconds > 30 || attempt == self.config.max_retries {
                    return Err(ProviderError::RateLimited {
                        retry_after_seconds: seconds,
                        daily,
                    });
                }
                tokio::time::sleep(Duration::from_secs(seconds.max(1))).await;
                continue;
            }
            if status.is_server_error() && attempt < self.config.max_retries {
                tokio::time::sleep(Duration::from_secs(1 << attempt.min(5))).await;
                continue;
            }
            if !status.is_success() {
                // Never log query secrets or arbitrary remote HTML in the UI.
                return Err(ProviderError::Http {
                    status: status.as_u16(),
                    message: status
                        .canonical_reason()
                        .unwrap_or("unknown response")
                        .into(),
                });
            }
            // Bound accepted bodies so a malformed provider cannot exhaust local memory.
            if response
                .content_length()
                .is_some_and(|size| size > 32 * 1024 * 1024)
            {
                return Err(ProviderError::InvalidData("响应体超过 32 MiB".into()));
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if body.len().saturating_add(chunk.len()) > 32 * 1024 * 1024 {
                    return Err(ProviderError::InvalidData("响应体超过 32 MiB".into()));
                }
                body.extend_from_slice(&chunk);
            }
            let value = serde_json::from_slice(&body)
                .map_err(|e| ProviderError::InvalidData(e.to_string()))?;
            return Ok((value, rate));
        }
        Err(ProviderError::InvalidData("重试次数已耗尽".into()))
    }

    pub async fn player_matches(
        &self,
        account_id: u32,
        offset: u32,
        limit: u32,
    ) -> Result<PlayerPage, ProviderError> {
        if account_id == 0 || account_id == u32::MAX || !(1..=500).contains(&limit) {
            return Err(ProviderError::InvalidConfiguration(
                "账号无效或分页大小不在 1–500".into(),
            ));
        }
        let mut url = self
            .base
            .join(&format!("players/{account_id}/matches"))
            .map_err(|e| ProviderError::InvalidConfiguration(e.to_string()))?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("limit", &limit.to_string())
                .append_pair("offset", &offset.to_string())
                .append_pair("significant", "0");
            for field in PROJECTIONS {
                query.append_pair("project", field);
            }
        }
        let (value, rate_limit) = self.request(url).await?;
        let rows = value
            .as_array()
            .ok_or_else(|| ProviderError::InvalidData("比赛列表不是数组".into()))?;
        let mut page = parse_player_page(rows, account_id, Utc::now().timestamp());
        page.rate_limit = rate_limit;
        Ok(page)
    }

    pub async fn heroes(&self) -> Result<Vec<Hero>, ProviderError> {
        let url = self
            .base
            .join("heroes")
            .map_err(|e| ProviderError::InvalidConfiguration(e.to_string()))?;
        let (value, _) = self.request(url).await?;
        let heroes: Vec<Hero> =
            serde_json::from_value(value).map_err(|e| ProviderError::InvalidData(e.to_string()))?;
        let unique: BTreeSet<_> = heroes.iter().map(|hero| hero.id).collect();
        if heroes.is_empty() || unique.len() != heroes.len() || unique.contains(&0) {
            return Err(ProviderError::InvalidData(
                "英雄目录为空或 ID 重复/无效".into(),
            ));
        }
        Ok(heroes)
    }

    /// Fetch only a known personal match; persist sanitized same-team role evidence.
    pub async fn role_evidence(
        &self,
        record: &MatchRecord,
    ) -> Result<MatchRoleEvidence, ProviderError> {
        if record
            .match_id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .is_none()
        {
            return Err(ProviderError::InvalidConfiguration("比赛 ID 无效".into()));
        }
        let url = self
            .base
            .join(&format!("matches/{}", record.match_id))
            .map_err(|e| ProviderError::InvalidConfiguration(e.to_string()))?;
        let (value, _) = self.request(url).await?;
        parse_role_evidence(&value, record, Utc::now().timestamp())
    }
}

/// Retains no teammate account IDs, names, chat, inventories, or raw response JSON.
pub fn parse_role_evidence(
    value: &serde_json::Value,
    record: &MatchRecord,
    fetched_at: i64,
) -> Result<MatchRoleEvidence, ProviderError> {
    let invalid = |message: &str| ProviderError::InvalidData(message.into());
    if value["match_id"]
        .as_u64()
        .map(|id| id.to_string())
        .as_deref()
        != Some(record.match_id.as_str())
    {
        return Err(invalid("比赛详情 ID 与个人历史不一致"));
    }
    let slot = record
        .player_slot
        .filter(|s| *s <= 4 || (128..=132).contains(s))
        .ok_or_else(|| invalid("个人历史缺少有效 player_slot，不能定位本队"))?;
    let rows = value["players"]
        .as_array()
        .ok_or_else(|| invalid("详情缺少 players"))?;
    let integer = |v: &serde_json::Value| v.as_u64().and_then(|n| u32::try_from(n).ok());
    let early = |v: &serde_json::Value| -> Option<f64> {
        let array = v.as_array()?;
        let mut sum = 0.0;
        for minute in 10..=12 {
            let number = array.get(minute)?.as_f64()?;
            if !number.is_finite() || number < 0.0 {
                return None;
            }
            sum += number / 3.0;
        }
        sum.is_finite().then_some(sum)
    };
    let mut players = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        let Some(player_slot) = integer(&row["player_slot"]).and_then(|n| u8::try_from(n).ok())
        else {
            return Err(invalid("详情含无效 player_slot"));
        };
        if !(player_slot <= 4 || (128..=132).contains(&player_slot)) || !seen.insert(player_slot) {
            return Err(invalid("详情槽位无效或重复"));
        }
        if (player_slot < 128) != (slot < 128) {
            continue;
        }
        let hero_id = integer(&row["hero_id"])
            .filter(|id| *id > 0)
            .ok_or_else(|| invalid("详情缺少英雄 ID"))?;
        if player_slot == slot && hero_id != record.hero_id {
            return Err(invalid("详情英雄与个人历史不一致"));
        }
        // If the provider exposes the target ID, it must match; no other identities are retained.
        if player_slot == slot
            && !row["account_id"].is_null()
            && integer(&row["account_id"]) != Some(record.account_id)
        {
            return Err(invalid("详情中的目标账号与个人历史不一致或无效"));
        }
        players.push(PlayerRoleEvidence {
            player_slot,
            hero_id,
            lane_role: integer(&row["lane_role"])
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| (1..=4).contains(n)),
            gold_per_min: integer(&row["gold_per_min"]),
            last_hits: integer(&row["last_hits"]),
            early_gold: early(&row["gold_t"]),
            early_last_hits: early(&row["lh_t"]),
            position_est: integer(&row["position_est"])
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| (1..=5).contains(n)),
        });
    }
    if players.len() != 5 || !players.iter().any(|p| p.player_slot == slot) {
        return Err(invalid("详情没有完整的本队五个槽位"));
    }
    Ok(MatchRoleEvidence {
        account_id: record.account_id,
        match_id: record.match_id.clone(),
        source: "opendota_match_details".into(),
        fetched_at,
        players,
    })
}

fn header_number(headers: &HeaderMap, key: &str) -> Option<u32> {
    let value: i64 = headers.get(key)?.to_str().ok()?.parse().ok()?;
    u32::try_from(value.max(0)).ok()
}
fn retry_after(headers: &HeaderMap) -> Option<u64> {
    let text = headers.get("retry-after")?.to_str().ok()?;
    text.parse().ok().or_else(|| {
        httpdate::parse_http_date(text)
            .ok()?
            .duration_since(SystemTime::now())
            .ok()
            .map(|d| d.as_secs().saturating_add(1))
    })
}
fn seconds_to_next_day() -> u64 {
    (86400 - Utc::now().timestamp().rem_euclid(86400)) as u64
}
fn next_utc_day() -> SystemTime {
    SystemTime::now() + Duration::from_secs(seconds_to_next_day())
}

#[derive(Deserialize)]
struct ApiPlayerMatch {
    match_id: serde_json::Value,
    hero_id: u32,
    start_time: i64,
    duration: Option<u32>,
    game_mode: Option<u32>,
    patch: Option<u32>,
    player_slot: Option<u8>,
    radiant_win: Option<bool>,
    kills: Option<u32>,
    deaths: Option<u32>,
    assists: Option<u32>,
    last_hits: Option<u32>,
    denies: Option<u32>,
    gold_per_min: Option<u32>,
    xp_per_min: Option<u32>,
    lane_role: Option<u8>,
    heroes: Option<BTreeMap<String, ApiHero>>,
}
#[derive(Deserialize)]
struct ApiHero {
    hero_id: Option<u32>,
}

pub fn parse_player_page(
    rows: &[serde_json::Value],
    account_id: u32,
    fetched_at: i64,
) -> PlayerPage {
    let mut matches = Vec::new();
    let mut rejected = Vec::new();
    for row in rows {
        match parse_record(row.clone(), account_id, fetched_at) {
            Ok(record) => matches.push(record),
            Err(reason) => rejected.push(RejectedRecord {
                match_id: row.get("match_id").and_then(exact_id),
                reason,
            }),
        }
    }
    PlayerPage {
        matches,
        received: rows.len(),
        rejected,
        rate_limit: RateLimitInfo::default(),
    }
}

fn exact_id(value: &serde_json::Value) -> Option<String> {
    let parsed = value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())?;
    (parsed > 0).then(|| parsed.to_string())
}

fn parse_record(
    value: serde_json::Value,
    account_id: u32,
    fetched_at: i64,
) -> Result<MatchRecord, String> {
    let m: ApiPlayerMatch = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let match_id = exact_id(&m.match_id).ok_or("match_id 必须是正整数或十进制字符串")?;
    if m.hero_id == 0 || m.start_time < 0 {
        return Err("hero_id 或 start_time 无效".into());
    }
    let mut team_heroes = Vec::new();
    let mut enemy_heroes = Vec::new();
    let mut valid_slots = BTreeSet::new();
    let mut unique_heroes = BTreeSet::new();
    let mut own_slot_matches = false;
    if let (Some(own_slot), Some(heroes)) = (m.player_slot.filter(|s| valid_slot(*s)), m.heroes) {
        for (slot, hero) in heroes {
            let Some(slot) = slot.parse::<u8>().ok().filter(|slot| valid_slot(*slot)) else {
                continue;
            };
            let Some(hero) = hero.hero_id.filter(|h| *h > 0) else {
                continue;
            };
            valid_slots.insert(slot);
            unique_heroes.insert(hero);
            if slot == own_slot {
                own_slot_matches = hero == m.hero_id;
                continue;
            }
            if (slot < 128) == (own_slot < 128) {
                team_heroes.push(hero);
            } else {
                enemy_heroes.push(hero);
            }
        }
    }
    let lineup_complete = valid_slots.len() == 10
        && unique_heroes.len() == 10
        && own_slot_matches
        && team_heroes.len() == 4
        && enemy_heroes.len() == 5;
    Ok(MatchRecord {
        match_id,
        account_id,
        hero_id: m.hero_id,
        start_time: m.start_time,
        duration: m.duration,
        game_mode: m.game_mode,
        patch: m.patch,
        player_slot: m.player_slot,
        radiant_win: m.radiant_win,
        kills: m.kills,
        deaths: m.deaths,
        assists: m.assists,
        last_hits: m.last_hits,
        denies: m.denies,
        gold_per_min: m.gold_per_min,
        xp_per_min: m.xp_per_min,
        lane_role: m.lane_role,
        team_heroes,
        enemy_heroes,
        lineup_complete,
        source: "opendota".into(),
        fetched_at,
    })
}
fn valid_slot(slot: u8) -> bool {
    slot <= 4 || (128..=132).contains(&slot)
}

#[cfg(test)]
mod role_evidence_tests {
    use super::*;
    use serde_json::{json, Value};

    // Explicitly synthetic transport fixtures, not a user's match or accuracy dataset.
    fn input() -> (MatchRecord, Value) {
        let record = MatchRecord {
            account_id: 42,
            match_id: "123456789".into(),
            hero_id: 3,
            player_slot: Some(2),
            ..Default::default()
        };
        let players: Vec<_> = (0u32..10)
            .map(|i| {
                let slot = if i < 5 { i } else { i + 123 };
                let account = if i == 2 { 42 } else { 900_000 + i };
                json!({
                    "player_slot":slot,"hero_id":i+1,"account_id":account,
                    "personaname":format!("synthetic-private-name-{i}"),
                    "profileurl":"https://example.invalid/private-player",
                    "lane_role":if i==2 {2}else{1},"gold_per_min":500,"last_hits":200,
                    "gold_t":[0,10,20,30,40,50,60,70,80,90,3000,3300,3600],
                    "lh_t":[0,1,2,3,4,5,6,7,8,9,30,33,36],"position_est":2
                })
            })
            .collect();
        (record, json!({"match_id":123456789,"players":players}))
    }

    #[test]
    fn sanitized_evidence_retains_only_own_team_and_no_player_identities() {
        let (record, value) = input();
        let evidence = parse_role_evidence(&value, &record, 1234).unwrap();
        assert_eq!(evidence.account_id, 42);
        assert_eq!(evidence.fetched_at, 1234);
        assert_eq!(evidence.players.len(), 5);
        assert!(evidence.players.iter().all(|p| p.player_slot <= 4));
        assert_eq!(evidence.players[2].early_gold, Some(3300.0));
        assert_eq!(evidence.players[2].early_last_hits, Some(33.0));
        let serialized = serde_json::to_value(&evidence).unwrap();
        for player in serialized["players"].as_array().unwrap() {
            assert!(player.get("account_id").is_none());
            assert!(player.get("personaname").is_none());
            assert!(player.get("profileurl").is_none());
        }
        let text = serialized.to_string();
        assert!(!text.contains("90000"));
        assert!(!text.contains("private"));
    }

    #[test]
    fn dire_target_keeps_only_the_five_dire_slots() {
        let (mut record, mut value) = input();
        record.player_slot = Some(130);
        record.hero_id = 8;
        value["players"][7]["account_id"] = json!(42);
        let evidence = parse_role_evidence(&value, &record, 1234).unwrap();
        assert_eq!(evidence.players.len(), 5);
        assert!(evidence
            .players
            .iter()
            .all(|p| (128..=132).contains(&p.player_slot)));
        value["players"] = Value::Array(value["players"].as_array().unwrap()[5..].to_vec());
        assert_eq!(
            parse_role_evidence(&value, &record, 1234)
                .unwrap()
                .players
                .len(),
            5
        );
    }

    #[test]
    fn target_match_hero_and_account_mismatches_are_rejected() {
        let (record, value) = input();
        for (field, wrong) in [("hero_id", json!(99)), ("account_id", json!(99))] {
            let mut invalid = value.clone();
            invalid["players"][2][field] = wrong;
            assert!(matches!(
                parse_role_evidence(&invalid, &record, 1234),
                Err(ProviderError::InvalidData(_))
            ));
        }
        let mut invalid = value.clone();
        invalid["match_id"] = json!(123456788);
        assert!(parse_role_evidence(&invalid, &record, 1234).is_err());
        let mut anonymous = value;
        anonymous["players"][2]["account_id"] = Value::Null;
        assert!(parse_role_evidence(&anonymous, &record, 1234).is_ok());
    }

    #[test]
    fn invalid_duplicate_and_incomplete_slots_cannot_form_a_team() {
        let (record, value) = input();
        for bad_slot in [json!(2), json!(5), json!(256), json!(-1), Value::Null] {
            let mut invalid = value.clone();
            invalid["players"][1]["player_slot"] = bad_slot;
            assert!(parse_role_evidence(&invalid, &record, 1234).is_err());
        }
        let mut missing = value.clone();
        missing["players"].as_array_mut().unwrap().remove(0);
        assert!(parse_role_evidence(&missing, &record, 1234).is_err());
        let mut invalid_record = record;
        invalid_record.player_slot = None;
        assert!(parse_role_evidence(&value, &invalid_record, 1234).is_err());
    }

    #[test]
    fn absent_short_null_and_negative_early_data_remain_missing() {
        let (record, value) = input();
        for bad in [
            Value::Null,
            json!([0, 1, 2]),
            json!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, null, 12]),
            json!([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, -1, 11, 12]),
        ] {
            let mut partial = value.clone();
            partial["players"][2]["gold_t"] = bad.clone();
            partial["players"][2]["lh_t"] = bad;
            let evidence = parse_role_evidence(&partial, &record, 1234).unwrap();
            assert_eq!(evidence.players[2].early_gold, None);
            assert_eq!(evidence.players[2].early_last_hits, None);
            assert_eq!(evidence.players[2].gold_per_min, Some(500));
        }
        let mut missing = value;
        missing["players"][2]
            .as_object_mut()
            .unwrap()
            .remove("gold_t");
        missing["players"][2]["gold_per_min"] = Value::Null;
        missing["players"][2]["last_hits"] = Value::Null;
        let evidence = parse_role_evidence(&missing, &record, 1234).unwrap();
        assert_eq!(evidence.players[2].early_gold, None);
        assert_eq!(evidence.players[2].gold_per_min, None);
        assert_eq!(evidence.players[2].last_hits, None);
    }

    #[test]
    fn malformed_exposed_target_identity_is_not_treated_as_anonymous() {
        let (record, value) = input();
        for bad in [json!("42"), json!(-1), json!(4_294_967_296u64), json!(true)] {
            let mut invalid = value.clone();
            invalid["players"][2]["account_id"] = bad;
            assert!(parse_role_evidence(&invalid, &record, 1234).is_err());
        }
    }

    #[test]
    fn finite_early_values_do_not_overflow_the_mean() {
        let (record, mut value) = input();
        for minute in 10..=12 {
            value["players"][2]["gold_t"][minute] = json!(1e308);
        }
        let evidence = parse_role_evidence(&value, &record, 1234).unwrap();
        let mean = evidence.players[2].early_gold.unwrap();
        assert!(mean.is_finite());
        assert!((mean / 1e308 - 1.0).abs() < 1e-12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_responses_preserve_nulls_and_full_lineups() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/opendota/player-projected.json"
        ))
        .unwrap();
        let page = parse_player_page(&rows, 70388657, 100);
        assert!(page.rejected.is_empty());
        assert_eq!(page.matches.len(), 2);
        let first = &page.matches[0];
        assert_eq!(first.match_id, "9073857007");
        assert_eq!(first.team_heroes.len(), 4);
        assert_eq!(first.enemy_heroes.len(), 5);
        assert!(first.lineup_complete);
        assert!(!first.team_heroes.contains(&first.hero_id));
        assert_eq!(first.won(), Some(true));
        assert_eq!(page.matches[1].lane_role, None);
        assert_eq!(first.patch, None);
    }
    #[test]
    fn default_page_does_not_fabricate_lineups() {
        let rows = serde_json::from_str::<Vec<serde_json::Value>>(include_str!(
            "../../../tests/fixtures/opendota/player-matches.json"
        ))
        .unwrap();
        let page = parse_player_page(&rows, 70388657, 0);
        assert!(!page.matches[0].lineup_complete);
        assert!(page.matches[0].enemy_heroes.is_empty());
    }
    #[test]
    fn malformed_rows_are_reported_without_losing_valid_rows() {
        let rows = vec![
            serde_json::json!({"match_id":1,"hero_id":0,"start_time":100}),
            serde_json::json!({"match_id":"9007199254740993","hero_id":1,"start_time":100}),
        ];
        let page = parse_player_page(&rows, 1, 0);
        assert_eq!(page.received, 2);
        assert_eq!(page.rejected.len(), 1);
        assert_eq!(page.matches[0].match_id, "9007199254740993");
        assert_eq!(page.matches[0].radiant_win, None);
    }
    #[tokio::test]
    async fn rate_limit_is_a_resumable_error() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            stream.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 2\r\nRetry-After: 180\r\nX-Rate-Limit-Remaining-Day: 0\r\nConnection: close\r\n\r\n{}").await.unwrap();
        });
        let client = OpenDotaClient::with_config(ProviderConfig {
            base_url: format!("http://{address}/"),
            min_interval: Duration::ZERO,
            ..Default::default()
        })
        .unwrap();
        assert!(matches!(
            client.player_matches(1, 0, 1).await,
            Err(ProviderError::RateLimited {
                retry_after_seconds: 180,
                daily: true
            })
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn transient_server_error_is_retried_and_projection_is_explicit() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for status in ["503 Service Unavailable", "200 OK"] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                loop {
                    let count = stream.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                    if request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                    assert!(request.len() < 16_384);
                }
                let request = String::from_utf8_lossy(&request);
                assert!(request.contains("offset=12"));
                assert!(request.contains("project=heroes"));
                assert!(request.contains("project=hero_id"));
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]"
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client = OpenDotaClient::with_config(ProviderConfig {
            base_url: format!("http://{address}/"),
            min_interval: Duration::ZERO,
            max_retries: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(client.player_matches(1, 12, 2).await.unwrap().received, 0);
        server.await.unwrap();
    }
}
