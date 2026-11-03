//! Headless diagnostic workflows. This crate intentionally has no UI or vision dependency.
use chrono::{NaiveDate, Utc};
use dpda_domain::{normalize_account_id, steam64};
use dpda_hero_pool::{aggregate, automatic_pool, PoolConfig};
use dpda_persistence::Database;
use dpda_recommender::{backtest, ModelConfig};
use serde_json::{json, Value};
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn date_boundary(value: &str, end_of_day: bool) -> Result<i64> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
    if date.format("%Y-%m-%d").to_string() != value {
        return Err("日期必须严格使用 YYYY-MM-DD 格式".into());
    }
    let time = if end_of_day {
        date.and_hms_opt(23, 59, 59)
    } else {
        date.and_hms_opt(0, 0, 0)
    }
    .ok_or("无效日期")?;
    let timestamp = time.and_utc().timestamp();
    if timestamp < 0 {
        return Err("日期必须为 1970-01-01 或以后".into());
    }
    Ok(timestamp)
}

pub fn existing_database(path: &Path) -> Result<Database> {
    if !path.is_file() {
        return Err(format!("数据库不存在：{}；先运行 sync", path.display()).into());
    }
    Ok(Database::open(path)?)
}

pub fn inspect_database(path: &Path, account: Option<&str>, limit: usize) -> Result<Value> {
    let db = existing_database(path)?;
    let profiles = if let Some(id) = account {
        vec![normalize_account_id(id)?]
    } else {
        db.profiles()?
    };
    let mut accounts = vec![];
    for account_id in profiles {
        let rows = db.records(account_id)?;
        accounts.push(json!({
            "account_id": account_id,
            "steam64": steam64(account_id),
            "matches": rows.len(),
            "missing_lineups": db.missing_lineups(account_id)?,
            "earliest_start_time": rows.iter().map(|r| r.start_time).min(),
            "latest_start_time": rows.iter().map(|r| r.start_time).max(),
            "checkpoint": db.checkpoint(account_id)?,
            "recent_records": rows.into_iter().take(limit).collect::<Vec<_>>()
        }));
    }
    Ok(json!({"schema_version":db.schema_version()?,"database":path,"accounts":accounts}))
}

pub fn statistics(
    path: &Path,
    account: &str,
    half_life_days: f64,
    now: i64,
    patch: Option<u32>,
) -> Result<Value> {
    if !half_life_days.is_finite() || half_life_days <= 0.0 {
        return Err("半衰期必须是正的有限天数".into());
    }
    let account_id = normalize_account_id(account)?;
    let db = existing_database(path)?;
    let rows = db.records(account_id)?;
    let stats = aggregate(&rows, now, half_life_days, patch);
    Ok(
        json!({"account_id":account_id,"source_matches":rows.len(),"as_of":now,"current_patch":patch,
        "half_life_days":half_life_days,"automatic_hero_pool":automatic_pool(&stats),"heroes":stats,
        "note":"历史统计包含导入范围内比赛；自动英雄池不自动断言五号位体系的位置，推荐前须明确位置。"}),
    )
}

pub fn run_backtest(
    path: &Path,
    account: &str,
    pool_path: Option<&Path>,
    role: u8,
    minimum: usize,
    half_life_days: f64,
) -> Result<Value> {
    if !(1..=5).contains(&role)
        || minimum == 0
        || !half_life_days.is_finite()
        || half_life_days <= 0.0
    {
        return Err("位置须为1..5、训练门槛须大于0、半衰期须为正的有限天数".into());
    }
    let account_id = normalize_account_id(account)?;
    let db = existing_database(path)?;
    let pool: PoolConfig = if let Some(path) = pool_path {
        serde_json::from_slice(&std::fs::read(path)?)?
    } else {
        db.setting(&format!("pool:{account_id}"))?
            .unwrap_or_default()
    };
    pool.validate()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
    let config = ModelConfig {
        half_life_days,
        ..Default::default()
    };
    Ok(serde_json::to_value(backtest(
        &db.records(account_id)?,
        &pool,
        role,
        &config,
        minimum,
    ))?)
}

