//! Explicit network smoke/import tool. No account is configured by default.
use dpda_data_provider::OpenDotaClient;
use dpda_domain::normalize_account_id;
use dpda_history::{sync_history, SyncOptions};
use dpda_persistence::Database;
use std::sync::atomic::AtomicBool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 3 {
        return Err("用法：cargo run -p dpda-history --example sync -- ACCOUNT_ID DATABASE_PATH [MAX_PAGES] [PAGE_SIZE]".into());
    }
    let account_id = normalize_account_id(&args[1])?;
    let now = chrono::Utc::now().timestamp();
    let options = SyncOptions {
        account_id,
        from_time: now - 5 * 366 * 86400,
        to_time: now,
        page_size: args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(100),
        max_pages: args.get(3).map(|s| s.parse()).transpose()?,
    };
    let mut db = Database::open(&args[2])?;
    let client = OpenDotaClient::new()?;
    let result = sync_history(
        &client,
        &mut db,
        options,
        &AtomicBool::new(false),
        |progress| {
            println!(
                "{}",
                serde_json::to_string(progress).expect("serializable progress")
            )
        },
    )
    .await?;
    println!("status={}, stored={}", result.status, db.count(account_id)?);
    Ok(())
}
