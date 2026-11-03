//! Retrospective coverage report. This does not predict alternative match outcomes.
use dpda_domain::normalize_account_id;
use dpda_hero_pool::PoolConfig;
use dpda_persistence::Database;
use dpda_recommender::{backtest, ModelConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 3 {
        return Err("用法：cargo run -p dpda-recommender --example backtest -- DATABASE ACCOUNT_ID [ROLE=2] [MIN_TRAINING=20] [POOL_JSON_PATH]".into());
    }
    let database = Database::open(&args[1])?;
    let account_id = normalize_account_id(&args[2])?;
    let role = args
        .get(3)
        .map(|s| s.parse::<u8>())
        .transpose()?
        .unwrap_or(2);
    let min_training = args
        .get(4)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(20);
    let pool = if let Some(path) = args.get(5) {
        serde_json::from_slice::<PoolConfig>(&std::fs::read(path)?)?
    } else {
        database
            .setting::<PoolConfig>(&format!("pool:{account_id}"))?
            .unwrap_or_default()
    };
    pool.validate()
        .map_err(|message| format!("英雄池配置：{message}"))?;
    let records = database.records(account_id)?;
    let report = backtest(&records, &pool, role, &ModelConfig::default(), min_training);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