pub fn doctor(database: Option<&Path>) -> Value {
    let memory_check = Database::in_memory().and_then(|db| db.schema_version());
    let sqlite = match memory_check {
        Ok(version) => json!({"status":"ok","schema_version":version}),
        Err(error) => json!({"status":"error","message":error.to_string()}),
    };
    let db = database.map(|path| match inspect_database(path, None, 0) {
        Ok(report) => report,
        Err(error) => json!({"status":"error","message":error.to_string()}),
    });
    json!({
        "version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "checked_at":Utc::now().to_rfc3339(),"sqlite":sqlite,"database":db,
        "runtime_requirements":{"gtk":false,"webkit":false,"opencv":false,"node":false,"windows_capture":false},
        "capabilities":{"public_history_sync":true,"local_statistics":true,"chronological_backtest":true,"game_capture":false,"overlay":false},
        "credentials":{"stratz_configured":credential("STRATZ_API_TOKEN").is_some(),"steam_configured":credential("STEAM_WEB_API_KEY").is_some()},
        "network":"not_probed; use api-probe"
    })
}

fn credential(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

fn transport_message(error: &reqwest::Error) -> &'static str {
    // Never format reqwest::Error for authenticated URLs: it may contain the query key.
    if error.is_timeout() {
        "request_timeout"
    } else if error.is_connect() {
        "connection_failed"
    } else {
        "http_transport_failed"
    }
}

async fn bounded_json(
    request: reqwest::RequestBuilder,
) -> std::result::Result<(u16, Value), String> {
    let mut response = request
        .send()
        .await
        .map_err(|e| transport_message(&e).to_string())?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        return Err(format!("HTTP {status}"));
    }
    let mut data = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| transport_message(&e).to_string())?
    {
        if data.len() + chunk.len() > 16 * 1024 * 1024 {
            return Err("response_too_large".into());
        }
        data.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&data).map_err(|_| "invalid_json".to_string())?;
    Ok((status, value))
}

pub async fn api_probe(
    account: Option<&str>,
    match_id: Option<&str>,
    probe_optional: bool,
) -> Result<Value> {
    let account_id = account.map(normalize_account_id).transpose()?;
    if match_id.is_some_and(|id| {
        id.is_empty()
            || !id.bytes().all(|b| b.is_ascii_digit())
            || id.parse::<u64>().ok().filter(|id| *id > 0).is_none()
    }) {
        return Err("match_id 必须是有效正整数字符串".into());
    }
    let provider = dpda_data_provider::OpenDotaClient::new()?;
    let mut sources = serde_json::Map::new();
    let began = Instant::now();
    sources.insert("opendota_heroes".into(), match provider.heroes().await {
        Ok(heroes) => json!({"status":"ok","heroes":heroes.len(),"latency_ms":began.elapsed().as_secs_f64()*1000.0,"endpoint":"https://api.opendota.com/api/heroes"}),
        Err(error) => json!({"status":"error","message":error.to_string()})
    });
    sources.insert("opendota_player_matches".into(), if let Some(id) = account_id {
        let began = Instant::now();
        match provider.player_matches(id, 0, 2).await {
            Ok(page) => json!({"status":"ok","account_id":id,"received":page.received,"parsed":page.matches.len(),"rejected":page.rejected,"complete_lineups":page.matches.iter().filter(|m|m.lineup_complete).count(),"rate_limit":page.rate_limit,"latency_ms":began.elapsed().as_secs_f64()*1000.0,"note":"公开账号查询，不是账号身份认证；只取两行验证结构。"}),
            Err(error) => json!({"status":"error","account_id":id,"message":error.to_string()})
        }
    } else { json!({"status":"not_requested","reason":"supply --account for a public history probe"}) });
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .user_agent("DPDA/0.1 diagnostic")
        .build()?;
    sources.insert("opendota_match_details".into(), if let Some(id) = match_id {
        let endpoint = format!("https://api.opendota.com/api/matches/{id}");
        match bounded_json(http.get(endpoint)).await {
            Ok((status, value)) => {
                let players = value.get("players").and_then(Value::as_array);
                let valid = value.get("match_id").and_then(Value::as_u64).map(|v|v.to_string()).as_deref()==Some(id) && players.is_some();
                json!({"status":if valid {"ok"}else{"invalid_schema"},"http_status":status,"match_id":id,"player_count":players.map(Vec::len),"hero_ids":players.map(|rows|rows.iter().filter_map(|p|p.get("hero_id").and_then(Value::as_u64)).collect::<Vec<_>>()),"note":"仅输出比赛英雄结构，不收集其他玩家账号。"})
            },
            Err(error) => json!({"status":"error","message":error})
        }
    } else {json!({"status":"not_requested","reason":"supply --match-id to verify full-match response"})});
    sources.insert("stratz".into(), match credential("STRATZ_API_TOKEN") {
        None => json!({"status":"not_configured","history_verified":false}),
        Some(_) if !probe_optional => json!({"status":"configured_not_probed","history_verified":false,"reason":"add --probe-optional to send the configured token to the official service"}),
        Some(token) => {
            // Standard GraphQL meta-field only: connectivity, not a claim about match permissions.
            match bounded_json(http.post("https://api.stratz.com/graphql").bearer_auth(token).json(&json!({"query":"query DpdaConnectivity { __typename }"}))).await {
                Ok((status, value)) => json!({"status":if value.pointer("/data/__typename").and_then(Value::as_str).is_some() && value.get("errors").is_none(){"reachable"}else{"graphql_error"},"http_status":status,"history_verified":false,"note":"仅验证 GraphQL 连接，不证明历史比赛覆盖或统计授权。"}),
                Err(message) => json!({"status":"error","message":message,"history_verified":false})
            }
        }
    });
    sources.insert("steam".into(), match credential("STEAM_WEB_API_KEY") {
        None => json!({"status":"not_configured","history_verified":false}),
        Some(_) if !probe_optional => json!({"status":"configured_not_probed","history_verified":false}),
        Some(_) if account_id.is_none() => json!({"status":"not_requested","history_verified":false,"reason":"--account required for own public profile probe"}),
        Some(key) => {
            let steam_id = steam64(account_id.expect("guarded above"));
            let request = http.get("https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v0002/").query(&[("key",key),("steamids",steam_id)]);
            match bounded_json(request).await {
                Ok((status, value)) => json!({"status":if value.pointer("/response/players").and_then(Value::as_array).is_some(){"reachable"}else{"invalid_schema"},"http_status":status,"history_verified":false,"note":"仅验证公开玩家摘要接口，不证明 DOTA2 历史接口授权或覆盖。"}),
                Err(message) => json!({"status":"error","message":message,"history_verified":false})
            }
        }
    });
    let ok = sources.values().all(|v| {
        !matches!(
            v.get("status").and_then(Value::as_str),
            Some("error" | "invalid_schema" | "graphql_error")
        )
    });
    Ok(json!({"checked_at":Utc::now().to_rfc3339(),"ok":ok,"sources":sources}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_bounds_are_inclusive_utc_and_validate_calendar() {
        assert_eq!(
            date_boundary("2026-11-03", true).unwrap()
                - date_boundary("2026-11-03", false).unwrap(),
            86399
        );
        assert!(date_boundary("2026-02-30", false).is_err());
        assert!(date_boundary("2026-1-3", false).is_err());
        assert!(date_boundary("1969-12-31", false).is_err());
    }
    #[test]
    fn inspect_does_not_create_a_missing_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing.db");
        assert!(inspect_database(&path, None, 10).is_err());
        assert!(!path.exists());
    }
    #[test]
    fn empty_database_has_no_invented_statistics() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("local.db");
        Database::open(&path)
            .unwrap()
            .ensure_profile(70388657)
            .unwrap();
        let report = statistics(&path, "70388657", 180.0, Utc::now().timestamp(), None).unwrap();
        assert_eq!(report["source_matches"], 0);
        assert_eq!(report["heroes"], json!([]));
    }
}
